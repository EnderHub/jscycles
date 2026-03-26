//! TypeScript configuration parsing for path alias resolution.
//!
//! Handles loading `tsconfig.json` and resolving path aliases like `@/utils`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use crate::TsConfigError;

/// Internal struct for parsing tsconfig.json.
#[derive(Debug, Deserialize)]
struct TsConfigJson {
    /// Path to parent config to extend.
    extends: Option<String>,

    /// Compiler options section.
    #[serde(rename = "compilerOptions")]
    compiler_options: Option<CompilerOptions>,
}

/// Internal struct for compiler options.
#[derive(Debug, Default, Deserialize, Clone)]
struct CompilerOptions {
    /// Base URL for module resolution.
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,

    /// Path alias mappings.
    paths: Option<HashMap<String, Vec<String>>>,
}

impl CompilerOptions {
    /// Merge with parent options. Self (child) takes precedence.
    fn merge_with_parent(self, parent_opts: Self) -> Self {
        Self {
            base_url: self.base_url.or(parent_opts.base_url),
            paths: match (self.paths, parent_opts.paths) {
                (Some(child_paths), Some(mut inherited)) => {
                    // Child paths override parent paths
                    inherited.extend(child_paths);
                    Some(inherited)
                },
                (Some(child_paths), None) => Some(child_paths),
                (None, Some(inherited)) => Some(inherited),
                (None, None) => None,
            },
        }
    }
}

/// A compiled path alias pattern.
#[derive(Debug, Clone)]
pub struct PathAlias {
    /// The pattern prefix (e.g., "@/" from "@/*").
    pub prefix: String,

    /// Whether this pattern has a wildcard.
    pub has_wildcard: bool,

    /// Replacement paths (resolved relative to base_url).
    pub replacements: Vec<PathBuf>,
}

/// Parsed TypeScript configuration for path resolution.
#[derive(Debug, Clone)]
pub struct TsConfig {
    /// Base URL for path resolution.
    pub base_url: PathBuf,

    /// Compiled path alias patterns.
    pub paths: Vec<PathAlias>,
}

/// Shared cache for tsconfig discovery and loading.
#[derive(Debug, Clone, Default)]
pub struct TsConfigCache {
    /// Memoized mapping from searched directories to discovered tsconfig paths.
    dir_cache: Arc<Mutex<HashMap<PathBuf, Option<PathBuf>>>>,
    /// Memoized parsed tsconfig contents keyed by file path.
    config_cache: Arc<Mutex<HashMap<PathBuf, TsConfig>>>,
}

/// Maximum depth for tsconfig extends chain to prevent infinite loops.
const MAX_EXTENDS_DEPTH: usize = 10;

impl TsConfig {
    /// Load and parse a tsconfig.json file.
    ///
    /// Handles `extends` field by recursively loading parent configs and merging.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    #[inline]
    pub fn load(path: &Path) -> Result<Self, TsConfigError> {
        let tsconfig_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        // Load and merge all compiler options from extends chain
        let compiler_options = Self::load_merged_options(path, 0)?;

        let base_url = compiler_options
            .base_url
            .map(|b| tsconfig_dir.join(b))
            .unwrap_or_else(|| tsconfig_dir.clone());

        let paths = compiler_options
            .paths
            .map(|mappings| Self::compile_path_mappings(mappings, &base_url))
            .unwrap_or_default();

        Ok(Self { base_url, paths })
    }

    /// Load and merge compiler options from tsconfig and its extends chain.
    fn load_merged_options(path: &Path, depth: usize) -> Result<CompilerOptions, TsConfigError> {
        if depth >= MAX_EXTENDS_DEPTH {
            return Err(TsConfigError::ExtendsDepth);
        }

        let contents = fs::read_to_string(path).map_err(TsConfigError::Read)?;
        let json: TsConfigJson = serde_json::from_str(&contents).map_err(TsConfigError::Parse)?;

        let tsconfig_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        // Load parent options if extends is specified
        let parent_options =
            Self::load_parent_options(json.extends.as_ref(), &tsconfig_dir, depth)?;

        // Merge child options with parent (child takes precedence)
        let child_options = json.compiler_options.unwrap_or_default();
        Ok(child_options.merge_with_parent(parent_options))
    }

    /// Load parent tsconfig options if extends is specified.
    fn load_parent_options(
        extends: Option<&String>,
        tsconfig_dir: &Path,
        depth: usize,
    ) -> Result<CompilerOptions, TsConfigError> {
        let Some(extends_path) = extends else {
            return Ok(CompilerOptions::default());
        };

        let parent_path = Self::resolve_extends_path(extends_path, tsconfig_dir)?;
        Self::load_merged_options(&parent_path, depth + 1)
    }

    /// Resolve the extends path to an absolute path.
    fn resolve_extends_path(extends: &str, tsconfig_dir: &Path) -> Result<PathBuf, TsConfigError> {
        // Handle relative paths
        if extends.starts_with('.') {
            let resolved = tsconfig_dir.join(extends);
            return Self::resolve_tsconfig_file(&resolved);
        }

        // Handle node_modules packages (e.g., "@tsconfig/node18/tsconfig.json")
        let node_modules_path = tsconfig_dir.join("node_modules").join(extends);
        if let Ok(resolved) = Self::resolve_tsconfig_file(&node_modules_path) {
            return Ok(resolved);
        }

        // Try parent node_modules directories
        let mut current = tsconfig_dir.parent();
        while let Some(dir) = current {
            let candidate = dir.join("node_modules").join(extends);
            if let Ok(resolved) = Self::resolve_tsconfig_file(&candidate) {
                return Ok(resolved);
            }
            current = dir.parent();
        }

        Err(TsConfigError::ExtendsNotFound(extends.to_owned()))
    }

    /// Resolve a path to an actual tsconfig file (handles missing .json extension).
    fn resolve_tsconfig_file(path: &Path) -> Result<PathBuf, TsConfigError> {
        // Try exact path first
        if path.exists() && path.is_file() {
            return Ok(path.to_path_buf());
        }

        // Try with .json extension
        let with_json = path.with_extension("json");
        if with_json.exists() && with_json.is_file() {
            return Ok(with_json);
        }

        // Try as directory with tsconfig.json inside
        let as_dir = path.join("tsconfig.json");
        if as_dir.exists() && as_dir.is_file() {
            return Ok(as_dir);
        }

        Err(TsConfigError::ExtendsNotFound(
            path.to_string_lossy().into_owned(),
        ))
    }

    /// Compile path mappings from tsconfig into PathAlias structs.
    fn compile_path_mappings(
        mappings: HashMap<String, Vec<String>>,
        base_url: &Path,
    ) -> Vec<PathAlias> {
        // Sort keys for deterministic iteration
        let mut entries: Vec<_> = mappings.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        entries
            .into_iter()
            .map(|(pattern, replacements)| Self::compile_alias(pattern, replacements, base_url))
            .collect()
    }

    /// Compile a single alias pattern and its replacements.
    fn compile_alias(pattern: String, replacements: Vec<String>, base_url: &Path) -> PathAlias {
        let (prefix, has_wildcard) = Self::parse_pattern(&pattern);
        let resolved_replacements = Self::resolve_replacements(replacements, base_url);

        PathAlias {
            prefix,
            has_wildcard,
            replacements: resolved_replacements,
        }
    }

    /// Parse a pattern into prefix and wildcard flag.
    fn parse_pattern(pattern: &str) -> (String, bool) {
        if pattern.ends_with('*') {
            (
                pattern.strip_suffix('*').unwrap_or(pattern).to_owned(),
                true,
            )
        } else {
            (pattern.to_owned(), false)
        }
    }

    /// Resolve replacement paths relative to base_url.
    fn resolve_replacements(replacements: Vec<String>, base_url: &Path) -> Vec<PathBuf> {
        replacements
            .into_iter()
            .map(|r| {
                let stripped = r.strip_suffix('*').unwrap_or(&r);
                base_url.join(stripped)
            })
            .collect()
    }

    /// Discover tsconfig.json by searching from the given directory upward.
    ///
    /// Returns `None` if no tsconfig.json is found.
    #[inline]
    pub fn discover(start: &Path) -> Option<Self> {
        let mut current = if start.is_file() {
            start.parent().map(Path::to_path_buf)
        } else {
            Some(start.to_path_buf())
        };

        while let Some(dir) = current {
            let tsconfig_path = dir.join("tsconfig.json");
            if tsconfig_path.exists() {
                return Self::load(&tsconfig_path).ok();
            }
            current = dir.parent().map(Path::to_path_buf);
        }

        None
    }

    /// Resolve a path alias to an actual file path.
    ///
    /// Returns `None` if the specifier doesn't match any configured alias.
    #[inline]
    pub fn resolve_alias(&self, specifier: &str) -> Option<PathBuf> {
        for alias in &self.paths {
            if let Some(resolved) = Self::try_resolve_with_alias(specifier, alias) {
                return Some(resolved);
            }
        }
        None
    }

    /// Try to resolve a specifier with a specific alias.
    fn try_resolve_with_alias(specifier: &str, alias: &PathAlias) -> Option<PathBuf> {
        if alias.has_wildcard {
            return Self::resolve_wildcard_alias(specifier, alias);
        }
        if specifier == alias.prefix {
            return alias.replacements.first().cloned();
        }
        None
    }

    /// Resolve a wildcard alias.
    fn resolve_wildcard_alias(specifier: &str, alias: &PathAlias) -> Option<PathBuf> {
        let suffix = specifier.strip_prefix(&alias.prefix)?;
        let replacement = alias.replacements.first()?;
        let resolved = replacement.join(suffix);

        // Try with various extensions
        if let Some(with_ext) = Self::try_resolve_with_extensions(&resolved) {
            return Some(with_ext);
        }

        // Try as directory with index file
        if let Some(index) = Self::try_resolve_as_directory(&resolved) {
            return Some(index);
        }

        // Return the path even if it doesn't exist (caller handles validation)
        Some(resolved)
    }

    /// Try to resolve a path with various extensions.
    fn try_resolve_with_extensions(base: &Path) -> Option<PathBuf> {
        for ext in &["", ".ts", ".tsx", ".js", ".jsx"] {
            let candidate = if ext.is_empty() {
                base.to_path_buf()
            } else {
                base.with_extension(ext.trim_start_matches('.'))
            };
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    /// Try to resolve a path as a directory with an index file.
    fn try_resolve_as_directory(path: &Path) -> Option<PathBuf> {
        if !path.is_dir() {
            return None;
        }
        for index_name in &["index.ts", "index.tsx", "index.js", "index.jsx"] {
            let index_path = path.join(index_name);
            if index_path.exists() {
                return Some(index_path);
            }
        }
        None
    }
}

impl TsConfigCache {
    /// Create a new empty cache.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover a tsconfig for a path, caching both lookup results and parsed configs.
    #[inline]
    pub fn discover(&self, start: &Path) -> Option<TsConfig> {
        let mut current = if start.is_file() {
            start.parent().map(Path::to_path_buf)
        } else {
            Some(start.to_path_buf())
        };
        let mut visited = Vec::new();

        while let Some(dir) = current {
            if let Some(cached) = self.cached_dir_result(&dir) {
                self.cache_dir_results(&visited, cached.as_deref());
                return cached.and_then(|path| self.load_cached(&path));
            }

            let tsconfig_path = dir.join("tsconfig.json");
            if tsconfig_path.exists() {
                visited.push(dir);
                self.cache_dir_results(&visited, Some(&tsconfig_path));
                return self.load_cached(&tsconfig_path);
            }

            visited.push(dir.clone());
            current = dir.parent().map(Path::to_path_buf);
        }

        self.cache_dir_results(&visited, None);
        None
    }

    /// Load a specific tsconfig path using the parsed-config cache.
    #[inline]
    pub fn load(&self, path: &Path) -> Option<TsConfig> {
        self.load_cached(path)
    }

    /// Get a cached directory lookup result, if any.
    fn cached_dir_result(&self, dir: &Path) -> Option<Option<PathBuf>> {
        self.dir_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(dir)
            .cloned()
    }

    /// Cache discovery results for a list of visited directories.
    fn cache_dir_results(&self, dirs: &[PathBuf], tsconfig_path: Option<&Path>) {
        let value = tsconfig_path.map(Path::to_path_buf);
        let mut cache = self.dir_cache.lock().unwrap_or_else(|err| err.into_inner());
        for dir in dirs {
            let _ = cache.insert(dir.clone(), value.clone());
        }
    }

    /// Load a parsed tsconfig from cache or disk.
    fn load_cached(&self, path: &Path) -> Option<TsConfig> {
        if let Some(config) = self
            .config_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(path)
            .cloned()
        {
            return Some(config);
        }

        let config = TsConfig::load(path).ok()?;
        let mut cache = self
            .config_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _ = cache.insert(path.to_path_buf(), config.clone());
        Some(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::tempdir;

    #[test]
    fn test_parse_tsconfig() {
        let json = r#"{
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/*"],
                    "~/utils": ["src/utils"]
                }
            }
        }"#;

        let parsed: TsConfigJson =
            serde_json::from_str(json).expect("test json should parse successfully");
        assert!(parsed.compiler_options.is_some());

        let opts = parsed
            .compiler_options
            .expect("compiler options should exist in test");
        assert_eq!(opts.base_url, Some(".".to_owned()));
        assert!(opts.paths.is_some());
    }

    #[test]
    fn test_wildcard_detection() {
        let pattern = "@/*";
        let has_wildcard = pattern.ends_with('*');
        assert!(has_wildcard);

        let prefix = pattern.strip_suffix('*').unwrap_or(pattern);
        assert_eq!(prefix, "@/");
    }

    // =========================================================================
    // Additional tsconfig tests for comprehensive coverage
    // =========================================================================

    #[test]
    fn test_parse_pattern_with_wildcard_at() {
        let (prefix, has_wildcard) = TsConfig::parse_pattern("@/*");
        assert_eq!(prefix, "@/", "prefix should be @/");
        assert!(has_wildcard, "should detect wildcard");
    }

    #[test]
    fn test_parse_pattern_with_wildcard_components() {
        let (prefix, has_wildcard) = TsConfig::parse_pattern("@components/*");
        assert_eq!(prefix, "@components/", "prefix should be @components/");
        assert!(has_wildcard, "should detect wildcard");
    }

    #[test]
    fn test_parse_pattern_with_wildcard_tilde() {
        let (prefix, has_wildcard) = TsConfig::parse_pattern("~/*");
        assert_eq!(prefix, "~/", "prefix should be ~/");
        assert!(has_wildcard, "should detect wildcard");
    }

    #[test]
    fn test_parse_pattern_without_wildcard_utils() {
        let (prefix, has_wildcard) = TsConfig::parse_pattern("@utils");
        assert_eq!(prefix, "@utils", "prefix should be @utils");
        assert!(!has_wildcard, "should not detect wildcard");
    }

    #[test]
    fn test_parse_pattern_without_wildcard_path() {
        let (prefix, has_wildcard) = TsConfig::parse_pattern("exact/path");
        assert_eq!(prefix, "exact/path", "prefix should be exact/path");
        assert!(!has_wildcard, "should not detect wildcard");
    }

    #[test]
    fn test_resolve_replacements() {
        let base_url = PathBuf::from("/project");
        let replacements = vec!["src/*".to_owned(), "lib/*".to_owned()];
        let resolved = TsConfig::resolve_replacements(replacements, &base_url);

        assert_eq!(resolved.len(), 2, "should have 2 resolved paths");
        assert_eq!(resolved.first(), Some(&PathBuf::from("/project/src/")));
        assert_eq!(resolved.get(1), Some(&PathBuf::from("/project/lib/")));
    }

    #[test]
    fn test_resolve_replacements_no_wildcard() {
        let base_url = PathBuf::from("/project");
        let replacements = vec!["src/utils".to_owned()];
        let resolved = TsConfig::resolve_replacements(replacements, &base_url);

        assert_eq!(resolved.len(), 1, "should have 1 resolved path");
        assert_eq!(resolved.first(), Some(&PathBuf::from("/project/src/utils")));
    }

    #[test]
    fn test_try_resolve_with_alias_exact_match() {
        let alias = PathAlias {
            prefix: "@utils".to_owned(),
            has_wildcard: false,
            replacements: vec![PathBuf::from("/project/src/utils")],
        };

        let result_exact = TsConfig::try_resolve_with_alias("@utils", &alias);
        assert_eq!(
            result_exact,
            Some(PathBuf::from("/project/src/utils")),
            "exact match should resolve"
        );

        let result_partial = TsConfig::try_resolve_with_alias("@utils/foo", &alias);
        assert_eq!(
            result_partial, None,
            "partial match should not resolve exact alias"
        );
    }

    #[test]
    fn test_compile_alias() {
        let base_url = PathBuf::from("/project");
        let alias = TsConfig::compile_alias("@/*".to_owned(), vec!["src/*".to_owned()], &base_url);

        assert_eq!(alias.prefix, "@/", "prefix should be @/");
        assert!(alias.has_wildcard, "should have wildcard");
        assert_eq!(alias.replacements.len(), 1, "should have 1 replacement");
    }

    #[test]
    fn test_compile_path_mappings_ordering() {
        let mut mappings = HashMap::new();
        let _ = mappings.insert("z/*".to_owned(), vec!["z/*".to_owned()]);
        let _ = mappings.insert("a/*".to_owned(), vec!["a/*".to_owned()]);
        let _ = mappings.insert("m/*".to_owned(), vec!["m/*".to_owned()]);

        let base_url = PathBuf::from("/");
        let aliases = TsConfig::compile_path_mappings(mappings, &base_url);

        // Should be sorted alphabetically
        assert_eq!(aliases.len(), 3, "should have 3 aliases");
        assert_eq!(aliases.first().map(|a| a.prefix.as_str()), Some("a/"));
        assert_eq!(aliases.get(1).map(|a| a.prefix.as_str()), Some("m/"));
        assert_eq!(aliases.get(2).map(|a| a.prefix.as_str()), Some("z/"));
    }

    #[test]
    fn test_tsconfig_clone() {
        let config = TsConfig {
            base_url: PathBuf::from("/project"),
            paths: vec![PathAlias {
                prefix: "@/".to_owned(),
                has_wildcard: true,
                replacements: vec![PathBuf::from("/project/src/")],
            }],
        };

        let cloned = config.clone();
        assert_eq!(cloned.base_url, config.base_url, "base_url should match");
        assert_eq!(cloned.paths.len(), config.paths.len(), "paths should match");
    }

    #[test]
    fn test_path_alias_clone() {
        let alias = PathAlias {
            prefix: "@utils/".to_owned(),
            has_wildcard: true,
            replacements: vec![PathBuf::from("/a/"), PathBuf::from("/b/")],
        };

        let cloned = alias.clone();
        assert_eq!(cloned.prefix, alias.prefix, "prefix should match");
        assert_eq!(
            cloned.has_wildcard, alias.has_wildcard,
            "wildcard should match"
        );
        assert_eq!(
            cloned.replacements.len(),
            alias.replacements.len(),
            "replacements should match"
        );
    }

    #[test]
    fn test_resolve_alias_no_match() {
        let config = TsConfig {
            base_url: PathBuf::from("/project"),
            paths: vec![PathAlias {
                prefix: "@/".to_owned(),
                has_wildcard: true,
                replacements: vec![PathBuf::from("/project/src/")],
            }],
        };

        let result_local = config.resolve_alias("./local");
        assert_eq!(result_local, None, "relative import should not match alias");

        let result_react = config.resolve_alias("react");
        assert_eq!(result_react, None, "external import should not match alias");

        let result_tilde = config.resolve_alias("~/utils");
        assert_eq!(result_tilde, None, "different prefix should not match");
    }

    #[test]
    fn test_resolve_alias_with_multiple_patterns() {
        let config = TsConfig {
            base_url: PathBuf::from("/project"),
            paths: vec![
                PathAlias {
                    prefix: "@/".to_owned(),
                    has_wildcard: true,
                    replacements: vec![PathBuf::from("/project/src/")],
                },
                PathAlias {
                    prefix: "@components/".to_owned(),
                    has_wildcard: true,
                    replacements: vec![PathBuf::from("/project/components/")],
                },
            ],
        };

        let result_utils = config.resolve_alias("@/utils");
        assert!(result_utils.is_some(), "should match @/ alias");
        assert!(
            result_utils
                .as_ref()
                .is_some_and(|p| p.starts_with("/project/src")),
            "should resolve to src"
        );

        let result_component = config.resolve_alias("@components/Button");
        assert!(
            result_component.is_some(),
            "should match @components/ alias"
        );
        assert!(
            result_component
                .as_ref()
                .is_some_and(|p| p.starts_with("/project/components")),
            "should resolve to components"
        );
    }

    #[test]
    fn test_compiler_options_merge() {
        let parent = CompilerOptions {
            base_url: Some("parent_base".to_owned()),
            paths: Some({
                let mut m = HashMap::new();
                let _ = m.insert("@parent/*".to_owned(), vec!["parent/*".to_owned()]);
                m
            }),
        };

        let child = CompilerOptions {
            base_url: Some("child_base".to_owned()),
            paths: Some({
                let mut m = HashMap::new();
                let _ = m.insert("@child/*".to_owned(), vec!["child/*".to_owned()]);
                m
            }),
        };

        let merged = child.merge_with_parent(parent);

        assert_eq!(
            merged.base_url,
            Some("child_base".to_owned()),
            "child base_url takes precedence"
        );
        let paths = merged.paths.expect("merged should have paths");
        assert!(
            paths.contains_key("@parent/*"),
            "should inherit parent paths"
        );
        assert!(paths.contains_key("@child/*"), "should have child paths");
    }

    #[test]
    fn test_compiler_options_merge_child_only() {
        let parent = CompilerOptions::default();

        let child = CompilerOptions {
            base_url: Some("child_base".to_owned()),
            paths: Some({
                let mut m = HashMap::new();
                let _ = m.insert("@/*".to_owned(), vec!["src/*".to_owned()]);
                m
            }),
        };

        let merged = child.merge_with_parent(parent);

        assert_eq!(
            merged.base_url,
            Some("child_base".to_owned()),
            "should have child base_url"
        );
        assert!(merged.paths.is_some(), "should have child paths");
    }

    #[test]
    fn test_compiler_options_merge_parent_only() {
        let parent = CompilerOptions {
            base_url: Some("parent_base".to_owned()),
            paths: Some({
                let mut m = HashMap::new();
                let _ = m.insert("@/*".to_owned(), vec!["lib/*".to_owned()]);
                m
            }),
        };

        let child = CompilerOptions::default();
        let merged = child.merge_with_parent(parent);

        assert_eq!(
            merged.base_url,
            Some("parent_base".to_owned()),
            "should inherit parent base_url"
        );
        assert!(merged.paths.is_some(), "should inherit parent paths");
    }

    #[test]
    fn test_compiler_options_default() {
        let opts = CompilerOptions::default();
        assert!(opts.base_url.is_none(), "default has no base_url");
        assert!(opts.paths.is_none(), "default has no paths");
    }

    #[test]
    fn test_tsconfig_error_extends_depth() {
        let err = TsConfigError::ExtendsDepth;
        let msg = format!("{err}");
        assert!(
            msg.contains("too deep"),
            "error message should mention depth"
        );
    }

    #[test]
    fn test_tsconfig_error_extends_not_found() {
        let err = TsConfigError::ExtendsNotFound("./missing.json".to_owned());
        let msg = format!("{err}");
        assert!(
            msg.contains("missing.json"),
            "error message should contain path"
        );
    }

    // =========================================================================
    // Extends chain tests
    // =========================================================================

    #[test]
    fn test_extends_relative_path_resolution() {
        // Test that relative extends paths work
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsconfig-extends");

        let config_opt = TsConfig::discover(&fixture_path);
        assert!(
            config_opt.is_some(),
            "should discover tsconfig with extends"
        );

        let config = config_opt.expect("config should exist");

        // Should have path aliases from all three configs merged
        // Child config (tsconfig.json): @app/*
        // Parent config (tsconfig.shared.json): @shared/*
        // Grandparent config (tsconfig.base.json): @base/*
        assert!(
            !config.paths.is_empty(),
            "should have paths from extends chain"
        );
    }

    #[test]
    fn test_extends_merges_all_path_aliases() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsconfig-extends");

        let config = TsConfig::discover(&fixture_path).expect("should discover config");

        // Verify all aliases from the chain are present
        let alias_prefixes: Vec<&str> = config.paths.iter().map(|a| a.prefix.as_str()).collect();

        assert!(
            alias_prefixes.contains(&"@app/"),
            "should have @app/* from child config"
        );
        assert!(
            alias_prefixes.contains(&"@shared/"),
            "should have @shared/* from parent config"
        );
        assert!(
            alias_prefixes.contains(&"@base/"),
            "should have @base/* from grandparent config"
        );
    }

    #[test]
    fn test_extends_from_node_modules() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsconfig-extends-pkg");

        let config_opt = TsConfig::discover(&fixture_path);
        assert!(
            config_opt.is_some(),
            "should discover tsconfig extending from node_modules"
        );

        let config = config_opt.expect("config should exist");

        // Should have aliases from both configs
        let alias_prefixes: Vec<&str> = config.paths.iter().map(|a| a.prefix.as_str()).collect();

        assert!(
            alias_prefixes.contains(&"@local/"),
            "should have @local/* from child config"
        );
        assert!(
            alias_prefixes.contains(&"@company/"),
            "should have @company/* from extended config in node_modules"
        );
    }

    #[test]
    fn test_extends_child_overrides_parent_same_alias() {
        // When child and parent define the same alias, child should win
        let child_paths =
            Some(std::iter::once(("@alias/*".to_owned(), vec!["child/*".to_owned()])).collect());
        let parent_paths =
            Some(std::iter::once(("@alias/*".to_owned(), vec!["parent/*".to_owned()])).collect());

        let child_opts = CompilerOptions {
            base_url: Some(".".to_owned()),
            paths: child_paths,
        };
        let parent_opts = CompilerOptions {
            base_url: Some(".".to_owned()),
            paths: parent_paths,
        };

        let merged = child_opts.merge_with_parent(parent_opts);
        let paths = merged.paths.expect("should have paths");

        let alias_target = paths.get("@alias/*").expect("should have @alias/*");
        assert_eq!(
            alias_target.first().map(String::as_str),
            Some("child/*"),
            "child should override parent for same alias"
        );
    }

    #[test]
    fn test_extends_base_url_inherited() {
        // When child doesn't specify baseUrl, should inherit from parent
        let parent_opts = CompilerOptions {
            base_url: Some("src".to_owned()),
            paths: None,
        };
        let child_opts = CompilerOptions {
            base_url: None,
            paths: Some(std::iter::once(("@/*".to_owned(), vec!["*".to_owned()])).collect()),
        };

        let merged = child_opts.merge_with_parent(parent_opts);

        assert_eq!(
            merged.base_url.as_deref(),
            Some("src"),
            "child should inherit baseUrl from parent"
        );
    }

    #[test]
    fn test_extends_base_url_child_overrides() {
        // When child specifies baseUrl, should override parent
        let parent_opts = CompilerOptions {
            base_url: Some("parent-src".to_owned()),
            paths: None,
        };
        let child_opts = CompilerOptions {
            base_url: Some("child-src".to_owned()),
            paths: None,
        };

        let merged = child_opts.merge_with_parent(parent_opts);

        assert_eq!(
            merged.base_url.as_deref(),
            Some("child-src"),
            "child should override parent baseUrl"
        );
    }

    #[test]
    fn test_extends_paths_are_merged_not_replaced() {
        // Different path aliases from parent and child should both be present
        let parent_paths =
            Some(std::iter::once(("@parent/*".to_owned(), vec!["parent/*".to_owned()])).collect());
        let child_paths =
            Some(std::iter::once(("@child/*".to_owned(), vec!["child/*".to_owned()])).collect());

        let parent_opts = CompilerOptions {
            base_url: Some(".".to_owned()),
            paths: parent_paths,
        };
        let child_opts = CompilerOptions {
            base_url: Some(".".to_owned()),
            paths: child_paths,
        };

        let merged = child_opts.merge_with_parent(parent_opts);
        let paths = merged.paths.expect("should have paths");

        assert!(
            paths.contains_key("@parent/*"),
            "should retain parent alias"
        );
        assert!(paths.contains_key("@child/*"), "should retain child alias");
    }

    #[test]
    fn test_tsconfig_cache_discovers_shared_parent_config() {
        let temp = tempdir().expect("tempdir should be created");
        let root = temp.path();
        fs::create_dir_all(root.join("packages/a/src")).expect("package a dirs should exist");
        fs::create_dir_all(root.join("packages/b/src")).expect("package b dirs should exist");
        fs::write(
            root.join("tsconfig.json"),
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
        )
        .expect("tsconfig should be written");

        let cache = TsConfigCache::new();
        let first = cache.discover(&root.join("packages/a"));
        let second = cache.discover(&root.join("packages/b"));

        assert!(first.is_some(), "first lookup should find tsconfig");
        assert!(
            second.is_some(),
            "second lookup should reuse cached discovery"
        );
        assert_eq!(
            first.expect("checked above").base_url,
            second.expect("checked above").base_url,
            "cached lookups should return equivalent configs"
        );
    }
}
