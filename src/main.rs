//! jscycles - Fast circular dependency detection for JS/TS.
//!
//! A drop-in replacement for `madge --circular` with 50-100x performance improvement.

use std::io::{self, BufRead as _, IsTerminal as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};
use rayon::prelude::*;

use jscycles::cycles::PackageCycleDetector;
use jscycles::graph::PackageDependencyGraph;
use jscycles::imports::Import;
use jscycles::output::{CycleFilter, UnifiedOutputFormatter as _, UnifiedResults};
use jscycles::{
    Config, CycleDetector, DependencyGraph, HumanFormatter, ImportExtractor, JscyclesError,
    JsonFormatter, Package, PackageDiscovery, PackageResult, TsConfig, Workspace,
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
                .help("Show file-level details for inter-package cycles")
                .action(ArgAction::SetTrue),
        )
}

/// Options needed during package processing.
struct ProcessOptions {
    /// Path to tsconfig.json.
    tsconfig: Option<PathBuf>,
    /// Skip tsconfig.json auto-detection.
    no_tsconfig: bool,
    /// File extensions to analyze.
    extensions: Vec<String>,
    /// Workspace configuration for inter-package cycle detection.
    workspace: Option<Workspace>,
}

/// Result of processing a package (includes imports for package graph).
struct ProcessedPackage {
    /// File-level cycle detection result.
    result: PackageResult,
    /// All imports from this package (for building package graph).
    imports: Vec<Import>,
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
    let workspace_root = scan_paths.first().map_or_else(
        || PathBuf::from("."),
        |p| {
            // Canonicalize to get absolute path, fall back to the path itself
            p.canonicalize().unwrap_or_else(|_| p.clone())
        },
    );
    let workspace = Workspace::discover(&workspace_root)?;

    // Create processing options
    let opts = ProcessOptions {
        tsconfig,
        no_tsconfig,
        extensions,
        workspace: workspace.clone(),
    };

    // Discover packages
    let packages = if !explicit_paths && !stdin_mode {
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
        let mut imports_by_package = std::collections::HashMap::new();
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
        let formatter = HumanFormatter::new(use_color);
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
    let tsconfig = if opts.no_tsconfig {
        None
    } else if let Some(path) = &opts.tsconfig {
        TsConfig::load(path).ok()
    } else {
        TsConfig::discover(&package.path)
    };

    // Get extensions for this package
    let extensions = package
        .config
        .extensions
        .clone()
        .unwrap_or_else(|| opts.extensions.clone());

    // Create import extractor with workspace context
    let mut extractor =
        ImportExtractor::new(extensions, tsconfig).with_workspace(opts.workspace.clone());
    if let Some(ignore) = &package.config.ignore {
        extractor = extractor.with_ignore_patterns(ignore.clone());
    }

    // Extract imports
    let imports = extractor.extract(&package.path)?;

    // Build dependency graph
    let graph = DependencyGraph::from_imports(&imports);

    // Detect cycles
    let cycles = CycleDetector::detect(&graph);

    let result = PackageResult {
        name: package.name.clone(),
        path: package.path.clone(),
        cycles,
    };

    Ok(ProcessedPackage { result, imports })
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
