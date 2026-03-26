//! Package discovery for monorepo scanning.
//!
//! Handles finding packages by scanning directories for `package.json` files.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use crate::DiscoveryError;
use crate::config::{Config, PackageConfig};
use crate::workspace::Workspace;

/// A discovered JavaScript/TypeScript package.
#[derive(Debug, Clone)]
pub struct Package {
    /// Package name from package.json "name" field.
    pub name: String,

    /// Directory containing package.json.
    pub path: PathBuf,

    /// Merged configuration for this package.
    pub config: PackageConfig,
}

/// Package.json structure (minimal).
#[derive(Debug, Deserialize)]
struct PackageJson {
    /// Package name.
    name: Option<String>,
}

/// Package discovery engine.
#[derive(Debug)]
pub struct PackageDiscovery {
    /// Patterns for directories to exclude from scanning.
    exclude_patterns: Vec<glob::Pattern>,

    /// Patterns for package names to include (--only).
    only_filters: Vec<glob::Pattern>,

    /// Patterns for package names to exclude (--exclude).
    exclude_filters: Vec<glob::Pattern>,

    /// Package-specific config overrides.
    package_configs: HashMap<String, PackageConfig>,

    /// Default extensions.
    default_extensions: Vec<String>,
}

impl PackageDiscovery {
    /// Create a new package discovery engine.
    ///
    /// # Errors
    ///
    /// Returns an error if any glob pattern is invalid.
    #[inline]
    pub fn new(
        config: &Config,
        only: &[String],
        exclude: &[String],
    ) -> Result<Self, DiscoveryError> {
        let exclude_patterns = config
            .scan
            .exclude
            .iter()
            .map(|p| {
                glob::Pattern::new(p).map_err(|source| DiscoveryError::InvalidPattern {
                    pattern: p.clone(),
                    source,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let only_filters = only
            .iter()
            .map(|p| {
                glob::Pattern::new(p).map_err(|source| DiscoveryError::InvalidPattern {
                    pattern: p.clone(),
                    source,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let exclude_filters = exclude
            .iter()
            .map(|p| {
                glob::Pattern::new(p).map_err(|source| DiscoveryError::InvalidPattern {
                    pattern: p.clone(),
                    source,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            exclude_patterns,
            only_filters,
            exclude_filters,
            package_configs: config.packages.clone(),
            default_extensions: config.defaults.extensions.clone(),
        })
    }

    /// Discover all packages under the given root directory.
    ///
    /// # Errors
    ///
    /// Returns an error if directory reading or package.json parsing fails.
    #[inline]
    pub fn discover(&self, root: &Path) -> Result<Vec<Package>, DiscoveryError> {
        let mut packages = Vec::new();

        for walk_result in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !self.should_exclude_dir(e.path()))
        {
            let dir_entry = walk_result.map_err(|err| DiscoveryError::ReadDir {
                path: root.to_path_buf(),
                source: err.into(),
            })?;

            if let Some(package) = self.try_parse_package_entry(&dir_entry)? {
                packages.push(package);
            }
        }

        Ok(packages)
    }

    /// Try to parse a package from a walkdir entry if it's a package.json.
    fn try_parse_package_entry(
        &self,
        entry: &walkdir::DirEntry,
    ) -> Result<Option<Package>, DiscoveryError> {
        if entry.file_type().is_dir() || entry.file_name() != "package.json" {
            return Ok(None);
        }
        let Some(package) = self.parse_package(entry.path())? else {
            return Ok(None);
        };
        if self.matches_filters(&package.name) {
            return Ok(Some(package));
        }
        Ok(None)
    }

    /// Discover packages from explicit paths (CLI arguments).
    ///
    /// # Errors
    ///
    /// Returns an error if package.json parsing fails.
    #[inline]
    pub fn discover_explicit(&self, paths: &[PathBuf]) -> Result<Vec<Package>, DiscoveryError> {
        let mut packages = Vec::new();

        for path in paths {
            self.discover_from_path(path, &mut packages)?;
        }

        Ok(packages)
    }

    /// Discover packages from an already-expanded workspace package map.
    ///
    /// This avoids recursively scanning the filesystem a second time when
    /// workspace discovery has already found the package roots.
    #[inline]
    pub fn discover_workspace_packages(
        &self,
        workspace: &Workspace,
        scan_paths: &[PathBuf],
    ) -> Vec<Package> {
        let normalized_paths: Vec<_> = scan_paths
            .iter()
            .map(|path| Self::normalize_scan_path(path))
            .collect();

        let mut packages: Vec<_> = workspace
            .packages
            .iter()
            .filter(|(_, path)| Self::matches_scan_paths(path, &normalized_paths))
            .filter(|(name, _)| self.matches_filters(name))
            .map(|(name, path)| self.build_package(name.clone(), path.clone()))
            .collect();

        packages.sort_by(|left, right| left.name.cmp(&right.name));
        packages
    }

    /// Discover packages from a single path.
    fn discover_from_path(
        &self,
        path: &Path,
        packages: &mut Vec<Package>,
    ) -> Result<(), DiscoveryError> {
        // Check for direct package.json
        if let Some(package_json_path) = Self::find_package_json(path) {
            self.add_package_if_matches(&package_json_path, packages)?;
            return Ok(());
        }

        // Search for package.json in subdirectories
        self.scan_directory_for_packages(path, packages)
    }

    /// Find package.json path from a given path.
    fn find_package_json(path: &Path) -> Option<PathBuf> {
        let with_package_json = path.join("package.json");
        if with_package_json.exists() {
            return Some(with_package_json);
        }
        if path.file_name().is_some_and(|n| n == "package.json") {
            return Some(path.to_path_buf());
        }
        None
    }

    /// Scan a directory for packages.
    fn scan_directory_for_packages(
        &self,
        path: &Path,
        packages: &mut Vec<Package>,
    ) -> Result<(), DiscoveryError> {
        for entry in WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !self.should_exclude_dir(e.path()))
            .flatten()
        {
            if let Some(package) = self.try_parse_package_entry(&entry)? {
                packages.push(package);
            }
        }
        Ok(())
    }

    /// Add a package from a package.json path if it matches filters.
    fn add_package_if_matches(
        &self,
        package_json_path: &Path,
        packages: &mut Vec<Package>,
    ) -> Result<(), DiscoveryError> {
        if let Some(package) = self.parse_package(package_json_path)?
            && self.matches_filters(&package.name)
        {
            packages.push(package);
        }
        Ok(())
    }

    /// Normalize a scan path for prefix matching against workspace package roots.
    fn normalize_scan_path(path: &Path) -> PathBuf {
        let base = if path.file_name().is_some_and(|name| name == "package.json") {
            path.parent().unwrap_or(path)
        } else {
            path
        };

        if base.is_absolute() {
            base.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(base))
                .unwrap_or_else(|_| base.to_path_buf())
        }
    }

    /// Check whether a package path falls under any requested scan path.
    fn matches_scan_paths(package_path: &Path, scan_paths: &[PathBuf]) -> bool {
        scan_paths.is_empty()
            || scan_paths
                .iter()
                .any(|scan_path| package_path.starts_with(scan_path))
    }

    /// Check if a directory should be excluded from scanning.
    fn should_exclude_dir(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Always exclude node_modules, dist, build
        if path_str.contains("node_modules")
            || path_str.ends_with("/dist")
            || path_str.ends_with("/build")
            || path_str.ends_with("/.next")
        {
            return true;
        }

        // Check exclude patterns
        for pattern in &self.exclude_patterns {
            if pattern.matches(&path_str) {
                return true;
            }
        }

        false
    }

    /// Parse a package.json file and create a Package struct.
    fn parse_package(&self, package_json_path: &Path) -> Result<Option<Package>, DiscoveryError> {
        let contents =
            fs::read_to_string(package_json_path).map_err(|source| DiscoveryError::ReadDir {
                path: package_json_path.to_path_buf(),
                source,
            })?;

        let package_json: PackageJson =
            serde_json::from_str(&contents).map_err(|source| DiscoveryError::ParsePackageJson {
                path: package_json_path.to_path_buf(),
                source,
            })?;

        let Some(name) = package_json.name else {
            // Skip packages without a name
            return Ok(None);
        };

        let package_dir = package_json_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        Ok(Some(self.build_package(name, package_dir)))
    }

    /// Build a package with config defaults applied.
    fn build_package(&self, name: String, package_dir: PathBuf) -> Package {
        let mut config = self.find_package_config(&name);

        if config.extensions.is_none() {
            config.extensions = Some(self.default_extensions.clone());
        }

        Package {
            name,
            path: package_dir,
            config,
        }
    }

    /// Find the matching package config for a package name.
    fn find_package_config(&self, name: &str) -> PackageConfig {
        let mut keys: Vec<_> = self.package_configs.keys().collect();
        keys.sort();
        for pattern in keys {
            if !Self::pattern_matches_name(pattern, name) {
                continue;
            }
            if let Some(cfg) = self.package_configs.get(pattern) {
                return cfg.clone();
            }
        }
        PackageConfig::default()
    }

    /// Check if a glob pattern matches a package name.
    fn pattern_matches_name(pattern: &str, name: &str) -> bool {
        glob::Pattern::new(pattern).is_ok_and(|p| p.matches(name))
    }

    /// Check if a package name matches the only/exclude filters.
    fn matches_filters(&self, name: &str) -> bool {
        // If --only patterns specified, package must match at least one
        if !self.only_filters.is_empty() {
            let matches_only = self.only_filters.iter().any(|p| p.matches(name));
            if !matches_only {
                return false;
            }
        }

        // If --exclude patterns specified, package must not match any
        for pattern in &self.exclude_filters {
            if pattern.matches(name) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::workspace::WorkspaceFormat;

    #[test]
    fn test_matches_filters_empty() {
        let config = Config::default();
        let discovery = PackageDiscovery::new(&config, &[], &[])
            .expect("test discovery should be created successfully");
        assert!(discovery.matches_filters("@myorg/pkg"));
    }

    #[test]
    fn test_matches_filters_only() {
        let config = Config::default();
        let discovery = PackageDiscovery::new(&config, &["@myorg/*".to_owned()], &[])
            .expect("test discovery should be created successfully");
        assert!(discovery.matches_filters("@myorg/pkg"));
        assert!(!discovery.matches_filters("@other/pkg"));
    }

    #[test]
    fn test_matches_filters_exclude() {
        let config = Config::default();
        let discovery = PackageDiscovery::new(&config, &[], &["*-legacy".to_owned()])
            .expect("test discovery should be created successfully");
        assert!(discovery.matches_filters("@myorg/pkg"));
        assert!(!discovery.matches_filters("@myorg/pkg-legacy"));
    }

    #[test]
    fn test_discover_workspace_packages_filters_by_scan_path() {
        let config = Config::default();
        let discovery = PackageDiscovery::new(&config, &[], &[])
            .expect("test discovery should be created successfully");

        let mut packages = HashMap::new();
        let _ = packages.insert("@demo/app".to_owned(), PathBuf::from("/repo/apps/app"));
        let _ = packages.insert("@demo/lib".to_owned(), PathBuf::from("/repo/libs/lib"));

        let workspace = Workspace {
            root: PathBuf::from("/repo"),
            format: WorkspaceFormat::Pnpm,
            packages,
            aliases: HashMap::new(),
        };

        let packages =
            discovery.discover_workspace_packages(&workspace, &[PathBuf::from("/repo/apps")]);

        assert_eq!(
            packages.len(),
            1,
            "should only include packages under scan path"
        );
        assert_eq!(packages[0].name, "@demo/app");
        assert_eq!(packages[0].path, PathBuf::from("/repo/apps/app"));
    }

    #[test]
    fn test_should_exclude_next_build_output() {
        let config = Config::default();
        let discovery = PackageDiscovery::new(&config, &[], &[])
            .expect("test discovery should be created successfully");

        assert!(
            discovery.should_exclude_dir(Path::new("/repo/apps/auth/.next")),
            ".next should be excluded from package discovery"
        );
    }
}
