//! Import extraction from JavaScript/TypeScript files using ast-grep.
//!
//! Parses source files and extracts import statements, resolving them to file paths.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ast_grep_core::{AstGrep, Pattern, StrDoc};
use ast_grep_language::SupportLang;
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::ExtractError;
use crate::tsconfig::TsConfig;
use crate::workspace::Workspace;

/// Cache for filesystem resolution results to avoid redundant stat calls.
///
/// Maps base path (without extension) to resolved path (with extension).
type ResolutionCache = Arc<Mutex<HashMap<PathBuf, Option<PathBuf>>>>;

/// Type alias for ast-grep document type.
type SgLang = StrDoc<SupportLang>;

/// Controls which imports are extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionMode {
    /// Extract and resolve all imports.
    #[default]
    All,
    /// Only keep imports that target workspace packages.
    WorkspaceOnly,
}

/// An extracted import statement.
#[derive(Debug, Clone)]
pub struct Import {
    /// File containing the import.
    pub source: PathBuf,

    /// What's being imported.
    pub target: ImportTarget,

    /// Original import specifier string.
    pub specifier: String,
}

/// Classification of an import target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportTarget {
    /// Successfully resolved to a file path.
    Resolved(PathBuf),

    /// External module (node_modules, not a workspace package).
    External(String),

    /// Workspace package import.
    WorkspacePackage {
        /// The package name (e.g., "@myorg/utils").
        package_name: String,
        /// Optional subpath (e.g., "helpers" from "@myorg/utils/helpers").
        subpath: Option<String>,
    },

    /// Could not resolve the import.
    Unresolved(String),
}

/// Import extraction engine.
pub struct ImportExtractor {
    /// File extensions to analyze.
    extensions: Vec<String>,

    /// TypeScript configuration for path alias resolution.
    tsconfig: Option<TsConfig>,

    /// Workspace configuration for workspace package detection.
    workspace: Option<Workspace>,

    /// Controls whether to resolve all imports or only workspace imports.
    mode: ExtractionMode,

    /// Glob patterns for files to ignore.
    ignore_patterns: Vec<glob::Pattern>,

    /// Cache for extension resolution to avoid redundant filesystem stat calls.
    resolution_cache: ResolutionCache,
}

impl ImportExtractor {
    /// Create a new import extractor.
    #[inline]
    pub fn new(extensions: Vec<String>, tsconfig: Option<TsConfig>) -> Self {
        Self {
            extensions,
            tsconfig,
            workspace: None,
            mode: ExtractionMode::All,
            ignore_patterns: Vec::new(),
            resolution_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set workspace configuration for workspace package detection.
    #[inline]
    #[must_use]
    pub fn with_workspace(mut self, workspace: Option<Workspace>) -> Self {
        self.workspace = workspace;
        self
    }

    /// Set extraction mode.
    #[inline]
    #[must_use]
    pub fn with_mode(mut self, mode: ExtractionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set ignore patterns for files to skip.
    #[inline]
    #[must_use]
    pub fn with_ignore_patterns(mut self, patterns: Vec<String>) -> Self {
        self.ignore_patterns = patterns
            .iter()
            .filter_map(|p| glob::Pattern::new(p).ok())
            .collect();
        self
    }

    /// Extract all imports from files in a package directory.
    ///
    /// Uses parallel file I/O for improved performance on large packages.
    ///
    /// # Errors
    ///
    /// Returns an error if file reading or parsing fails.
    #[inline]
    pub fn extract(&self, package_path: &Path) -> Result<Vec<Import>, ExtractError> {
        // Collect all file paths first
        let files: Vec<PathBuf> = WalkDir::new(package_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !self.should_skip(entry.path()))
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_type().is_dir())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| self.extensions.iter().any(|expected| expected == ext))
            })
            .map(|entry| entry.into_path())
            .collect();

        // Process files in parallel
        let results: Vec<Result<Vec<Import>, ExtractError>> = files
            .par_iter()
            .map(|path| self.extract_from_file(path))
            .collect();

        // Aggregate results, propagating first error
        let mut imports = Vec::new();
        for result in results {
            imports.extend(result?);
        }

        Ok(imports)
    }

    /// Check if a path should be skipped.
    fn should_skip(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Always skip node_modules, dist, build
        if path_str.contains("node_modules")
            || path_str.ends_with("/dist")
            || path_str.ends_with("/build")
            || path_str.ends_with("/.next")
            || path_str.contains("/.git/")
        {
            return true;
        }

        // Check ignore patterns
        for pattern in &self.ignore_patterns {
            if pattern.matches(&path_str) {
                return true;
            }
        }

        false
    }

    /// Extract imports from a single file.
    fn extract_from_file(&self, file_path: &Path) -> Result<Vec<Import>, ExtractError> {
        let source = fs::read_to_string(file_path).map_err(|source| ExtractError::IoError {
            path: file_path.to_path_buf(),
            source,
        })?;

        let lang = Self::detect_language(file_path);
        let ast = AstGrep::new(&source, lang);

        let mut imports = Vec::new();

        // Extract static imports: import { x } from './y'
        imports.extend(self.extract_static_imports(&ast, file_path, lang));

        // Extract re-exports: export { x } from './y' and export * from './y'
        imports.extend(self.extract_reexports(&ast, file_path, lang));

        // Extract dynamic imports: import('./y')
        imports.extend(self.extract_dynamic_imports(&ast, file_path, lang));

        // Extract require calls: require('./y')
        imports.extend(self.extract_require_calls(&ast, file_path, lang));

        Ok(imports)
    }

    /// Detect the ast-grep language from file extension.
    fn detect_language(path: &Path) -> SupportLang {
        match path.extension().and_then(|e| e.to_str()) {
            Some("tsx" | "jsx") => SupportLang::Tsx, // JSX uses TSX parser
            Some("js" | "mjs" | "cjs") => SupportLang::JavaScript,
            _ => SupportLang::TypeScript, // ts and unknown default to TypeScript
        }
    }

    /// Extract static import statements.
    fn extract_static_imports(
        &self,
        ast: &AstGrep<SgLang>,
        file_path: &Path,
        lang: SupportLang,
    ) -> Vec<Import> {
        let mut imports = Vec::new();

        // Match import declarations via AST traversal
        for node in ast.root().children() {
            if node.kind() != "import_statement" {
                continue;
            }
            if let Some(import) = self.extract_import_from_statement(&node, file_path) {
                imports.push(import);
            }
        }

        // Also try pattern matching for imports
        self.extract_via_pattern(
            ast,
            file_path,
            lang,
            "import $$$SPECIFIERS from $SOURCE",
            &mut imports,
        );

        imports
    }

    /// Extract import from an import statement node.
    fn extract_import_from_statement(
        &self,
        node: &ast_grep_core::Node<'_, SgLang>,
        file_path: &Path,
    ) -> Option<Import> {
        for child in node.children() {
            if child.kind() == "string" || child.kind() == "string_fragment" {
                return self.create_import_from_specifier_text(&child.text(), file_path);
            }
        }
        None
    }

    /// Create an import from a specifier text (with quotes).
    fn create_import_from_specifier_text(&self, text: &str, file_path: &Path) -> Option<Import> {
        let specifier = text
            .trim_start_matches(['"', '\''])
            .trim_end_matches(['"', '\''])
            .to_owned();

        if specifier.is_empty() {
            return None;
        }

        if self.mode == ExtractionMode::WorkspaceOnly {
            return self.create_workspace_import(&specifier, file_path);
        }

        let target = self.resolve_import(file_path, &specifier);
        Some(Import {
            source: file_path.to_path_buf(),
            target,
            specifier,
        })
    }

    /// Create an import only when it targets a workspace package.
    fn create_workspace_import(&self, specifier: &str, file_path: &Path) -> Option<Import> {
        if specifier.starts_with('.') || specifier.starts_with('/') {
            return None;
        }

        let workspace = self.workspace.as_ref()?;
        if !workspace.is_workspace_package(specifier) {
            return None;
        }

        let package_name = workspace.resolve_package_name(specifier).to_owned();
        let subpath = Workspace::extract_subpath(specifier).map(str::to_owned);

        Some(Import {
            source: file_path.to_path_buf(),
            target: ImportTarget::WorkspacePackage {
                package_name,
                subpath,
            },
            specifier: specifier.to_owned(),
        })
    }

    /// Extract imports via ast-grep pattern matching.
    fn extract_via_pattern(
        &self,
        ast: &AstGrep<SgLang>,
        file_path: &Path,
        lang: SupportLang,
        pattern_str: &str,
        imports: &mut Vec<Import>,
    ) {
        let Ok(pattern) = Pattern::try_new(pattern_str, lang) else {
            return;
        };
        for m in ast.root().find_all(&pattern) {
            let Some(source_node) = m.get_env().get_match("SOURCE") else {
                continue;
            };
            let text = source_node.text();

            // Only accept string literals (must start with quote)
            if !text.starts_with('"') && !text.starts_with('\'') {
                continue;
            }

            let specifier = text
                .trim_start_matches(['"', '\''])
                .trim_end_matches(['"', '\''])
                .to_owned();

            // Avoid duplicates and empty specifiers
            if specifier.is_empty() || imports.iter().any(|i| i.specifier == specifier) {
                continue;
            }
            let target = self.resolve_import(file_path, &specifier);
            imports.push(Import {
                source: file_path.to_path_buf(),
                target,
                specifier,
            });
        }
    }

    /// Extract re-export statements.
    ///
    /// Handles both named re-exports (`export { x } from './y'`) and
    /// star re-exports (`export * from './y'`).
    fn extract_reexports(
        &self,
        ast: &AstGrep<SgLang>,
        file_path: &Path,
        _lang: SupportLang,
    ) -> Vec<Import> {
        let mut imports = Vec::new();

        // Traverse export_statement nodes directly
        for node in ast.root().children() {
            if node.kind() != "export_statement" {
                continue;
            }

            // Check if this is a re-export (has "from" keyword)
            let has_from = node.children().any(|c| c.kind() == "from");
            if !has_from {
                continue;
            }

            // Find the string literal source and create import
            if let Some(import) = self.extract_reexport_source(&node, file_path) {
                imports.push(import);
            }
        }

        imports
    }

    /// Extract the source string from a re-export statement node.
    fn extract_reexport_source(
        &self,
        node: &ast_grep_core::Node<'_, SgLang>,
        file_path: &Path,
    ) -> Option<Import> {
        for child in node.children() {
            if child.kind() == "string" {
                return self.create_import_from_specifier_text(&child.text(), file_path);
            }
        }
        None
    }

    /// Extract dynamic import() calls.
    fn extract_dynamic_imports(
        &self,
        ast: &AstGrep<SgLang>,
        file_path: &Path,
        lang: SupportLang,
    ) -> Vec<Import> {
        let mut imports = Vec::new();
        self.extract_string_literal_pattern(ast, file_path, lang, "import($SOURCE)", &mut imports);
        imports
    }

    /// Extract require() calls.
    fn extract_require_calls(
        &self,
        ast: &AstGrep<SgLang>,
        file_path: &Path,
        lang: SupportLang,
    ) -> Vec<Import> {
        let mut imports = Vec::new();
        self.extract_string_literal_pattern(ast, file_path, lang, "require($SOURCE)", &mut imports);
        imports
    }

    /// Extract imports from a pattern where SOURCE must be a string literal.
    fn extract_string_literal_pattern(
        &self,
        ast: &AstGrep<SgLang>,
        file_path: &Path,
        lang: SupportLang,
        pattern_str: &str,
        imports: &mut Vec<Import>,
    ) {
        let Ok(pattern) = Pattern::try_new(pattern_str, lang) else {
            return;
        };
        for m in ast.root().find_all(&pattern) {
            let Some(source_node) = m.get_env().get_match("SOURCE") else {
                continue;
            };
            let text = source_node.text();
            // Only handle string literals
            if !text.starts_with('"') && !text.starts_with('\'') {
                continue;
            }
            if let Some(import) = self.create_import_from_specifier_text(&text, file_path) {
                imports.push(import);
            }
        }
    }

    /// Resolve an import specifier to a file path.
    fn resolve_import(&self, from: &Path, specifier: &str) -> ImportTarget {
        // External module (node_modules or scoped package)
        if !specifier.starts_with('.') && !specifier.starts_with('/') {
            return self.resolve_external_or_alias(specifier);
        }

        // Relative import
        self.resolve_relative_import(from, specifier)
    }

    /// Resolve an external module, tsconfig alias, or workspace package.
    fn resolve_external_or_alias(&self, specifier: &str) -> ImportTarget {
        // Check if it's a workspace package FIRST (before tsconfig aliases)
        // This is important because tsconfig aliases might resolve to a path within
        // a workspace package, but we want to track package-level dependencies.
        if let Some(workspace) = &self.workspace
            && workspace.is_workspace_package(specifier)
        {
            // Use resolve_package_name to handle tsconfig aliases
            let package_name = workspace.resolve_package_name(specifier).to_owned();
            let subpath = Workspace::extract_subpath(specifier).map(str::to_owned);
            return ImportTarget::WorkspacePackage {
                package_name,
                subpath,
            };
        }

        // Check if it's a tsconfig path alias (for non-workspace imports)
        if let Some(tsconfig) = &self.tsconfig
            && let Some(resolved) = tsconfig.resolve_alias(specifier)
        {
            return ImportTarget::Resolved(resolved);
        }

        ImportTarget::External(specifier.to_owned())
    }

    /// Resolve a relative import specifier.
    fn resolve_relative_import(&self, from: &Path, specifier: &str) -> ImportTarget {
        let base_dir = from.parent().unwrap_or_else(|| Path::new("."));
        let resolved_base = Self::normalize_path(&base_dir.join(specifier));

        // Try to resolve with various extensions (using cache)
        if let Some(resolved) = self.try_resolve_with_ext_cached(&resolved_base) {
            return ImportTarget::Resolved(resolved);
        }

        // Try as directory with index file (using cache)
        if let Some(index) = self.try_resolve_index_cached(&resolved_base) {
            return ImportTarget::Resolved(index);
        }

        // Try resolving with extensions when path doesn't exist (for reporting)
        if !resolved_base.to_string_lossy().contains('.') {
            let candidate = PathBuf::from(format!("{}.ts", resolved_base.display()));
            return ImportTarget::Resolved(candidate);
        }

        ImportTarget::Unresolved(specifier.to_owned())
    }

    /// Try to resolve with various extensions, using cache to avoid redundant fs calls.
    fn try_resolve_with_ext_cached(&self, base: &Path) -> Option<PathBuf> {
        // Check cache first
        {
            let cache = self
                .resolution_cache
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            if let Some(cached) = cache.get(base) {
                return cached.clone();
            }
        }

        // Perform actual filesystem resolution
        let result = Self::try_resolve_with_ext(base);

        // Store in cache
        drop(
            self.resolution_cache
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .insert(base.to_path_buf(), result.clone()),
        );

        result
    }

    /// Try to resolve with various extensions (uncached).
    fn try_resolve_with_ext(base: &Path) -> Option<PathBuf> {
        for ext in &["", ".ts", ".tsx", ".js", ".jsx"] {
            let candidate = if ext.is_empty() {
                base.to_path_buf()
            } else {
                PathBuf::from(format!("{}{ext}", base.display()))
            };
            if candidate.exists() && candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    /// Try to resolve as directory with index file, using cache.
    fn try_resolve_index_cached(&self, path: &Path) -> Option<PathBuf> {
        let cache_key = path.join("__index__");

        // Check cache first
        {
            let cache = self
                .resolution_cache
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            if let Some(cached) = cache.get(&cache_key) {
                return cached.clone();
            }
        }

        // Perform actual filesystem resolution
        let result = Self::try_resolve_index(path);

        // Store in cache
        drop(
            self.resolution_cache
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .insert(cache_key, result.clone()),
        );

        result
    }

    /// Try to resolve as directory with index file (uncached).
    fn try_resolve_index(path: &Path) -> Option<PathBuf> {
        if !path.is_dir() {
            return None;
        }
        for index in &["index.ts", "index.tsx", "index.js", "index.jsx"] {
            let candidate = path.join(index);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    /// Normalize a path by removing `.` and resolving `..` components.
    fn normalize_path(path: &Path) -> PathBuf {
        use std::path::Component;
        let mut components: Vec<Component<'_>> = Vec::new();
        for component in path.components() {
            match component {
                Component::CurDir => {},
                Component::ParentDir if !components.is_empty() => {
                    let _ = components.pop();
                },
                comp @ (Component::Prefix(_)
                | Component::RootDir
                | Component::ParentDir
                | Component::Normal(_)) => components.push(comp),
            }
        }
        components.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_external_detection() {
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);
        let target = extractor.resolve_import(Path::new("/project/src/index.ts"), "react");
        assert!(
            matches!(target, ImportTarget::External(s) if s == "react"),
            "expected External for 'react'"
        );
    }

    #[test]
    fn test_scoped_package_detection() {
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);
        let target =
            extractor.resolve_import(Path::new("/project/src/index.ts"), "@myorg/shared-utils");
        assert!(
            matches!(target, ImportTarget::External(s) if s == "@myorg/shared-utils"),
            "expected External for scoped package"
        );
    }

    #[test]
    fn test_relative_import() {
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);
        let target = extractor.resolve_import(Path::new("/project/src/index.ts"), "./utils");
        // Will be Resolved (even if doesn't exist) or Unresolved
        assert!(
            !matches!(target, ImportTarget::External(_)),
            "relative import should not be External"
        );
    }

    #[test]
    fn test_reexport_extraction() {
        use ast_grep_core::AstGrep;

        let code = r"export { helper } from './helper';
export * from './utils';";

        let ast = AstGrep::new(code, SupportLang::TypeScript);
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);

        let imports =
            extractor.extract_reexports(&ast, Path::new("/test/file.ts"), SupportLang::TypeScript);

        let specifiers: Vec<_> = imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(
            specifiers.contains(&"./helper"),
            "should extract './helper' from named re-export, got: {specifiers:?}"
        );
        assert!(
            specifiers.contains(&"./utils"),
            "should extract './utils' from star re-export, got: {specifiers:?}"
        );
    }

    // =========================================================================
    // Additional import extraction tests for comprehensive coverage
    // =========================================================================

    /// Creates a test workspace with common packages.
    fn create_test_workspace() -> Workspace {
        use crate::workspace::WorkspaceFormat;
        use std::collections::HashMap;

        let mut packages = HashMap::new();
        let _ = packages.insert("@myorg/utils".to_owned(), PathBuf::from("/pkg/utils"));
        let _ = packages.insert("@myorg/shared".to_owned(), PathBuf::from("/pkg/shared"));
        let _ = packages.insert("common".to_owned(), PathBuf::from("/pkg/common"));

        Workspace {
            root: PathBuf::from("/"),
            format: WorkspaceFormat::Npm,
            packages,
            aliases: HashMap::new(),
        }
    }

    #[test]
    fn test_workspace_package_scoped_no_subpath() {
        let workspace = create_test_workspace();
        let extractor =
            ImportExtractor::new(vec!["ts".to_owned()], None).with_workspace(Some(workspace));

        let target = extractor.resolve_import(Path::new("/app/index.ts"), "@myorg/utils");
        assert!(
            matches!(
                &target,
                ImportTarget::WorkspacePackage {
                    package_name,
                    subpath: None
                } if package_name == "@myorg/utils"
            ),
            "should detect @myorg/utils as workspace package, got: {target:?}"
        );
    }

    #[test]
    fn test_workspace_package_scoped_with_subpath() {
        let workspace = create_test_workspace();
        let extractor =
            ImportExtractor::new(vec!["ts".to_owned()], None).with_workspace(Some(workspace));

        let target = extractor.resolve_import(Path::new("/app/index.ts"), "@myorg/shared/helpers");
        assert!(
            matches!(
                &target,
                ImportTarget::WorkspacePackage {
                    package_name,
                    subpath: Some(sub)
                } if package_name == "@myorg/shared" && sub == "helpers"
            ),
            "should detect @myorg/shared/helpers with subpath, got: {target:?}"
        );
    }

    #[test]
    fn test_workspace_package_unscoped() {
        let workspace = create_test_workspace();
        let extractor =
            ImportExtractor::new(vec!["ts".to_owned()], None).with_workspace(Some(workspace));

        let target = extractor.resolve_import(Path::new("/app/index.ts"), "common");
        assert!(
            matches!(
                &target,
                ImportTarget::WorkspacePackage {
                    package_name,
                    subpath: None
                } if package_name == "common"
            ),
            "should detect common as workspace package, got: {target:?}"
        );
    }

    #[test]
    fn test_workspace_external_package() {
        let workspace = create_test_workspace();
        let extractor =
            ImportExtractor::new(vec!["ts".to_owned()], None).with_workspace(Some(workspace));

        let target = extractor.resolve_import(Path::new("/app/index.ts"), "react");
        assert!(
            matches!(&target, ImportTarget::External(name) if name == "react"),
            "should detect react as external, got: {target:?}"
        );
    }

    #[test]
    fn test_workspace_package_deep_subpath() {
        use crate::workspace::WorkspaceFormat;
        use std::collections::HashMap;

        let mut packages = HashMap::new();
        let _ = packages.insert("@myorg/utils".to_owned(), PathBuf::from("/pkg/utils"));

        let ws = Workspace {
            root: PathBuf::from("/"),
            format: WorkspaceFormat::Npm,
            packages,
            aliases: HashMap::new(),
        };

        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None).with_workspace(Some(ws));

        let target =
            extractor.resolve_import(Path::new("/app/index.ts"), "@myorg/utils/deep/nested/path");
        assert!(
            matches!(
                &target,
                ImportTarget::WorkspacePackage {
                    package_name,
                    subpath: Some(sub)
                } if package_name == "@myorg/utils" && sub == "deep/nested/path"
            ),
            "should extract deep nested subpath, got: {target:?}"
        );
    }

    #[test]
    fn test_workspace_only_mode_skips_non_workspace_imports() {
        let workspace = create_test_workspace();
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None)
            .with_workspace(Some(workspace))
            .with_mode(ExtractionMode::WorkspaceOnly);

        let workspace_import = extractor.create_import_from_specifier_text(
            "\"@myorg/shared/helpers\"",
            Path::new("/app/index.ts"),
        );
        assert!(
            matches!(
                workspace_import.as_ref().map(|import| &import.target),
                Some(ImportTarget::WorkspacePackage {
                    package_name,
                    subpath: Some(subpath),
                }) if package_name == "@myorg/shared" && subpath == "helpers"
            ),
            "workspace-only mode should keep workspace imports, got: {workspace_import:?}"
        );

        let relative_import = extractor
            .create_import_from_specifier_text("\"./local-helper\"", Path::new("/app/index.ts"));
        assert!(
            relative_import.is_none(),
            "workspace-only mode should skip relative imports"
        );

        let external_import =
            extractor.create_import_from_specifier_text("\"react\"", Path::new("/app/index.ts"));
        assert!(
            external_import.is_none(),
            "workspace-only mode should skip external imports"
        );
    }

    #[test]
    fn test_import_target_equality() {
        let resolved1 = ImportTarget::Resolved(PathBuf::from("a.ts"));
        let resolved2 = ImportTarget::Resolved(PathBuf::from("a.ts"));
        let resolved3 = ImportTarget::Resolved(PathBuf::from("b.ts"));

        assert_eq!(resolved1, resolved2, "same resolved path should be equal");
        assert_ne!(
            resolved1, resolved3,
            "different resolved paths should not be equal"
        );

        let external1 = ImportTarget::External("react".to_owned());
        let external2 = ImportTarget::External("react".to_owned());
        let external3 = ImportTarget::External("vue".to_owned());

        assert_eq!(external1, external2, "same external should be equal");
        assert_ne!(
            external1, external3,
            "different externals should not be equal"
        );

        let ws1 = ImportTarget::WorkspacePackage {
            package_name: "@pkg/a".to_owned(),
            subpath: Some("utils".to_owned()),
        };
        let ws2 = ImportTarget::WorkspacePackage {
            package_name: "@pkg/a".to_owned(),
            subpath: Some("utils".to_owned()),
        };
        let ws3 = ImportTarget::WorkspacePackage {
            package_name: "@pkg/a".to_owned(),
            subpath: None,
        };

        assert_eq!(ws1, ws2, "same workspace package should be equal");
        assert_ne!(ws1, ws3, "different subpaths should not be equal");
    }

    #[test]
    fn test_import_clone() {
        let import = Import {
            source: PathBuf::from("index.ts"),
            target: ImportTarget::WorkspacePackage {
                package_name: "@myorg/utils".to_owned(),
                subpath: Some("helpers".to_owned()),
            },
            specifier: "@myorg/utils/helpers".to_owned(),
        };

        let cloned = import.clone();
        assert_eq!(cloned.source, import.source, "cloned source should match");
        assert_eq!(cloned.target, import.target, "cloned target should match");
        assert_eq!(
            cloned.specifier, import.specifier,
            "cloned specifier should match"
        );
    }

    #[test]
    fn test_no_workspace_external_detection() {
        // Without workspace, all non-relative imports should be external
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);

        let target = extractor.resolve_import(Path::new("/app/index.ts"), "@myorg/utils");
        assert!(
            matches!(target, ImportTarget::External(name) if name == "@myorg/utils"),
            "without workspace, should detect as external"
        );
    }

    #[test]
    fn test_with_ignore_patterns() {
        // This test ensures the builder pattern works without panicking
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None)
            .with_ignore_patterns(vec!["*.test.ts".to_owned(), "**/__tests__/**".to_owned()]);

        // Verify the extractor was created (ignore patterns are private, so we just verify construction)
        assert!(
            !extractor.extensions.is_empty(),
            "extractor should have extensions set"
        );
    }

    #[test]
    fn test_static_import_extraction() {
        use ast_grep_core::AstGrep;

        let code = r"import { foo } from './foo';
import bar from './bar';
import * as utils from './utils';
import type { Type } from './types';";

        let ast = AstGrep::new(code, SupportLang::TypeScript);
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);

        let imports = extractor.extract_static_imports(
            &ast,
            Path::new("/test/file.ts"),
            SupportLang::TypeScript,
        );

        let specifiers: Vec<_> = imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(
            specifiers.contains(&"./foo"),
            "should extract './foo', got: {specifiers:?}"
        );
        assert!(
            specifiers.contains(&"./bar"),
            "should extract './bar', got: {specifiers:?}"
        );
        assert!(
            specifiers.contains(&"./utils"),
            "should extract './utils', got: {specifiers:?}"
        );
    }

    #[test]
    fn test_dynamic_import_extraction() {
        use ast_grep_core::AstGrep;

        let code = r"const mod = import('./dynamic');
const lazy = import('./lazy-module');
async function load() { return import('./async'); }";

        let ast = AstGrep::new(code, SupportLang::TypeScript);
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);

        let imports = extractor.extract_dynamic_imports(
            &ast,
            Path::new("/test/file.ts"),
            SupportLang::TypeScript,
        );

        let specifiers: Vec<_> = imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(
            specifiers.contains(&"./dynamic"),
            "should extract dynamic import './dynamic', got: {specifiers:?}"
        );
        assert!(
            specifiers.contains(&"./lazy-module"),
            "should extract dynamic import './lazy-module', got: {specifiers:?}"
        );
    }

    #[test]
    fn test_require_extraction() {
        use ast_grep_core::AstGrep;

        let code = r"const a = require('./module-a');
const { b } = require('./module-b');
require('./side-effect');";

        let ast = AstGrep::new(code, SupportLang::JavaScript);
        let extractor = ImportExtractor::new(vec!["js".to_owned()], None);

        let imports = extractor.extract_require_calls(
            &ast,
            Path::new("/test/file.js"),
            SupportLang::JavaScript,
        );

        let specifiers: Vec<_> = imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(
            specifiers.contains(&"./module-a"),
            "should extract require './module-a', got: {specifiers:?}"
        );
        assert!(
            specifiers.contains(&"./module-b"),
            "should extract require './module-b', got: {specifiers:?}"
        );
    }

    #[test]
    fn test_mixed_import_styles() {
        use ast_grep_core::AstGrep;

        let code = r"import { a } from './static';
export { b } from './reexport';
const c = import('./dynamic');
const d = require('./cjs');";

        let ast = AstGrep::new(code, SupportLang::TypeScript);
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);

        let static_imports = extractor.extract_static_imports(
            &ast,
            Path::new("/test/file.ts"),
            SupportLang::TypeScript,
        );
        let reexports =
            extractor.extract_reexports(&ast, Path::new("/test/file.ts"), SupportLang::TypeScript);
        let dynamic = extractor.extract_dynamic_imports(
            &ast,
            Path::new("/test/file.ts"),
            SupportLang::TypeScript,
        );
        let requires = extractor.extract_require_calls(
            &ast,
            Path::new("/test/file.ts"),
            SupportLang::TypeScript,
        );

        assert!(!static_imports.is_empty(), "should have static imports");
        assert!(!reexports.is_empty(), "should have reexports");
        assert!(!dynamic.is_empty(), "should have dynamic imports");
        assert!(!requires.is_empty(), "should have require calls");
    }

    #[test]
    fn test_absolute_path_import() {
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);

        // Absolute paths starting with / are relative to some base
        let target = extractor.resolve_import(Path::new("/project/src/index.ts"), "/utils");
        assert!(
            !matches!(target, ImportTarget::External(_)),
            "absolute path starting with / should not be external"
        );
    }

    #[test]
    fn test_parent_directory_import() {
        let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);

        let target = extractor.resolve_import(Path::new("/project/src/deep/index.ts"), "../utils");
        assert!(
            !matches!(target, ImportTarget::External(_)),
            "parent directory import should not be external"
        );
    }
}
