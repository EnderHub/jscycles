//! Workspace detection and parsing for monorepos.
//!
//! Supports npm/yarn workspaces, pnpm workspaces, and TypeScript project references.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use crate::WorkspaceError;

/// Workspace configuration format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceFormat {
    /// npm/yarn/bun: package.json "workspaces" field.
    Npm,
    /// pnpm: pnpm-workspace.yaml file.
    Pnpm,
    /// TypeScript: tsconfig.json "references" field.
    TypeScript,
}

/// A detected monorepo workspace.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Root directory of the workspace.
    pub root: PathBuf,

    /// Detected workspace format.
    pub format: WorkspaceFormat,

    /// Mapping from package name to package directory path.
    pub packages: HashMap<String, PathBuf>,

    /// Mapping from tsconfig path aliases to package names.
    /// e.g., `@ender/shared-utils-system-test` -> `@ender/shared-utils-test`
    pub aliases: HashMap<String, String>,
}

/// npm/yarn package.json with workspaces field.
#[derive(Debug, Deserialize)]
struct NpmPackageJson {
    /// Workspaces can be an array or object with packages field.
    workspaces: Option<WorkspacesField>,
}

/// Workspaces field can be array or object.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WorkspacesField {
    /// Simple array of glob patterns.
    Array(Vec<String>),
    /// Object with packages array.
    Object { packages: Vec<String> },
}

/// pnpm-workspace.yaml structure.
#[derive(Debug, Deserialize)]
struct PnpmWorkspace {
    /// Package glob patterns.
    packages: Option<Vec<String>>,
}

/// tsconfig.json with references.
#[derive(Debug, Deserialize)]
struct TsConfigWithRefs {
    /// Project references.
    references: Option<Vec<TsReference>>,
}

/// TypeScript project reference.
#[derive(Debug, Deserialize)]
struct TsReference {
    /// Path to referenced project.
    path: String,
}

/// Minimal package.json for reading name.
#[derive(Debug, Deserialize)]
struct MinimalPackageJson {
    /// Package name.
    name: Option<String>,
}

/// tsconfig.json with paths for alias detection.
#[derive(Debug, Deserialize)]
struct TsConfigWithPaths {
    /// Compiler options containing paths.
    #[serde(rename = "compilerOptions")]
    compiler_options: Option<TsCompilerOptionsWithPaths>,
}

/// Compiler options with paths field.
#[derive(Debug, Deserialize)]
struct TsCompilerOptionsWithPaths {
    /// Path aliases mapping.
    paths: Option<HashMap<String, Vec<String>>>,
}

impl Workspace {
    /// Discover workspace configuration starting from a directory.
    ///
    /// Searches upward for workspace root, checking in order:
    /// 1. pnpm-workspace.yaml
    /// 2. package.json with workspaces field
    /// 3. tsconfig.json with references field
    ///
    /// # Errors
    ///
    /// Returns an error if config parsing fails.
    #[inline]
    pub fn discover(start: &Path) -> Result<Option<Self>, WorkspaceError> {
        let mut current = if start.is_absolute() {
            start.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|err| WorkspaceError::Read {
                    path: start.to_path_buf(),
                    source: err,
                })?
                .join(start)
        };

        // Search upward for workspace root
        loop {
            if let Some(workspace) = Self::try_detect_at(&current)? {
                return Ok(Some(workspace));
            }

            let Some(parent) = current.parent() else {
                break;
            };
            if parent == current {
                break;
            }
            current = parent.to_path_buf();
        }

        Ok(None)
    }

    /// Try to detect workspace at a specific directory.
    fn try_detect_at(dir: &Path) -> Result<Option<Self>, WorkspaceError> {
        // Try pnpm first (most explicit)
        if let Some(workspace) = Self::try_pnpm(dir)? {
            return Ok(Some(workspace));
        }

        // Try npm/yarn workspaces
        if let Some(workspace) = Self::try_npm(dir)? {
            return Ok(Some(workspace));
        }

        // Try TypeScript references
        if let Some(workspace) = Self::try_typescript(dir)? {
            return Ok(Some(workspace));
        }

        Ok(None)
    }

    /// Try to parse pnpm-workspace.yaml.
    fn try_pnpm(dir: &Path) -> Result<Option<Self>, WorkspaceError> {
        let yaml_path = dir.join("pnpm-workspace.yaml");
        if !yaml_path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&yaml_path).map_err(|err| WorkspaceError::Read {
            path: yaml_path.clone(),
            source: err,
        })?;

        let config: PnpmWorkspace =
            serde_yaml::from_str(&contents).map_err(|err| WorkspaceError::Parse {
                path: yaml_path,
                message: err.to_string(),
            })?;

        let patterns = config.packages.unwrap_or_default();
        let packages = Self::expand_patterns(dir, &patterns)?;
        let aliases = Self::load_aliases(dir, &packages);

        Ok(Some(Self {
            root: dir.to_path_buf(),
            format: WorkspaceFormat::Pnpm,
            packages,
            aliases,
        }))
    }

    /// Try to parse npm/yarn workspaces from package.json.
    fn try_npm(dir: &Path) -> Result<Option<Self>, WorkspaceError> {
        let pkg_path = dir.join("package.json");
        if !pkg_path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&pkg_path).map_err(|err| WorkspaceError::Read {
            path: pkg_path.clone(),
            source: err,
        })?;

        let pkg: NpmPackageJson =
            serde_json::from_str(&contents).map_err(|err| WorkspaceError::Parse {
                path: pkg_path,
                message: err.to_string(),
            })?;

        let Some(workspaces) = pkg.workspaces else {
            return Ok(None);
        };

        let patterns = match workspaces {
            WorkspacesField::Array(arr) => arr,
            WorkspacesField::Object { packages } => packages,
        };

        let packages = Self::expand_patterns(dir, &patterns)?;
        let aliases = Self::load_aliases(dir, &packages);

        Ok(Some(Self {
            root: dir.to_path_buf(),
            format: WorkspaceFormat::Npm,
            packages,
            aliases,
        }))
    }

    /// Try to parse TypeScript project references from tsconfig.json.
    fn try_typescript(dir: &Path) -> Result<Option<Self>, WorkspaceError> {
        let ts_path = dir.join("tsconfig.json");
        if !ts_path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&ts_path).map_err(|err| WorkspaceError::Read {
            path: ts_path.clone(),
            source: err,
        })?;

        // Strip comments from JSON (tsconfig allows them)
        let stripped = Self::strip_json_comments(&contents);

        let config: TsConfigWithRefs =
            serde_json::from_str(&stripped).map_err(|err| WorkspaceError::Parse {
                path: ts_path,
                message: err.to_string(),
            })?;

        let Some(refs) = config.references else {
            return Ok(None);
        };

        if refs.is_empty() {
            return Ok(None);
        }

        let paths: Vec<String> = refs.into_iter().map(|r| r.path).collect();
        let packages = Self::resolve_ts_references(dir, &paths)?;
        let aliases = Self::load_aliases(dir, &packages);

        Ok(Some(Self {
            root: dir.to_path_buf(),
            format: WorkspaceFormat::TypeScript,
            packages,
            aliases,
        }))
    }

    /// Strip single-line and multi-line comments from JSON.
    fn strip_json_comments(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();
        let mut in_string = false;

        while let Some(ch) = chars.next() {
            if in_string {
                Self::handle_char_in_string(ch, &mut chars, &mut result, &mut in_string);
                continue;
            }

            if ch == '"' {
                in_string = true;
                result.push(ch);
            } else if ch == '/' {
                Self::handle_potential_comment(&mut chars, &mut result, ch);
            } else {
                result.push(ch);
            }
        }

        result
    }

    /// Handle a character while inside a string literal.
    fn handle_char_in_string(
        ch: char,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        result: &mut String,
        in_string: &mut bool,
    ) {
        result.push(ch);
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                result.push(next);
                let _ = chars.next();
            }
        } else if ch == '"' {
            *in_string = false;
        } else {
            // Regular character in string, already pushed above
        }
    }

    /// Handle potential comment start in JSON.
    fn handle_potential_comment(
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        result: &mut String,
        ch: char,
    ) {
        match chars.peek() {
            Some('/') => Self::skip_single_line_comment(chars, result),
            Some('*') => Self::skip_multi_line_comment(chars),
            _ => result.push(ch),
        }
    }

    /// Skip a single-line comment (`// ...`).
    fn skip_single_line_comment(
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        result: &mut String,
    ) {
        let _ = chars.next();
        for ch in chars.by_ref() {
            if ch == '\n' {
                result.push('\n');
                break;
            }
        }
    }

    /// Skip a multi-line comment (`/* ... */`).
    fn skip_multi_line_comment(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
        let _ = chars.next();
        while let Some(ch) = chars.next() {
            if ch == '*' && chars.peek() == Some(&'/') {
                let _ = chars.next();
                break;
            }
        }
    }

    /// Expand glob patterns to package name -> path mappings.
    fn expand_patterns(
        root: &Path,
        patterns: &[String],
    ) -> Result<HashMap<String, PathBuf>, WorkspaceError> {
        let mut packages = HashMap::new();
        let mut include_patterns = Vec::new();
        let mut exclude_patterns = Vec::new();

        for pattern in patterns {
            let stripped = pattern.strip_prefix('!').unwrap_or(pattern);
            let manifest_pattern = Self::manifest_pattern(stripped)?;
            if pattern.starts_with('!') {
                exclude_patterns.push(manifest_pattern);
            } else {
                include_patterns.push((stripped.to_owned(), manifest_pattern));
            }
        }

        for (pattern, manifest_pattern) in include_patterns {
            Self::expand_single_pattern(
                root,
                &pattern,
                &manifest_pattern,
                &exclude_patterns,
                &mut packages,
            )?;
        }

        Ok(packages)
    }

    /// Expand a single glob pattern.
    fn expand_single_pattern(
        root: &Path,
        pattern: &str,
        manifest_pattern: &glob::Pattern,
        exclude_patterns: &[glob::Pattern],
        packages: &mut HashMap<String, PathBuf>,
    ) -> Result<(), WorkspaceError> {
        let search_root = root.join(Self::pattern_base(pattern));
        if !search_root.exists() {
            return Ok(());
        }

        for entry in WalkDir::new(search_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !Self::is_in_excluded_dir(entry.path()))
            .flatten()
        {
            if entry.file_type().is_dir() || entry.file_name() != "package.json" {
                continue;
            }

            let Ok(relative_manifest) = entry.path().strip_prefix(root) else {
                continue;
            };
            let relative_manifest = relative_manifest.to_string_lossy().replace('\\', "/");

            if !manifest_pattern.matches(&relative_manifest)
                || exclude_patterns
                    .iter()
                    .any(|exclude| exclude.matches(&relative_manifest))
            {
                continue;
            }

            if let Some((name, path)) = Self::read_package_name(entry.path())? {
                let _ = packages.insert(name, path);
            }
        }

        Ok(())
    }

    /// Build a glob pattern that matches a package manifest path.
    fn manifest_pattern(pattern: &str) -> Result<glob::Pattern, WorkspaceError> {
        let manifest_pattern = format!("{}/package.json", pattern.trim_end_matches('/'));
        glob::Pattern::new(&manifest_pattern).map_err(|source| WorkspaceError::InvalidPattern {
            pattern: pattern.to_owned(),
            source,
        })
    }

    /// Extract the non-glob prefix of a workspace pattern.
    fn pattern_base(pattern: &str) -> PathBuf {
        let mut base = PathBuf::new();

        for component in pattern.split('/') {
            if component.is_empty() || Self::has_glob_chars(component) {
                break;
            }
            base.push(component);
        }

        if base.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            base
        }
    }

    /// Check whether a pattern component contains glob metacharacters.
    fn has_glob_chars(component: &str) -> bool {
        component.contains('*')
            || component.contains('?')
            || component.contains('[')
            || component.contains(']')
            || component.contains('{')
            || component.contains('}')
    }

    /// Check if a path is inside a node_modules or dist directory.
    ///
    /// These directories contain external dependencies or build outputs,
    /// not workspace packages.
    fn is_in_excluded_dir(path: &Path) -> bool {
        path.components().any(|c| {
            if let std::path::Component::Normal(name) = c {
                name == "node_modules"
                    || name == "dist"
                    || name == "build"
                    || name == ".git"
                    || name == ".next"
            } else {
                false
            }
        })
    }

    /// Read package name from a package.json path.
    fn read_package_name(pkg_json: &Path) -> Result<Option<(String, PathBuf)>, WorkspaceError> {
        let contents = fs::read_to_string(pkg_json).map_err(|err| WorkspaceError::Read {
            path: pkg_json.to_path_buf(),
            source: err,
        })?;

        let pkg: MinimalPackageJson =
            serde_json::from_str(&contents).map_err(|err| WorkspaceError::Parse {
                path: pkg_json.to_path_buf(),
                message: err.to_string(),
            })?;

        let Some(name) = pkg.name else {
            return Ok(None);
        };

        let path = pkg_json
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        Ok(Some((name, path)))
    }

    /// Resolve TypeScript project references to package mappings.
    fn resolve_ts_references(
        root: &Path,
        paths: &[String],
    ) -> Result<HashMap<String, PathBuf>, WorkspaceError> {
        let mut packages = HashMap::new();

        for ref_path in paths {
            let dir = root.join(ref_path);
            let pkg_json = dir.join("package.json");

            if pkg_json.exists()
                && let Some((name, path)) = Self::read_package_name(&pkg_json)?
            {
                let _ = packages.insert(name, path);
            }
        }

        Ok(packages)
    }

    /// Load tsconfig path aliases and map them to package names.
    ///
    /// Looks for tsconfig.base.json or tsconfig.json with paths, then maps
    /// each alias to the package that contains the target path.
    fn load_aliases(root: &Path, packages: &HashMap<String, PathBuf>) -> HashMap<String, String> {
        // Try tsconfig.base.json first (common in Nx), then tsconfig.json
        let tsconfig_paths = [root.join("tsconfig.base.json"), root.join("tsconfig.json")];

        for tsconfig_path in tsconfig_paths {
            if let Some(aliases) =
                Self::try_load_aliases_from_tsconfig(&tsconfig_path, root, packages)
            {
                return aliases;
            }
        }

        HashMap::new()
    }

    /// Try to load aliases from a specific tsconfig file.
    fn try_load_aliases_from_tsconfig(
        tsconfig_path: &Path,
        root: &Path,
        packages: &HashMap<String, PathBuf>,
    ) -> Option<HashMap<String, String>> {
        if !tsconfig_path.exists() {
            return None;
        }

        let contents = fs::read_to_string(tsconfig_path).ok()?;
        let stripped = Self::strip_json_comments(&contents);
        let config: TsConfigWithPaths = serde_json::from_str(&stripped).ok()?;
        let paths = config.compiler_options?.paths?;

        // Build a list of (relative_path, package_name) sorted by path length desc
        let pkg_paths = Self::build_sorted_package_paths(root, packages);

        // Build aliases from paths
        let aliases = Self::build_aliases_from_paths(&paths, &pkg_paths);

        // Return aliases even if empty (this is the right tsconfig to use)
        Some(aliases)
    }

    /// Build sorted list of (relative_path, package_name) pairs.
    fn build_sorted_package_paths(
        root: &Path,
        packages: &HashMap<String, PathBuf>,
    ) -> Vec<(String, String)> {
        // Collect into a vec and sort for deterministic iteration
        let mut items: Vec<_> = packages.iter().collect();
        items.sort_by_key(|(name, _)| *name);

        let mut pkg_paths: Vec<(String, String)> = items
            .into_iter()
            .filter_map(|(pkg_name, pkg_path)| {
                let rel_path = pkg_path.strip_prefix(root).ok()?;
                let rel_str = rel_path.to_string_lossy().to_string();
                Some((rel_str, pkg_name.clone()))
            })
            .collect();

        // Sort by path length descending to match most specific path first
        pkg_paths.sort_by(|first, second| second.0.len().cmp(&first.0.len()));
        pkg_paths
    }

    /// Build alias map from tsconfig paths.
    fn build_aliases_from_paths(
        paths: &HashMap<String, Vec<String>>,
        pkg_paths: &[(String, String)],
    ) -> HashMap<String, String> {
        // Sort paths for deterministic iteration
        let mut sorted_paths: Vec<_> = paths.iter().collect();
        sorted_paths.sort_by_key(|(alias, _)| *alias);

        let mut aliases = HashMap::new();

        for (alias, targets) in sorted_paths {
            Self::try_add_alias(alias, targets, pkg_paths, &mut aliases);
        }

        aliases
    }

    /// Try to add an alias for a tsconfig path entry.
    fn try_add_alias(
        alias: &str,
        targets: &[String],
        pkg_paths: &[(String, String)],
        aliases: &mut HashMap<String, String>,
    ) {
        // Skip wildcard aliases (e.g., "@ender/*")
        if alias.contains('*') {
            return;
        }

        let Some(target) = targets.first() else {
            return;
        };

        // Find which package contains this path
        let Some(pkg_name) = Self::find_package_for_target(target, pkg_paths) else {
            return;
        };

        // Don't create alias if it matches the package name
        if alias == pkg_name {
            return;
        }

        let _ = aliases.insert(alias.to_owned(), pkg_name);
    }

    /// Find the package that contains the given target path.
    fn find_package_for_target(target: &str, pkg_paths: &[(String, String)]) -> Option<String> {
        for (rel_str, pkg_name) in pkg_paths {
            if target.starts_with(rel_str.as_str()) {
                return Some(pkg_name.clone());
            }
        }
        None
    }

    /// Check if an import specifier refers to a workspace package.
    ///
    /// Checks both direct package names and tsconfig path aliases.
    #[inline]
    pub fn is_workspace_package(&self, specifier: &str) -> bool {
        let package_name = Self::extract_package_name(specifier);

        // Direct package match
        if self.packages.contains_key(package_name) {
            return true;
        }

        // Check if it's an alias that maps to a workspace package
        if let Some(real_name) = self.aliases.get(package_name) {
            return self.packages.contains_key(real_name);
        }

        false
    }

    /// Get the path for a workspace package.
    ///
    /// Resolves tsconfig path aliases to their actual package paths.
    #[inline]
    pub fn get_package_path(&self, specifier: &str) -> Option<&PathBuf> {
        let package_name = Self::extract_package_name(specifier);

        // Direct package match
        if let Some(path) = self.packages.get(package_name) {
            return Some(path);
        }

        // Check if it's an alias
        if let Some(real_name) = self.aliases.get(package_name) {
            return self.packages.get(real_name);
        }

        None
    }

    /// Resolve an import specifier to its canonical package name.
    ///
    /// If the specifier uses a tsconfig alias, returns the real package name.
    /// Otherwise returns the extracted package name as-is.
    #[inline]
    pub fn resolve_package_name<'specifier>(
        &'specifier self,
        specifier: &'specifier str,
    ) -> &'specifier str {
        let package_name = Self::extract_package_name(specifier);

        // Check if it's an alias
        if let Some(real_name) = self.aliases.get(package_name) {
            return real_name.as_str();
        }

        package_name
    }

    /// Extract the package name from an import specifier.
    ///
    /// Handles scoped packages: `@org/pkg/subpath` -> `@org/pkg`
    /// And regular packages: `pkg/subpath` -> `pkg`
    #[inline]
    pub fn extract_package_name(specifier: &str) -> &str {
        if specifier.starts_with('@') {
            // Scoped package: @org/pkg or @org/pkg/subpath
            Self::extract_scoped_package_name(specifier)
        } else {
            // Regular package: pkg or pkg/subpath
            specifier.split('/').next().unwrap_or(specifier)
        }
    }

    /// Extract scoped package name (@org/pkg).
    fn extract_scoped_package_name(specifier: &str) -> &str {
        let mut parts = specifier.splitn(3, '/');
        let scope = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");

        // Find where @scope/name ends
        let scope_len = scope.len();
        let name_len = name.len();

        if name.is_empty() {
            specifier
        } else {
            // Return @scope/name (scope + '/' + name)
            let end = scope_len + 1 + name_len;
            specifier.get(..end).unwrap_or(specifier)
        }
    }

    /// Extract the subpath from an import specifier, if any.
    ///
    /// `@org/pkg/foo/bar` -> `Some("foo/bar")`
    /// `@org/pkg` -> `None`
    #[inline]
    pub fn extract_subpath(specifier: &str) -> Option<&str> {
        let pkg_name = Self::extract_package_name(specifier);
        let rest = specifier.get(pkg_name.len()..)?;

        if rest.is_empty() {
            None
        } else {
            // Skip leading '/'
            rest.strip_prefix('/')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::tempdir;

    #[test]
    fn test_extract_package_name_simple() {
        assert_eq!(
            Workspace::extract_package_name("lodash"),
            "lodash",
            "simple package name"
        );
        assert_eq!(
            Workspace::extract_package_name("lodash/fp"),
            "lodash",
            "package with subpath"
        );
    }

    #[test]
    fn test_extract_package_name_scoped() {
        assert_eq!(
            Workspace::extract_package_name("@org/pkg"),
            "@org/pkg",
            "scoped package"
        );
        assert_eq!(
            Workspace::extract_package_name("@org/pkg/utils"),
            "@org/pkg",
            "scoped package with subpath"
        );
        assert_eq!(
            Workspace::extract_package_name("@org/pkg/deep/path"),
            "@org/pkg",
            "scoped package with deep subpath"
        );
    }

    #[test]
    fn test_extract_subpath() {
        assert_eq!(
            Workspace::extract_subpath("lodash"),
            None,
            "no subpath for simple package"
        );
        assert_eq!(
            Workspace::extract_subpath("lodash/fp"),
            Some("fp"),
            "subpath for simple package"
        );
        assert_eq!(
            Workspace::extract_subpath("@org/pkg"),
            None,
            "no subpath for scoped package"
        );
        assert_eq!(
            Workspace::extract_subpath("@org/pkg/utils"),
            Some("utils"),
            "subpath for scoped package"
        );
        assert_eq!(
            Workspace::extract_subpath("@org/pkg/deep/path"),
            Some("deep/path"),
            "deep subpath for scoped package"
        );
    }

    #[test]
    fn test_strip_json_comments() {
        let input = r#"{
            // single line comment
            "key": "value", /* inline comment */
            "other": true
        }"#;
        let stripped = Workspace::strip_json_comments(input);
        assert!(
            !stripped.contains("//"),
            "should strip single-line comments"
        );
        assert!(!stripped.contains("/*"), "should strip multi-line comments");
        assert!(stripped.contains("\"key\""), "should preserve strings");
    }

    #[test]
    fn test_is_workspace_package() {
        let mut packages = HashMap::new();
        let _ = packages.insert("@myorg/utils".to_owned(), PathBuf::from("/pkg/utils"));
        let _ = packages.insert("shared".to_owned(), PathBuf::from("/pkg/shared"));

        let workspace = Workspace {
            root: PathBuf::from("/"),
            format: WorkspaceFormat::Npm,
            packages,
            aliases: HashMap::new(),
        };

        assert!(
            workspace.is_workspace_package("@myorg/utils"),
            "should detect workspace package"
        );
        assert!(
            workspace.is_workspace_package("@myorg/utils/helpers"),
            "should detect workspace package with subpath"
        );
        assert!(
            workspace.is_workspace_package("shared"),
            "should detect simple workspace package"
        );
        assert!(
            !workspace.is_workspace_package("react"),
            "should not match external package"
        );
    }

    // =========================================================================
    // Additional workspace tests for comprehensive coverage
    // =========================================================================

    #[test]
    fn test_extract_package_name_edge_cases() {
        // Empty string
        assert_eq!(
            Workspace::extract_package_name(""),
            "",
            "empty string returns empty"
        );

        // Just an @ symbol
        assert_eq!(
            Workspace::extract_package_name("@"),
            "@",
            "just @ returns @"
        );

        // Scope without package name
        assert_eq!(
            Workspace::extract_package_name("@org/"),
            "@org/",
            "scope without name returns full string"
        );

        // Multiple slashes in simple package
        assert_eq!(
            Workspace::extract_package_name("pkg/a/b/c"),
            "pkg",
            "multiple slashes extracts first part"
        );

        // Package with special characters (valid npm names)
        assert_eq!(
            Workspace::extract_package_name("my-pkg-123"),
            "my-pkg-123",
            "package with hyphens and numbers"
        );

        // Scoped package with special characters
        assert_eq!(
            Workspace::extract_package_name("@my-org/my-pkg"),
            "@my-org/my-pkg",
            "scoped package with hyphens"
        );
    }

    #[test]
    fn test_extract_subpath_edge_cases() {
        // Empty string
        assert_eq!(
            Workspace::extract_subpath(""),
            None,
            "empty string has no subpath"
        );

        // Trailing slash (no actual subpath content)
        assert_eq!(
            Workspace::extract_subpath("pkg/"),
            Some(""),
            "trailing slash gives empty subpath"
        );

        // Complex nested subpath
        assert_eq!(
            Workspace::extract_subpath("lodash/fp/core/utils"),
            Some("fp/core/utils"),
            "deeply nested subpath"
        );

        // Scoped package with complex subpath
        assert_eq!(
            Workspace::extract_subpath("@scope/name/dist/esm/index"),
            Some("dist/esm/index"),
            "scoped package with dist path"
        );
    }

    #[test]
    fn test_get_package_path() {
        let mut packages = HashMap::new();
        let _ = packages.insert("@myorg/utils".to_owned(), PathBuf::from("/pkg/utils"));
        let _ = packages.insert("shared".to_owned(), PathBuf::from("/pkg/shared"));

        let workspace = Workspace {
            root: PathBuf::from("/"),
            format: WorkspaceFormat::Npm,
            packages,
            aliases: HashMap::new(),
        };

        assert_eq!(
            workspace.get_package_path("@myorg/utils"),
            Some(&PathBuf::from("/pkg/utils")),
            "should get path for workspace package"
        );

        assert_eq!(
            workspace.get_package_path("@myorg/utils/helpers"),
            Some(&PathBuf::from("/pkg/utils")),
            "should get path for workspace package with subpath"
        );

        assert_eq!(
            workspace.get_package_path("nonexistent"),
            None,
            "should return None for nonexistent package"
        );
    }

    #[test]
    fn test_workspace_format_equality() {
        assert_eq!(WorkspaceFormat::Npm, WorkspaceFormat::Npm, "Npm equals Npm");
        assert_eq!(
            WorkspaceFormat::Pnpm,
            WorkspaceFormat::Pnpm,
            "Pnpm equals Pnpm"
        );
        assert_eq!(
            WorkspaceFormat::TypeScript,
            WorkspaceFormat::TypeScript,
            "TypeScript equals TypeScript"
        );
        assert_ne!(
            WorkspaceFormat::Npm,
            WorkspaceFormat::Pnpm,
            "Npm not equals Pnpm"
        );
    }

    #[test]
    fn test_strip_json_comments_url_in_string() {
        let input_url = r#"{"url": "http://example.com/path"}"#;
        let stripped_url = Workspace::strip_json_comments(input_url);
        assert!(
            stripped_url.contains("http://example.com/path"),
            "should preserve URLs in strings"
        );
    }

    #[test]
    fn test_strip_json_comments_escaped_quotes() {
        let input_escaped = r#"{"text": "say \"hello\""}"#;
        let stripped_escaped = Workspace::strip_json_comments(input_escaped);
        assert!(
            stripped_escaped.contains(r#"\"hello\""#),
            "should preserve escaped quotes"
        );
    }

    #[test]
    fn test_strip_json_comments_multiple_types() {
        let input_multi = r#"{
            // line comment 1
            "a": 1, // trailing comment
            /* block
               comment */
            "b": 2
        }"#;
        let stripped_multi = Workspace::strip_json_comments(input_multi);
        assert!(
            !stripped_multi.contains("line comment"),
            "should strip line comment"
        );
        assert!(
            !stripped_multi.contains("trailing comment"),
            "should strip trailing comment"
        );
        assert!(
            !stripped_multi.contains("block"),
            "should strip multi-line comment"
        );
        assert!(
            stripped_multi.contains("\"a\": 1"),
            "should preserve JSON content"
        );
        assert!(
            stripped_multi.contains("\"b\": 2"),
            "should preserve JSON content"
        );
    }

    #[test]
    fn test_strip_json_comments_preserves_in_string() {
        let input_str = r#"{"comment": "// not a comment /* also not */"}"#;
        let stripped_str = Workspace::strip_json_comments(input_str);
        assert!(
            stripped_str.contains("// not a comment"),
            "should preserve // inside string"
        );
        assert!(
            stripped_str.contains("/* also not */"),
            "should preserve /* */ inside string"
        );
    }

    #[test]
    fn test_strip_json_comments_unterminated() {
        let input_unterm = r#"{"key": "value"} /* unterminated"#;
        let stripped_unterm = Workspace::strip_json_comments(input_unterm);
        assert!(
            stripped_unterm.contains("\"key\": \"value\""),
            "should preserve content before unterminated comment"
        );
    }

    #[test]
    fn test_workspace_clone() {
        let mut packages = HashMap::new();
        let _ = packages.insert("@test/pkg".to_owned(), PathBuf::from("/pkg"));

        let workspace = Workspace {
            root: PathBuf::from("/root"),
            format: WorkspaceFormat::Pnpm,
            packages,
            aliases: HashMap::new(),
        };

        let cloned = workspace.clone();
        assert_eq!(cloned.root, workspace.root, "cloned root should match");
        assert_eq!(
            cloned.format, workspace.format,
            "cloned format should match"
        );
        assert_eq!(
            cloned.packages, workspace.packages,
            "cloned packages should match"
        );
    }

    #[test]
    fn test_workspace_multiple_packages() {
        let mut packages = HashMap::new();
        let _ = packages.insert("@scope/pkg-a".to_owned(), PathBuf::from("/packages/a"));
        let _ = packages.insert("@scope/pkg-b".to_owned(), PathBuf::from("/packages/b"));
        let _ = packages.insert("@scope/pkg-c".to_owned(), PathBuf::from("/packages/c"));
        let _ = packages.insert("unscoped".to_owned(), PathBuf::from("/packages/unscoped"));

        let workspace = Workspace {
            root: PathBuf::from("/"),
            format: WorkspaceFormat::Npm,
            packages,
            aliases: HashMap::new(),
        };

        // All should be workspace packages
        assert!(
            workspace.is_workspace_package("@scope/pkg-a"),
            "pkg-a is workspace package"
        );
        assert!(
            workspace.is_workspace_package("@scope/pkg-b"),
            "pkg-b is workspace package"
        );
        assert!(
            workspace.is_workspace_package("@scope/pkg-c"),
            "pkg-c is workspace package"
        );
        assert!(
            workspace.is_workspace_package("unscoped"),
            "unscoped is workspace package"
        );

        // With subpaths
        assert!(
            workspace.is_workspace_package("@scope/pkg-a/src/utils"),
            "pkg-a with subpath"
        );
        assert!(
            workspace.is_workspace_package("unscoped/lib"),
            "unscoped with subpath"
        );

        // Non-existent packages
        assert!(
            !workspace.is_workspace_package("@scope/pkg-d"),
            "pkg-d is not workspace package"
        );
        assert!(
            !workspace.is_workspace_package("@other/pkg"),
            "different scope is not workspace package"
        );
    }

    #[test]
    fn test_workspace_empty_packages() {
        let workspace = Workspace {
            root: PathBuf::from("/"),
            format: WorkspaceFormat::Npm,
            packages: HashMap::new(),
            aliases: HashMap::new(),
        };

        assert!(
            !workspace.is_workspace_package("any-package"),
            "empty workspace has no packages"
        );
        assert!(
            workspace.get_package_path("any-package").is_none(),
            "empty workspace returns no path"
        );
    }

    // =========================================================================
    // pnpm workspace tests
    // =========================================================================

    #[test]
    fn test_pnpm_workspace_discovery() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pnpm-workspace");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        assert_eq!(
            workspace.format,
            WorkspaceFormat::Pnpm,
            "should detect pnpm format"
        );
        assert_eq!(workspace.packages.len(), 2, "should find 2 packages");
    }

    #[test]
    fn test_pnpm_workspace_package_names() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pnpm-workspace");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        assert!(
            workspace.packages.contains_key("@pnpm-test/a"),
            "should find @pnpm-test/a"
        );
        assert!(
            workspace.packages.contains_key("@pnpm-test/b"),
            "should find @pnpm-test/b"
        );
    }

    #[test]
    fn test_pnpm_workspace_complex_discovery() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pnpm-workspace-complex");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        assert_eq!(
            workspace.format,
            WorkspaceFormat::Pnpm,
            "should detect pnpm format"
        );

        // Should find core and utils, but NOT ignored (due to negative glob !packages/ignored)
        assert!(
            workspace.packages.contains_key("@pnpm-complex/core"),
            "should find @pnpm-complex/core"
        );
        assert!(
            workspace.packages.contains_key("@pnpm-complex/utils"),
            "should find @pnpm-complex/utils"
        );
    }

    #[test]
    fn test_pnpm_workspace_root_has_correct_path() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pnpm-workspace");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        assert!(
            workspace.root.ends_with("pnpm-workspace"),
            "root should be the fixture directory"
        );
    }

    // =========================================================================
    // npm workspace tests
    // =========================================================================

    #[test]
    fn test_npm_workspace_discovery() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inter-package-simple");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        assert_eq!(
            workspace.format,
            WorkspaceFormat::Npm,
            "should detect npm format"
        );
    }

    #[test]
    fn test_npm_workspace_package_paths() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inter-package-simple");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        // Verify paths are resolved correctly
        let pkg_a_path = workspace
            .packages
            .get("@simple/a")
            .expect("should find @simple/a");
        assert!(
            pkg_a_path.ends_with("packages/pkg-a"),
            "pkg-a path should end with packages/pkg-a"
        );

        let pkg_b_path = workspace
            .packages
            .get("@simple/b")
            .expect("should find @simple/b");
        assert!(
            pkg_b_path.ends_with("packages/pkg-b"),
            "pkg-b path should end with packages/pkg-b"
        );
    }

    // =========================================================================
    // Workspace error handling tests
    // =========================================================================

    #[test]
    fn test_workspace_error_display() {
        use crate::WorkspaceError;

        let read_err = WorkspaceError::Read {
            path: PathBuf::from("/test/path"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "test error"),
        };
        let read_msg = format!("{read_err}");
        assert!(
            read_msg.contains("/test/path"),
            "Read error should contain path"
        );
        assert!(
            read_msg.contains("test error"),
            "Read error should contain source message"
        );

        let parse_err = WorkspaceError::Parse {
            path: PathBuf::from("/test/config.json"),
            message: "invalid JSON".to_owned(),
        };
        let parse_msg = format!("{parse_err}");
        assert!(
            parse_msg.contains("/test/config.json"),
            "Parse error should contain path"
        );
        assert!(
            parse_msg.contains("invalid JSON"),
            "Parse error should contain message"
        );

        let glob_err = WorkspaceError::InvalidPattern {
            pattern: "**[invalid".to_owned(),
            source: glob::Pattern::new("**[invalid").unwrap_err(),
        };
        let glob_msg = format!("{glob_err}");
        assert!(
            glob_msg.contains("**[invalid"),
            "Glob error should contain pattern"
        );
    }

    // =========================================================================
    // Non-workspace directory tests
    // =========================================================================

    #[test]
    fn test_non_workspace_directory() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple-cycle");

        let workspace = Workspace::discover(&fixture_path).expect("discovery should not error");

        assert!(
            workspace.is_none(),
            "simple-cycle fixture is not a workspace"
        );
    }

    // =========================================================================
    // tsconfig path alias tests
    // =========================================================================

    #[test]
    fn test_tsconfig_aliases_discovery() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsconfig-aliases");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        assert_eq!(
            workspace.format,
            WorkspaceFormat::Pnpm,
            "should detect pnpm format"
        );
        assert_eq!(workspace.packages.len(), 2, "should find 2 packages");
    }

    #[test]
    fn test_tsconfig_aliases_loaded() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsconfig-aliases");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        // Check that aliases are loaded (wildcard aliases are skipped)
        assert_eq!(
            workspace.aliases.len(),
            2,
            "should have 2 non-wildcard aliases"
        );

        // Verify specific aliases
        assert_eq!(
            workspace.aliases.get("@alias-test/package-a"),
            Some(&"@alias-test/pkg-a".to_owned()),
            "package-a alias should map to pkg-a"
        );
        assert_eq!(
            workspace.aliases.get("@alias-test/package-b"),
            Some(&"@alias-test/pkg-b".to_owned()),
            "package-b alias should map to pkg-b"
        );
    }

    #[test]
    fn test_tsconfig_aliases_is_workspace_package() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsconfig-aliases");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        // Direct package names work
        assert!(
            workspace.is_workspace_package("@alias-test/pkg-a"),
            "direct package name should work"
        );
        assert!(
            workspace.is_workspace_package("@alias-test/pkg-b"),
            "direct package name should work"
        );

        // Aliases also work
        assert!(
            workspace.is_workspace_package("@alias-test/package-a"),
            "alias should be recognized as workspace package"
        );
        assert!(
            workspace.is_workspace_package("@alias-test/package-b"),
            "alias should be recognized as workspace package"
        );

        // With subpaths
        assert!(
            workspace.is_workspace_package("@alias-test/package-a/utils"),
            "alias with subpath should be recognized"
        );
    }

    #[test]
    fn test_tsconfig_aliases_resolve_package_name() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsconfig-aliases");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        // Direct names resolve to themselves
        assert_eq!(
            workspace.resolve_package_name("@alias-test/pkg-a"),
            "@alias-test/pkg-a",
            "direct name resolves to itself"
        );

        // Aliases resolve to real package names
        assert_eq!(
            workspace.resolve_package_name("@alias-test/package-a"),
            "@alias-test/pkg-a",
            "alias resolves to real package name"
        );

        // With subpaths
        assert_eq!(
            workspace.resolve_package_name("@alias-test/package-a/utils"),
            "@alias-test/pkg-a",
            "alias with subpath resolves to real package name"
        );
    }

    #[test]
    fn test_tsconfig_aliases_get_package_path() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsconfig-aliases");

        let workspace = Workspace::discover(&fixture_path)
            .expect("workspace discovery should succeed")
            .expect("fixture should be a workspace");

        // Direct package name
        let direct_path = workspace.get_package_path("@alias-test/pkg-a");
        assert!(
            direct_path.is_some(),
            "direct package name should have path"
        );
        assert!(
            direct_path
                .expect("checked above")
                .ends_with("packages/pkg-a"),
            "path should end with packages/pkg-a"
        );

        // Alias should resolve to same path
        let alias_path = workspace.get_package_path("@alias-test/package-a");
        assert!(alias_path.is_some(), "alias should resolve to path");
        assert_eq!(
            direct_path, alias_path,
            "alias should resolve to same path as direct name"
        );
    }

    #[test]
    fn test_recursive_workspace_patterns_skip_excluded_dirs() {
        let temp = tempdir().expect("tempdir should be created");
        let root = temp.path();

        let real_pkg = root.join("apps/real-app");
        let ignored_pkg = root.join("apps/real-app/node_modules/fake-pkg");
        let dist_pkg = root.join("apps/real-app/dist/generated-pkg");

        fs::create_dir_all(&real_pkg).expect("real package dir should be created");
        fs::create_dir_all(&ignored_pkg).expect("node_modules package dir should be created");
        fs::create_dir_all(&dist_pkg).expect("dist package dir should be created");

        fs::write(
            real_pkg.join("package.json"),
            r#"{ "name": "@demo/real-app" }"#,
        )
        .expect("real package manifest should be written");
        fs::write(
            ignored_pkg.join("package.json"),
            r#"{ "name": "@demo/fake-pkg" }"#,
        )
        .expect("node_modules package manifest should be written");
        fs::write(
            dist_pkg.join("package.json"),
            r#"{ "name": "@demo/generated-pkg" }"#,
        )
        .expect("dist package manifest should be written");

        let packages = Workspace::expand_patterns(root, &[String::from("apps/**/*")])
            .expect("workspace patterns should expand");

        assert!(
            packages.contains_key("@demo/real-app"),
            "should include the real recursive workspace package"
        );
        assert!(
            !packages.contains_key("@demo/fake-pkg"),
            "should skip node_modules packages"
        );
        assert!(
            !packages.contains_key("@demo/generated-pkg"),
            "should skip dist packages"
        );
    }
}
