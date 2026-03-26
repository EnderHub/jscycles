//! jscycles - Fast circular dependency detection for JS/TS.
//!
//! A drop-in replacement for `madge --circular` with 50-100x performance improvement.

use std::collections::{BTreeSet, HashMap};
use std::io::{self, BufRead as _, IsTerminal as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Arg, ArgAction, Command};
use rayon::prelude::*;
use serde::Deserialize;

use jscycles::cycles::PackageCycleDetector;
use jscycles::graph::PackageDependencyGraph;
use jscycles::imports::{ExtractionMode, Import};
use jscycles::output::{CycleFilter, UnifiedOutputFormatter as _, UnifiedResults};
use jscycles::{
    Config, CycleDetector, DependencyGraph, HumanFormatter, ImportExtractor, JscyclesError,
    JsonFormatter, Package, PackageDiscovery, PackageResult, TsConfig, TsConfigCache, Workspace,
};

/// Build the CLI command.
#[expect(clippy::too_many_lines, reason = "CLI definition is naturally long")]
fn build_cli() -> Command {
    Command::new("jscycles")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Fast circular dependency detection for JavaScript/TypeScript")
        .arg(
            Arg::new("paths")
                .help("Paths to check (if empty, uses config scan paths)")
                .action(ArgAction::Append)
                .value_parser(clap::value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("only")
                .long("only")
                .help("Only check packages matching glob pattern (can be repeated)")
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("exclude")
                .long("exclude")
                .help("Exclude packages matching glob pattern (can be repeated)")
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("extensions")
                .long("extensions")
                .help("File extensions to analyze")
                .default_value("ts,tsx,js,jsx")
                .value_delimiter(','),
        )
        .arg(
            Arg::new("tsconfig")
                .long("tsconfig")
                .help("Path to tsconfig.json")
                .value_parser(clap::value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("no-tsconfig")
                .long("no-tsconfig")
                .help("Skip tsconfig.json auto-detection")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .help("Output as JSON")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("stdin")
                .long("stdin")
                .help("Read paths from stdin")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Path to config file")
                .default_value("jscycles.yaml")
                .value_parser(clap::value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .help("Only output if cycles found")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("inner")
                .long("inner")
                .help("Show only file-level (inner) cycles")
                .action(ArgAction::SetTrue)
                .conflicts_with("outer"),
        )
        .arg(
            Arg::new("outer")
                .long("outer")
                .help("Show only package-level (outer) cycles")
                .action(ArgAction::SetTrue)
                .conflicts_with("inner"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Show all packages (including clean) and file-level details")
                .action(ArgAction::SetTrue),
        )
}

/// Options needed during package processing.
struct ProcessOptions {
    /// Preloaded explicit tsconfig.json, if provided.
    explicit_tsconfig: Option<TsConfig>,
    /// Skip tsconfig.json auto-detection.
    no_tsconfig: bool,
    /// File extensions to analyze.
    extensions: Vec<String>,
    /// Workspace configuration for inter-package cycle detection.
    workspace: Option<Workspace>,
    /// Shared tsconfig discovery/load cache.
    tsconfig_cache: Arc<TsConfigCache>,

    /// How much work to do while processing each package.
    mode: ProcessMode,
}

/// Result of processing a package (includes imports for package graph).
struct ProcessedPackage {
    /// File-level cycle detection result.
    result: PackageResult,
    /// All imports from this package (for building package graph).
    imports: Vec<Import>,
}

/// Minimal package.json dependency sections for package-level cycle detection.
#[derive(Debug, Default, Deserialize)]
struct PackageManifest {
    /// Runtime dependencies.
    dependencies: Option<HashMap<String, String>>,
    /// Development dependencies.
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<HashMap<String, String>>,
    /// Peer dependencies.
    #[serde(rename = "peerDependencies")]
    peer_dependencies: Option<HashMap<String, String>>,
    /// Optional dependencies.
    #[serde(rename = "optionalDependencies")]
    optional_dependencies: Option<HashMap<String, String>>,
}

/// Controls how much package processing is required for a given CLI mode.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProcessMode {
    /// Full import extraction plus file-level cycle detection.
    Full,
    /// Only collect workspace-package imports for package-level cycles.
    WorkspaceOnly,
}

/// Run the cycle detection.
#[expect(
    clippy::too_many_lines,
    reason = "main entry point with necessary setup"
)]
fn run(matches: &clap::ArgMatches) -> Result<bool, JscyclesError> {
    // Extract arguments
    let default_config = PathBuf::from("jscycles.yaml");
    let config_path: &PathBuf = matches.get_one("config").unwrap_or(&default_config);
    let paths: Vec<PathBuf> = matches
        .get_many::<PathBuf>("paths")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let only: Vec<String> = matches
        .get_many::<String>("only")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let exclude: Vec<String> = matches
        .get_many::<String>("exclude")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let extensions: Vec<String> = matches
        .get_many::<String>("extensions")
        .map(|v| v.cloned().collect())
        .unwrap_or_else(|| {
            vec![
                "ts".to_owned(),
                "tsx".to_owned(),
                "js".to_owned(),
                "jsx".to_owned(),
            ]
        });
    let tsconfig: Option<PathBuf> = matches.get_one::<PathBuf>("tsconfig").cloned();
    let no_tsconfig = matches.get_flag("no-tsconfig");
    let json_output = matches.get_flag("json");
    let stdin_mode = matches.get_flag("stdin");
    let quiet = matches.get_flag("quiet");
    let verbose = matches.get_flag("verbose");

    // Determine cycle filter
    let filter = if matches.get_flag("inner") {
        CycleFilter::Inner
    } else if matches.get_flag("outer") {
        CycleFilter::Outer
    } else {
        CycleFilter::All
    };

    // Load config
    let config = Config::load_or_default(config_path);

    // Create package discovery
    let discovery = PackageDiscovery::new(&config, &only, &exclude)?;

    // Determine paths to scan first (needed for workspace discovery)
    let explicit_paths = !paths.is_empty();
    let scan_paths: Vec<PathBuf> = if stdin_mode {
        // Read paths from stdin
        io::stdin()
            .lock()
            .lines()
            .map_while(Result::ok)
            .map(PathBuf::from)
            .collect()
    } else if paths.is_empty() {
        // Use current directory
        vec![PathBuf::from(".")]
    } else {
        paths
    };

    // Discover workspace from scan paths (not current directory)
    // This allows detecting workspaces when scanning external directories
    let workspace = if filter == CycleFilter::Inner {
        None
    } else {
        let workspace_root = scan_paths.first().map_or_else(
            || PathBuf::from("."),
            |p| {
                // Canonicalize to get absolute path, fall back to the path itself
                p.canonicalize().unwrap_or_else(|_| p.clone())
            },
        );
        Workspace::discover(&workspace_root)?
    };

    let process_mode = if filter == CycleFilter::Outer && workspace.is_some() {
        ProcessMode::WorkspaceOnly
    } else {
        ProcessMode::Full
    };

    let tsconfig_cache = Arc::new(TsConfigCache::new());
    let explicit_tsconfig = if no_tsconfig || process_mode == ProcessMode::WorkspaceOnly {
        None
    } else {
        tsconfig
            .as_deref()
            .and_then(|path| tsconfig_cache.load(path))
    };

    // Create processing options
    let opts = ProcessOptions {
        explicit_tsconfig,
        no_tsconfig,
        extensions,
        workspace: workspace.clone(),
        tsconfig_cache,
        mode: process_mode,
    };

    // Discover packages
    let packages = if process_mode == ProcessMode::WorkspaceOnly {
        workspace
            .as_ref()
            .map(|ws| discovery.discover_workspace_packages(ws, &scan_paths))
            .unwrap_or_default()
    } else if !explicit_paths && !stdin_mode {
        discovery.discover(&PathBuf::from("."))?
    } else {
        discovery.discover_explicit(&scan_paths)?
    };

    // Process packages in parallel (returns both file cycles and imports)
    let processed: Vec<ProcessedPackage> = packages
        .par_iter()
        .map(|package| process_package_with_imports(package, &opts))
        .collect::<Result<Vec<_>, _>>()?;

    // Build package dependency graph and detect package-level cycles
    let (package_cycles, package_cycles_with_files) = if workspace.is_some() {
        let mut imports_by_package = HashMap::new();
        for proc in &processed {
            let _ = imports_by_package.insert(proc.result.name.clone(), proc.imports.clone());
        }
        let pkg_graph = PackageDependencyGraph::from_imports(&imports_by_package);
        if verbose {
            // Verbose mode: include file-level details (slower)
            (
                Vec::new(),
                PackageCycleDetector::detect_with_files(&pkg_graph),
            )
        } else {
            // Default: just package names (faster)
            (PackageCycleDetector::detect(&pkg_graph), Vec::new())
        }
    } else {
        (Vec::new(), Vec::new())
    };

    // Build unified results
    let unified = UnifiedResults {
        file_results: processed.into_iter().map(|p| p.result).collect(),
        package_cycles,
        package_cycles_with_files,
    };

    // Output results
    let use_color = io::stdout().is_terminal();
    let mut stdout = io::stdout().lock();

    let has_cycles = match filter {
        CycleFilter::All => unified.has_any_cycles(),
        CycleFilter::Inner => unified.has_file_cycles(),
        CycleFilter::Outer => unified.has_package_cycles(),
    };

    if quiet && !has_cycles {
        return Ok(false);
    }

    if json_output {
        let formatter = JsonFormatter::new();
        formatter.format_unified(&unified, filter, &mut stdout)?;
    } else {
        let formatter = HumanFormatter::new(use_color).with_verbose(verbose);
        formatter.format_unified(&unified, filter, &mut stdout)?;
    }

    stdout.flush()?;

    Ok(has_cycles)
}

/// Process a single package, returning both file cycles and imports.
fn process_package_with_imports(
    package: &Package,
    opts: &ProcessOptions,
) -> Result<ProcessedPackage, JscyclesError> {
    // Determine tsconfig
    let tsconfig = if opts.mode == ProcessMode::WorkspaceOnly || opts.no_tsconfig {
        None
    } else if let Some(config) = &opts.explicit_tsconfig {
        Some(config.clone())
    } else {
        opts.tsconfig_cache.discover(&package.path)
    };

    // Get extensions for this package
    let extensions = package
        .config
        .extensions
        .clone()
        .unwrap_or_else(|| opts.extensions.clone());

    // Create import extractor with workspace context
    let mut extractor = ImportExtractor::new(extensions, tsconfig)
        .with_workspace(opts.workspace.clone())
        .with_mode(match opts.mode {
            ProcessMode::Full => ExtractionMode::All,
            ProcessMode::WorkspaceOnly => ExtractionMode::WorkspaceOnly,
        });
    if let Some(ignore) = &package.config.ignore {
        extractor = extractor.with_ignore_patterns(ignore.clone());
    }

    // Extract imports
    let imports = if opts.mode == ProcessMode::WorkspaceOnly {
        match opts
            .workspace
            .as_ref()
            .and_then(|workspace| collect_workspace_manifest_imports(package, workspace))
        {
            Some(manifest_imports) if !manifest_imports.is_empty() => manifest_imports,
            _ => extractor.extract(&package.path)?,
        }
    } else {
        extractor.extract(&package.path)?
    };

    let cycles = if opts.mode == ProcessMode::WorkspaceOnly {
        Vec::new()
    } else {
        let graph = DependencyGraph::from_imports(&imports);
        CycleDetector::detect(&graph)
    };

    let result = PackageResult {
        name: package.name.clone(),
        path: package.path.clone(),
        cycles,
    };

    Ok(ProcessedPackage { result, imports })
}

/// Read workspace-package edges from a package manifest.
///
/// Returns `None` if the package.json cannot be read or parsed, allowing
/// callers to fall back to source-level import scanning.
fn collect_workspace_manifest_imports(
    package: &Package,
    workspace: &Workspace,
) -> Option<Vec<Import>> {
    let package_json_path = package.path.join("package.json");
    let contents = std::fs::read_to_string(&package_json_path).ok()?;
    let manifest: PackageManifest = serde_json::from_str(&contents).ok()?;

    let mut deps = BTreeSet::new();
    for section in [
        manifest.dependencies,
        manifest.dev_dependencies,
        manifest.peer_dependencies,
        manifest.optional_dependencies,
    ] {
        let Some(entries) = section else {
            continue;
        };
        for dependency in entries.into_keys() {
            if workspace.is_workspace_package(&dependency) {
                let _ = deps.insert(dependency);
            }
        }
    }

    Some(
        deps.into_iter()
            .map(|dependency| Import {
                source: package_json_path.clone(),
                target: jscycles::ImportTarget::WorkspacePackage {
                    package_name: workspace.resolve_package_name(&dependency).to_owned(),
                    subpath: None,
                },
                specifier: dependency,
            })
            .collect(),
    )
}

fn main() -> ExitCode {
    let cli = build_cli();
    let matches = cli.get_matches();

    match run(&matches) {
        Ok(has_cycles) => {
            if has_cycles {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        },
        Err(err) => {
            drop(writeln!(io::stderr(), "Error: {err}"));
            ExitCode::from(2)
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use jscycles::workspace::WorkspaceFormat;

    #[test]
    fn test_collect_workspace_manifest_imports_uses_declared_dependencies() {
        let temp = tempdir().expect("tempdir should be created");
        let package_path = temp.path().join("pkg-a");
        fs::create_dir_all(&package_path).expect("package dir should be created");
        fs::write(
            package_path.join("package.json"),
            r#"{
  "name": "@demo/a",
  "dependencies": {
    "@demo/b": "workspace:*",
    "react": "^19.0.0"
  },
  "devDependencies": {
    "@demo/c": "workspace:*"
  }
}"#,
        )
        .expect("package.json should be written");

        let package = Package {
            name: "@demo/a".to_owned(),
            path: package_path,
            config: jscycles::config::PackageConfig::default(),
        };

        let mut packages = HashMap::new();
        let _ = packages.insert("@demo/a".to_owned(), temp.path().join("pkg-a"));
        let _ = packages.insert("@demo/b".to_owned(), temp.path().join("pkg-b"));
        let _ = packages.insert("@demo/c".to_owned(), temp.path().join("pkg-c"));

        let workspace = Workspace {
            root: temp.path().to_path_buf(),
            format: WorkspaceFormat::Npm,
            packages,
            aliases: HashMap::new(),
        };

        let imports = collect_workspace_manifest_imports(&package, &workspace)
            .expect("manifest imports should be collected");

        let specifiers: Vec<_> = imports
            .iter()
            .map(|import| import.specifier.as_str())
            .collect();
        assert_eq!(specifiers, vec!["@demo/b", "@demo/c"]);
    }
}
