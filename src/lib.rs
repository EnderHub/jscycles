//! Fast circular dependency detection for JavaScript/TypeScript projects.
//!
//! `jscycles` is a CLI tool that detects circular dependencies within JS/TS packages,
//! designed as a drop-in replacement for `madge --circular` with 50-100x performance improvement.

pub mod config;
pub mod cycles;
pub mod discovery;
pub mod graph;
pub mod imports;
pub mod output;
pub mod tsconfig;
pub mod workspace;

use std::path::PathBuf;

use thiserror::Error;

/// Top-level error type for jscycles operations.
#[derive(Debug, Error)]
pub enum JscyclesError {
    /// Configuration file error.
    #[error("Config error: {0}")]
    Config(#[from] ConfigError),

    /// Package discovery error.
    #[error("Discovery error: {0}")]
    Discovery(#[from] DiscoveryError),

    /// Import extraction error.
    #[error("Import extraction error: {0}")]
    Import(#[from] ExtractError),

    /// TypeScript config error.
    #[error("TSConfig error: {0}")]
    TsConfig(#[from] TsConfigError),

    /// Workspace detection error.
    #[error("Workspace error: {0}")]
    Workspace(#[from] WorkspaceError),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Error type for configuration operations.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Failed to read config file.
    #[error("Failed to read config file: {0}")]
    Read(#[source] std::io::Error),

    /// Failed to parse config YAML.
    #[error("Failed to parse config YAML: {0}")]
    Parse(#[source] serde_yaml::Error),
}

/// Error type for package discovery operations.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// Invalid glob pattern.
    #[error("Invalid glob pattern '{pattern}': {source}")]
    InvalidPattern {
        /// The invalid pattern.
        pattern: String,
        /// The underlying error.
        #[source]
        source: glob::PatternError,
    },

    /// Failed to read directory.
    #[error("Failed to read directory {path}: {source}")]
    ReadDir {
        /// The directory path.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse package.json.
    #[error("Failed to parse package.json at {path}: {source}")]
    ParsePackageJson {
        /// The package.json path.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: serde_json::Error,
    },

    /// Missing package name in package.json.
    #[error("Missing 'name' field in package.json at {0}")]
    MissingPackageName(PathBuf),
}

/// Error type for import extraction operations.
#[derive(Debug, Error)]
pub enum ExtractError {
    /// Failed to parse file.
    #[error("Failed to parse {path}: {message}")]
    ParseError {
        /// The file path.
        path: PathBuf,
        /// The error message.
        message: String,
    },

    /// IO error during extraction.
    #[error("IO error reading {path}: {source}")]
    IoError {
        /// The file path.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Error type for TypeScript config operations.
#[derive(Debug, Error)]
pub enum TsConfigError {
    /// Failed to read tsconfig.json.
    #[error("Failed to read tsconfig.json: {0}")]
    Read(#[source] std::io::Error),

    /// Failed to parse tsconfig.json.
    #[error("Failed to parse tsconfig.json: {0}")]
    Parse(#[source] serde_json::Error),

    /// Invalid path in tsconfig.
    #[error("Invalid path in tsconfig: {0}")]
    InvalidPath(String),

    /// Extended tsconfig not found.
    #[error("Extended tsconfig not found: {0}")]
    ExtendsNotFound(String),

    /// Extends chain too deep (possible circular reference).
    #[error("Extends chain too deep (max 10), possible circular reference")]
    ExtendsDepth,
}

/// Error type for workspace detection operations.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    /// Failed to read workspace config file.
    #[error("Failed to read {path}: {source}")]
    Read {
        /// The file path.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse workspace config.
    #[error("Failed to parse {path}: {message}")]
    Parse {
        /// The file path.
        path: PathBuf,
        /// The error message.
        message: String,
    },

    /// Invalid glob pattern.
    #[error("Invalid glob pattern '{pattern}': {source}")]
    InvalidPattern {
        /// The invalid pattern.
        pattern: String,
        /// The underlying error.
        #[source]
        source: glob::PatternError,
    },
}

// Re-exports for public API
pub use config::Config;
pub use cycles::{
    Cycle, CycleDetector, PackageCycle, PackageCycleDetector, PackageCycleEdge,
    PackageCycleWithFiles,
};
pub use discovery::{Package, PackageDiscovery};
pub use graph::{DependencyGraph, EdgeEvidence, PackageDependencyGraph};
pub use imports::{Import, ImportExtractor, ImportTarget};
pub use output::{
    CycleFilter, HumanFormatter, JsonFormatter, OutputFormatter, PackageResult, Results,
    UnifiedOutputFormatter, UnifiedResults,
};
pub use tsconfig::{TsConfig, TsConfigCache};
pub use workspace::{Workspace, WorkspaceFormat};
