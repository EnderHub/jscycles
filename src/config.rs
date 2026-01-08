//! Configuration file parsing for jscycles.
//!
//! Handles loading and parsing `jscycles.yaml` configuration files.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::ConfigError;

/// Default include patterns for package scanning.
fn default_include() -> Vec<String> {
    vec![".".to_owned()]
}

/// Default exclude patterns for package scanning.
fn default_exclude() -> Vec<String> {
    vec![
        "**/node_modules".to_owned(),
        "**/dist".to_owned(),
        "**/build".to_owned(),
    ]
}

/// Default file extensions to analyze.
fn default_extensions() -> Vec<String> {
    vec![
        "ts".to_owned(),
        "tsx".to_owned(),
        "js".to_owned(),
        "jsx".to_owned(),
    ]
}

/// Root configuration for jscycles.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Scan configuration for package discovery.
    #[serde(default)]
    pub scan: ScanConfig,

    /// Default settings for all packages.
    #[serde(default)]
    pub defaults: DefaultsConfig,

    /// Package-specific overrides keyed by package name glob pattern.
    #[serde(default)]
    pub packages: HashMap<String, PackageConfig>,
}

/// Configuration for package scanning/discovery.
#[derive(Debug, Deserialize)]
pub struct ScanConfig {
    /// Glob patterns for directories to include in scanning.
    #[serde(default = "default_include")]
    pub include: Vec<String>,

    /// Glob patterns for directories to exclude from scanning.
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
}

impl Default for ScanConfig {
    #[inline]
    fn default() -> Self {
        Self {
            include: default_include(),
            exclude: default_exclude(),
        }
    }
}

/// Default settings applied to all packages.
#[derive(Debug, Deserialize)]
pub struct DefaultsConfig {
    /// File extensions to analyze.
    #[serde(default = "default_extensions")]
    pub extensions: Vec<String>,
}

impl Default for DefaultsConfig {
    #[inline]
    fn default() -> Self {
        Self {
            extensions: default_extensions(),
        }
    }
}

/// Package-specific configuration overrides.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct PackageConfig {
    /// Override file extensions for this package.
    pub extensions: Option<Vec<String>>,

    /// Glob patterns for files to ignore within this package.
    pub ignore: Option<Vec<String>>,
}

impl Config {
    /// Load configuration from a YAML file.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::Read` if the file cannot be read,
    /// or `ConfigError::Parse` if the YAML is invalid.
    #[inline]
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path).map_err(ConfigError::Read)?;
        serde_yaml::from_str(&contents).map_err(ConfigError::Parse)
    }

    /// Load configuration from a file, returning defaults if the file doesn't exist.
    #[inline]
    pub fn load_or_default(path: &Path) -> Self {
        if path.exists() {
            Self::load(path).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Get the effective extensions for a package, considering overrides.
    #[inline]
    pub fn extensions_for_package(&self, package_name: &str) -> Vec<String> {
        self.find_matching_package_config(package_name)
            .and_then(|pkg_config| pkg_config.extensions.clone())
            .unwrap_or_else(|| self.defaults.extensions.clone())
    }

    /// Get the ignore patterns for a package.
    #[inline]
    pub fn ignore_for_package(&self, package_name: &str) -> Option<Vec<String>> {
        let pkg_config = self.find_matching_package_config(package_name)?;
        pkg_config.ignore.clone()
    }

    /// Find the first matching package config for a package name.
    fn find_matching_package_config(&self, package_name: &str) -> Option<&PackageConfig> {
        // Sort keys for deterministic iteration order
        let mut keys: Vec<_> = self.packages.keys().collect();
        keys.sort();
        for pattern in keys {
            if Self::pattern_matches(pattern, package_name) {
                return self.packages.get(pattern);
            }
        }
        None
    }

    /// Check if a glob pattern matches a package name.
    #[inline]
    fn pattern_matches(pattern: &str, package_name: &str) -> bool {
        glob::Pattern::new(pattern).is_ok_and(|p| p.matches(package_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.scan.include, vec!["."]);
        assert_eq!(
            config.scan.exclude,
            vec!["**/node_modules", "**/dist", "**/build"]
        );
        assert_eq!(config.defaults.extensions, vec!["ts", "tsx", "js", "jsx"]);
    }

    #[test]
    fn test_parse_yaml() {
        let yaml = r#"
scan:
  include:
    - "libs/*"
    - "apps/*"
  exclude:
    - "**/node_modules"

defaults:
  extensions: [ts, tsx]

packages:
  "@myorg/legacy-*":
    extensions: [js]
"#;
        let config: Config =
            serde_yaml::from_str(yaml).expect("test yaml should parse successfully");
        assert_eq!(config.scan.include, vec!["libs/*", "apps/*"]);
        assert_eq!(config.defaults.extensions, vec!["ts", "tsx"]);
        assert!(config.packages.contains_key("@myorg/legacy-*"));
    }
}
