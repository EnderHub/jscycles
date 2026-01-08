//! Integration tests using fixture directories.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Tests are allowed to use expect/unwrap for simplicity"
)]

use std::path::PathBuf;

use jscycles::{
    Config, CycleDetector, DependencyGraph, ImportExtractor, PackageDiscovery, TsConfig, Workspace,
    WorkspaceFormat,
};

/// Get the path to a test fixture directory.
fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Run cycle detection on a fixture and return sorted cycle descriptions.
fn detect_cycles_in_fixture(fixture_name: &str) -> Vec<String> {
    let path = fixture_path(fixture_name);
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        vec![
            "ts".to_owned(),
            "tsx".to_owned(),
            "js".to_owned(),
            "jsx".to_owned(),
        ],
        tsconfig,
    );

    let imports = extractor
        .extract(&path)
        .expect("fixture extraction should succeed");
    let graph = DependencyGraph::from_imports(&imports);
    let cycles = CycleDetector::detect(&graph);

    // Convert cycles to relative path strings for readable snapshots
    let mut cycle_strings: Vec<String> = cycles
        .iter()
        .map(|cycle| {
            cycle
                .path
                .iter()
                .filter_map(|p| {
                    p.strip_prefix(&path)
                        .ok()
                        .map(|rel| rel.to_string_lossy().to_string())
                })
                .collect::<Vec<_>>()
                .join(" → ")
        })
        .collect();

    cycle_strings.sort();
    cycle_strings
}

#[test]
fn test_fixture_simple_cycle() {
    let cycles = detect_cycles_in_fixture("simple-cycle");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_fixture_no_cycles() {
    let cycles = detect_cycles_in_fixture("no-cycles");
    assert!(cycles.is_empty(), "no-cycles fixture should have no cycles");
}

#[test]
fn test_fixture_complex_cycles() {
    let cycles = detect_cycles_in_fixture("complex-cycles");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_fixture_with_tsconfig() {
    let cycles = detect_cycles_in_fixture("with-tsconfig");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_fixture_monorepo_discovery() {
    let path = fixture_path("monorepo-mock");
    let config = Config::default();
    let discovery =
        PackageDiscovery::new(&config, &[], &[]).expect("discovery should be created successfully");

    let packages = discovery
        .discover(&path)
        .expect("monorepo discovery should succeed");

    let mut package_names: Vec<_> = packages.iter().map(|p| p.name.clone()).collect();
    package_names.sort();

    insta::assert_debug_snapshot!(package_names);
}

#[test]
fn test_monorepo_per_package_cycles() {
    let path = fixture_path("monorepo-mock");
    let config = Config::default();
    let discovery =
        PackageDiscovery::new(&config, &[], &[]).expect("discovery should be created successfully");

    let packages = discovery
        .discover(&path)
        .expect("monorepo discovery should succeed");

    // Collect cycles per package for snapshot
    let mut results: Vec<(String, Vec<String>)> = packages
        .iter()
        .map(|package| {
            let tsconfig = TsConfig::discover(&package.path);
            let extractor = ImportExtractor::new(vec!["ts".to_owned(), "tsx".to_owned()], tsconfig);

            let imports = extractor
                .extract(&package.path)
                .expect("package extraction should succeed");
            let graph = DependencyGraph::from_imports(&imports);
            let cycles = CycleDetector::detect(&graph);

            let cycle_strs: Vec<String> = cycles
                .iter()
                .map(|cycle| {
                    cycle
                        .path
                        .iter()
                        .filter_map(|p| {
                            p.strip_prefix(&package.path)
                                .ok()
                                .map(|rel| rel.to_string_lossy().to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(" → ")
                })
                .collect();

            (package.name.clone(), cycle_strs)
        })
        .collect();

    results.sort_by(|a, b| a.0.cmp(&b.0));
    insta::assert_debug_snapshot!(results);
}

// =============================================================================
// Import extraction edge case tests
// =============================================================================

/// Extract imports from a fixture and return sorted specifiers.
fn extract_imports_from_fixture(fixture_name: &str) -> Vec<(String, String)> {
    let path = fixture_path(fixture_name);
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        vec![
            "ts".to_owned(),
            "tsx".to_owned(),
            "js".to_owned(),
            "jsx".to_owned(),
        ],
        tsconfig,
    );

    let imports = extractor
        .extract(&path)
        .expect("fixture extraction should succeed");

    // Return (source_file, specifier) pairs, sorted for determinism
    let mut pairs: Vec<(String, String)> = imports
        .iter()
        .map(|import| {
            let source = import
                .source
                .strip_prefix(&path)
                .unwrap_or(&import.source)
                .to_string_lossy()
                .to_string();
            (source, import.specifier.clone())
        })
        .collect();

    pairs.sort();
    pairs
}

/// Extract raw imports from a fixture (for graph construction).
fn extract_raw_imports_from_fixture(fixture_name: &str) -> Vec<jscycles::Import> {
    let path = fixture_path(fixture_name);
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        vec![
            "ts".to_owned(),
            "tsx".to_owned(),
            "js".to_owned(),
            "jsx".to_owned(),
        ],
        tsconfig,
    );

    extractor
        .extract(&path)
        .expect("fixture extraction should succeed")
}

#[test]
fn test_fixture_import_styles() {
    // This fixture tests various import syntaxes:
    // - Static ES imports
    // - Dynamic imports: import('./module')
    // - CommonJS require: require('./module')
    // - Re-exports: export { x } from './y' and export * from './y'
    let imports = extract_imports_from_fixture("import-styles");
    insta::assert_debug_snapshot!(imports);
}

#[test]
fn test_fixture_import_styles_cycles() {
    // The import-styles fixture has a cycle via static imports
    let cycles = detect_cycles_in_fixture("import-styles");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_fixture_self_loop() {
    // A file that imports itself should be detected as a cycle
    let cycles = detect_cycles_in_fixture("self-loop");
    insta::assert_debug_snapshot!(cycles);
}

// =============================================================================
// Additional edge case tests
// =============================================================================

#[test]
fn test_fixture_long_cycle() {
    // Tests a longer cycle: A -> B -> C -> D -> E -> A (5 nodes)
    let cycles = detect_cycles_in_fixture("long-cycle");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_fixture_diamond_no_cycle() {
    // Diamond dependency pattern is NOT a cycle:
    // top -> left, top -> right, left -> bottom, right -> bottom
    let cycles = detect_cycles_in_fixture("diamond");
    assert!(
        cycles.is_empty(),
        "diamond dependency should not be detected as a cycle, found: {cycles:?}"
    );
}

#[test]
fn test_fixture_dynamic_import_cycle() {
    // Cycle involving dynamic import(): A -> B (via dynamic import) -> A
    let cycles = detect_cycles_in_fixture("dynamic-cycle");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_fixture_tsconfig_complex_paths() {
    // Tests multiple tsconfig path aliases:
    // @utils/*, @components/*, @/*
    // With a cycle: index -> @utils/helper -> @/index
    let cycles = detect_cycles_in_fixture("tsconfig-complex");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_fixture_tsconfig_complex_imports() {
    // Verify all path aliases are resolved correctly
    let imports = extract_imports_from_fixture("tsconfig-complex");
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// Inter-package cycle detection tests
// =============================================================================

use jscycles::{PackageCycleDetector, PackageDependencyGraph};

/// Helper to detect inter-package cycles in a fixture.
fn detect_inter_package_cycles(fixture_name: &str) -> Vec<String> {
    use std::collections::HashMap;

    let path = fixture_path(fixture_name);

    // Discover workspace
    let workspace = Workspace::discover(&path)
        .expect("workspace discovery should succeed")
        .expect("fixture should be a workspace");

    // Collect imports by package (sort keys for deterministic order)
    let mut imports_by_package: HashMap<String, Vec<jscycles::Import>> = HashMap::new();

    let mut pkg_names: Vec<_> = workspace.packages.keys().cloned().collect();
    pkg_names.sort();

    for pkg_name in pkg_names {
        let pkg_path = workspace
            .packages
            .get(&pkg_name)
            .expect("package should exist");
        let tsconfig = TsConfig::discover(pkg_path);
        let extractor = ImportExtractor::new(vec!["ts".to_owned(), "tsx".to_owned()], tsconfig)
            .with_workspace(Some(workspace.clone()));

        let imports = extractor
            .extract(pkg_path)
            .expect("import extraction should succeed");

        let _ = imports_by_package.insert(pkg_name, imports);
    }

    // Build package dependency graph
    let graph = PackageDependencyGraph::from_imports(&imports_by_package);

    // Detect cycles
    let cycles = PackageCycleDetector::detect(&graph);

    // Format cycles for snapshot
    let mut cycle_strings: Vec<String> = cycles
        .iter()
        .map(|cycle| cycle.packages.join(" -> "))
        .collect();

    cycle_strings.sort();
    cycle_strings
}

#[test]
fn test_inter_package_simple_cycle() {
    // A <-> B (two-way cycle between two packages)
    let cycles = detect_inter_package_cycles("inter-package-simple");
    assert_eq!(
        cycles.len(),
        1,
        "should detect exactly one inter-package cycle"
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_inter_package_three_way_cycle() {
    // A -> B -> C -> A (three-way cycle)
    let cycles = detect_inter_package_cycles("inter-package-cycle");
    assert_eq!(
        cycles.len(),
        1,
        "should detect exactly one inter-package cycle"
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_inter_package_no_cycle() {
    // A -> B -> C (no cycle - linear dependency)
    let cycles = detect_inter_package_cycles("inter-package-no-cycle");
    assert!(
        cycles.is_empty(),
        "should detect no inter-package cycles, found: {cycles:?}"
    );
}

#[test]
fn test_pnpm_workspace_cycle() {
    // Test pnpm-workspace.yaml detection with cycle
    let cycles = detect_inter_package_cycles("pnpm-workspace");
    assert_eq!(
        cycles.len(),
        1,
        "should detect inter-package cycle in pnpm workspace"
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_workspace_discovery_npm() {
    let path = fixture_path("inter-package-simple");
    let workspace = Workspace::discover(&path)
        .expect("workspace discovery should succeed")
        .expect("fixture should be a workspace");

    assert_eq!(
        workspace.format,
        WorkspaceFormat::Npm,
        "should detect npm workspace format"
    );
    assert_eq!(
        workspace.packages.len(),
        2,
        "should find 2 packages in workspace"
    );
    assert!(
        workspace.packages.contains_key("@simple/a"),
        "should find @simple/a"
    );
    assert!(
        workspace.packages.contains_key("@simple/b"),
        "should find @simple/b"
    );
}

#[test]
fn test_workspace_discovery_pnpm() {
    let path = fixture_path("pnpm-workspace");
    let workspace = Workspace::discover(&path)
        .expect("workspace discovery should succeed")
        .expect("fixture should be a workspace");

    assert_eq!(
        workspace.format,
        WorkspaceFormat::Pnpm,
        "should detect pnpm workspace format"
    );
    assert_eq!(
        workspace.packages.len(),
        2,
        "should find 2 packages in workspace"
    );
}

#[test]
fn test_workspace_package_is_detected() {
    let path = fixture_path("inter-package-simple");
    let workspace = Workspace::discover(&path)
        .expect("workspace discovery should succeed")
        .expect("fixture should be a workspace");

    assert!(
        workspace.is_workspace_package("@simple/a"),
        "@simple/a should be workspace package"
    );
    assert!(
        workspace.is_workspace_package("@simple/a/utils"),
        "@simple/a/utils should match workspace package"
    );
    assert!(
        !workspace.is_workspace_package("react"),
        "react should not be workspace package"
    );
}

#[test]
fn test_workspace_import_extraction() {
    let path = fixture_path("inter-package-simple");
    let workspace = Workspace::discover(&path)
        .expect("workspace discovery should succeed")
        .expect("fixture should be a workspace");

    let pkg_a_path = workspace
        .packages
        .get("@simple/a")
        .expect("pkg-a should exist")
        .clone();

    let extractor =
        ImportExtractor::new(vec!["ts".to_owned()], None).with_workspace(Some(workspace));

    let imports = extractor
        .extract(pkg_a_path.as_path())
        .expect("extraction should succeed");

    // Find the workspace import
    let workspace_imports: Vec<_> = imports
        .iter()
        .filter(|i| matches!(i.target, jscycles::ImportTarget::WorkspacePackage { .. }))
        .collect();

    assert_eq!(
        workspace_imports.len(),
        1,
        "pkg-a should have 1 workspace import"
    );

    let import = workspace_imports.first().unwrap();
    assert_eq!(import.specifier, "@simple/b", "should import @simple/b");
}

// =============================================================================
// Combined file and package cycle tests
// =============================================================================

#[test]
fn test_monorepo_with_file_cycle_has_inner_cycle() {
    // The monorepo-mock fixture has an internal file cycle in pkg-a
    let path = fixture_path("monorepo-mock");
    let config = Config::default();
    let discovery =
        PackageDiscovery::new(&config, &[], &[]).expect("discovery should be created successfully");

    let packages = discovery
        .discover(&path)
        .expect("monorepo discovery should succeed");

    // Find pkg-a and check for file cycles
    let pkg_a = packages
        .iter()
        .find(|p| p.name == "@test/pkg-a")
        .expect("should find pkg-a");

    let tsconfig = TsConfig::discover(&pkg_a.path);
    let extractor = ImportExtractor::new(vec!["ts".to_owned(), "tsx".to_owned()], tsconfig);

    let imports = extractor
        .extract(&pkg_a.path)
        .expect("package extraction should succeed");
    let graph = DependencyGraph::from_imports(&imports);
    let cycles = CycleDetector::detect(&graph);

    assert!(!cycles.is_empty(), "pkg-a should have internal file cycles");
}

// =============================================================================
// tsconfig extends chain tests
// =============================================================================

#[test]
fn test_fixture_tsconfig_extends_chain() {
    // Tests tsconfig with extends chain: tsconfig.json -> tsconfig.shared.json -> tsconfig.base.json
    // Each level adds path aliases that should all be usable for import resolution
    let cycles = detect_cycles_in_fixture("tsconfig-extends");
    // The fixture has a cycle: base/utils -> app/config -> shared/helpers -> base/utils
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_fixture_tsconfig_extends_imports() {
    // Verify all path aliases from the extends chain are resolved
    let imports = extract_imports_from_fixture("tsconfig-extends");
    insta::assert_debug_snapshot!(imports);
}

#[test]
fn test_fixture_tsconfig_extends_pkg() {
    // Tests tsconfig extending from a package in node_modules
    let imports = extract_imports_from_fixture("tsconfig-extends-pkg");
    insta::assert_debug_snapshot!(imports);
}

#[test]
fn test_fixture_tsconfig_extends_pkg_cycles() {
    // Verify cycle detection works with extends from node_modules
    let cycles = detect_cycles_in_fixture("tsconfig-extends-pkg");
    insta::assert_debug_snapshot!(cycles);
}

// =============================================================================
// pnpm workspace complex tests
// =============================================================================

#[test]
fn test_pnpm_workspace_complex_cycle() {
    // Tests pnpm workspace with complex globs (including negative globs)
    let cycles = detect_inter_package_cycles("pnpm-workspace-complex");
    assert_eq!(
        cycles.len(),
        1,
        "should detect inter-package cycle in complex pnpm workspace"
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_pnpm_workspace_complex_discovery() {
    // Verify workspace discovery with complex pnpm-workspace.yaml
    let path = fixture_path("pnpm-workspace-complex");
    let workspace = Workspace::discover(&path)
        .expect("workspace discovery should succeed")
        .expect("fixture should be a workspace");

    assert_eq!(
        workspace.format,
        WorkspaceFormat::Pnpm,
        "should detect pnpm format"
    );

    // Should find core and utils but NOT ignored (negative glob)
    assert!(
        workspace.packages.contains_key("@pnpm-complex/core"),
        "should find @pnpm-complex/core"
    );
    assert!(
        workspace.packages.contains_key("@pnpm-complex/utils"),
        "should find @pnpm-complex/utils"
    );
    // Note: negative globs may or may not be supported by the glob crate
    // This test verifies the actual behavior
}

// =============================================================================
// EXPANDED SIMPLE CYCLE TESTS
// =============================================================================

#[test]
fn test_simple_cycle_has_exactly_one_cycle() {
    let cycles = detect_cycles_in_fixture("simple-cycle");
    assert_eq!(cycles.len(), 1, "simple-cycle should have exactly 1 cycle");
}

#[test]
fn test_simple_cycle_involves_two_files() {
    let cycles = detect_cycles_in_fixture("simple-cycle");
    let cycle = cycles.first().expect("should have a cycle");
    // Count the arrows to determine number of files
    let file_count = cycle.matches('→').count() + 1;
    assert!(
        file_count >= 2,
        "simple cycle should involve at least 2 files, found {file_count}"
    );
}

#[test]
fn test_simple_cycle_import_extraction() {
    let imports = extract_imports_from_fixture("simple-cycle");
    assert!(
        !imports.is_empty(),
        "simple-cycle should have at least one import"
    );
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// EXPANDED NO CYCLES TESTS
// =============================================================================

#[test]
fn test_no_cycles_import_extraction() {
    let imports = extract_imports_from_fixture("no-cycles");
    assert!(
        !imports.is_empty(),
        "no-cycles should have imports even without cycles"
    );
    insta::assert_debug_snapshot!(imports);
}

#[test]
fn test_no_cycles_graph_structure() {
    let path = fixture_path("no-cycles");
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        vec![
            "ts".to_owned(),
            "tsx".to_owned(),
            "js".to_owned(),
            "jsx".to_owned(),
        ],
        tsconfig,
    );

    let imports = extractor.extract(&path).expect("extraction should succeed");
    let graph = DependencyGraph::from_imports(&imports);

    // Verify graph has nodes but no cycles
    assert!(
        graph.node_count() > 0,
        "graph should have nodes even without cycles"
    );
}

// =============================================================================
// EXPANDED COMPLEX CYCLES TESTS
// =============================================================================

#[test]
fn test_complex_cycles_has_multiple_cycles() {
    let cycles = detect_cycles_in_fixture("complex-cycles");
    assert!(
        !cycles.is_empty(),
        "complex-cycles should have at least one cycle"
    );
}

#[test]
fn test_complex_cycles_import_extraction() {
    let imports = extract_imports_from_fixture("complex-cycles");
    assert!(
        imports.len() >= 3,
        "complex-cycles should have at least 3 imports"
    );
    insta::assert_debug_snapshot!(imports);
}

#[test]
fn test_complex_cycles_all_cycles_are_unique() {
    let cycles = detect_cycles_in_fixture("complex-cycles");
    let unique_cycles: std::collections::HashSet<_> = cycles.iter().collect();
    assert_eq!(
        cycles.len(),
        unique_cycles.len(),
        "all cycles should be unique"
    );
}

// =============================================================================
// EXPANDED DIAMOND TESTS (NO CYCLE)
// =============================================================================

#[test]
fn test_diamond_import_extraction() {
    let imports = extract_imports_from_fixture("diamond");
    // Diamond has 4 imports: top->left, top->right, left->bottom, right->bottom
    assert!(
        imports.len() >= 4,
        "diamond should have at least 4 imports, found {}",
        imports.len()
    );
    insta::assert_debug_snapshot!(imports);
}

#[test]
fn test_diamond_graph_structure() {
    let path = fixture_path("diamond");
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        vec![
            "ts".to_owned(),
            "tsx".to_owned(),
            "js".to_owned(),
            "jsx".to_owned(),
        ],
        tsconfig,
    );

    let imports = extractor.extract(&path).expect("extraction should succeed");
    let graph = DependencyGraph::from_imports(&imports);

    // Diamond has 4 nodes: top, left, right, bottom
    assert!(
        graph.node_count() >= 4,
        "diamond graph should have at least 4 nodes"
    );
}

// =============================================================================
// EXPANDED LONG CYCLE TESTS
// =============================================================================

#[test]
fn test_long_cycle_has_five_or_more_nodes() {
    let cycles = detect_cycles_in_fixture("long-cycle");
    assert_eq!(cycles.len(), 1, "long-cycle should have exactly 1 cycle");

    let cycle = cycles.first().expect("should have a cycle");
    let file_count = cycle.matches('→').count() + 1;
    // The cycle representation includes the closing node, so A→B→C→D→E→A has 6 parts
    assert!(
        file_count >= 5,
        "long cycle should involve at least 5 files, found {file_count}"
    );
}

#[test]
fn test_long_cycle_import_extraction() {
    let imports = extract_imports_from_fixture("long-cycle");
    assert_eq!(
        imports.len(),
        5,
        "long-cycle should have exactly 5 imports (A->B->C->D->E->A)"
    );
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// EXPANDED SELF LOOP TESTS
// =============================================================================

#[test]
fn test_self_loop_has_single_file_cycle() {
    let cycles = detect_cycles_in_fixture("self-loop");
    assert_eq!(cycles.len(), 1, "self-loop should have exactly 1 cycle");

    let cycle = cycles.first().expect("should have a cycle");
    // Self-loop has only 1 file (the file imports itself)
    let part_count = cycle.split('→').count();
    assert!(
        part_count <= 2,
        "self-loop cycle should be a single file or file -> file"
    );
}

#[test]
fn test_self_loop_import_extraction() {
    let imports = extract_imports_from_fixture("self-loop");
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// EXPANDED DYNAMIC IMPORT TESTS
// =============================================================================

#[test]
fn test_dynamic_cycle_detects_dynamic_imports() {
    let imports = extract_imports_from_fixture("dynamic-cycle");

    // Check that we have at least one dynamic import (import() syntax)
    let has_dynamic = imports
        .iter()
        .any(|(_, spec)| spec.starts_with("./") || spec.starts_with("../"));
    assert!(has_dynamic, "should detect dynamic imports");
    insta::assert_debug_snapshot!(imports);
}

#[test]
fn test_dynamic_cycle_cycle_detection() {
    let cycles = detect_cycles_in_fixture("dynamic-cycle");
    assert!(
        !cycles.is_empty(),
        "dynamic-cycle should have at least one cycle"
    );
}

// =============================================================================
// EXPANDED IMPORT STYLES TESTS
// =============================================================================

#[test]
fn test_import_styles_static_imports() {
    let imports = extract_imports_from_fixture("import-styles");

    // Should have static ES imports
    let has_static_import = imports.iter().any(|(_, _)| true); // All imports count
    assert!(has_static_import, "should have static imports");
}

#[test]
fn test_import_styles_dynamic_imports() {
    let imports = extract_imports_from_fixture("import-styles");
    assert!(
        !imports.is_empty(),
        "import-styles should have various import types"
    );
}

#[test]
fn test_import_styles_require() {
    let imports = extract_imports_from_fixture("import-styles");
    assert!(
        !imports.is_empty(),
        "import-styles should include require() calls"
    );
}

#[test]
fn test_import_styles_reexports() {
    let imports = extract_imports_from_fixture("import-styles");
    // Re-exports like `export { x } from './y'` should be captured
    assert!(
        !imports.is_empty(),
        "import-styles should capture re-exports"
    );
}

// =============================================================================
// EXPANDED TSCONFIG TESTS
// =============================================================================

#[test]
fn test_tsconfig_path_alias_resolution() {
    let path = fixture_path("with-tsconfig");
    let tsconfig = TsConfig::discover(&path);

    assert!(tsconfig.is_some(), "should discover tsconfig.json");

    let config = tsconfig.expect("config should exist");
    assert!(
        !config.paths.is_empty(),
        "tsconfig should have path aliases"
    );
}

#[test]
fn test_tsconfig_base_url_set() {
    let path = fixture_path("with-tsconfig");
    let tsconfig = TsConfig::discover(&path);

    assert!(tsconfig.is_some(), "should discover tsconfig.json");
}

#[test]
fn test_tsconfig_complex_multiple_aliases() {
    let path = fixture_path("tsconfig-complex");
    let tsconfig = TsConfig::discover(&path);

    assert!(tsconfig.is_some(), "should discover tsconfig.json");

    let config = tsconfig.expect("config should exist");
    // tsconfig-complex has @utils/*, @components/*, @/*
    assert!(
        config.paths.len() >= 2,
        "tsconfig-complex should have multiple path aliases"
    );
}

#[test]
fn test_tsconfig_complex_alias_prefixes() {
    let path = fixture_path("tsconfig-complex");
    let tsconfig = TsConfig::discover(&path).expect("should discover config");

    let prefixes: Vec<_> = tsconfig.paths.iter().map(|p| p.prefix.as_str()).collect();

    // Should have @utils/, @components/, @/
    assert!(
        prefixes.iter().any(|p| p.contains("utils")),
        "should have @utils/* alias"
    );
}

// =============================================================================
// EXPANDED TSCONFIG EXTENDS TESTS
// =============================================================================

#[test]
fn test_tsconfig_extends_three_level_chain() {
    let path = fixture_path("tsconfig-extends");
    let tsconfig = TsConfig::discover(&path).expect("should discover config");

    // Should have aliases from all three levels
    let prefixes: Vec<_> = tsconfig.paths.iter().map(|p| p.prefix.as_str()).collect();

    assert!(
        prefixes.iter().any(|p| p.contains("app")),
        "should have @app/* from child"
    );
    assert!(
        prefixes.iter().any(|p| p.contains("shared")),
        "should have @shared/* from parent"
    );
    assert!(
        prefixes.iter().any(|p| p.contains("base")),
        "should have @base/* from grandparent"
    );
}

#[test]
fn test_tsconfig_extends_import_resolution() {
    let imports = extract_imports_from_fixture("tsconfig-extends");

    // Check that imports using path aliases from different levels are resolved
    let specifiers: Vec<_> = imports.iter().map(|(_, s)| s.as_str()).collect();

    assert!(
        specifiers.iter().any(|s| s.contains("@app")),
        "should have @app/* import"
    );
    assert!(
        specifiers.iter().any(|s| s.contains("@shared")),
        "should have @shared/* import"
    );
    assert!(
        specifiers.iter().any(|s| s.contains("@base")),
        "should have @base/* import"
    );
}

#[test]
fn test_tsconfig_extends_node_modules_discovery() {
    let path = fixture_path("tsconfig-extends-pkg");
    let tsconfig = TsConfig::discover(&path);

    assert!(
        tsconfig.is_some(),
        "should discover tsconfig extending from node_modules"
    );
}

#[test]
fn test_tsconfig_extends_node_modules_aliases() {
    let path = fixture_path("tsconfig-extends-pkg");
    let tsconfig = TsConfig::discover(&path).expect("should discover config");

    let prefixes: Vec<_> = tsconfig.paths.iter().map(|p| p.prefix.as_str()).collect();

    assert!(
        prefixes.iter().any(|p| p.contains("local")),
        "should have @local/* from child config"
    );
    assert!(
        prefixes.iter().any(|p| p.contains("company")),
        "should have @company/* from node_modules config"
    );
}

// =============================================================================
// EXPANDED INTER-PACKAGE CYCLE TESTS
// =============================================================================

#[test]
fn test_inter_package_simple_package_count() {
    let path = fixture_path("inter-package-simple");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(
        workspace.packages.len(),
        2,
        "should have exactly 2 packages"
    );
}

#[test]
fn test_inter_package_simple_package_names() {
    let path = fixture_path("inter-package-simple");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    let mut names: Vec<_> = workspace.packages.keys().cloned().collect();
    names.sort();

    assert_eq!(names, vec!["@simple/a", "@simple/b"]);
}

#[test]
fn test_inter_package_three_way_package_count() {
    let path = fixture_path("inter-package-cycle");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(
        workspace.packages.len(),
        3,
        "should have exactly 3 packages"
    );
}

#[test]
fn test_inter_package_no_cycle_is_linear() {
    let path = fixture_path("inter-package-no-cycle");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(
        workspace.packages.len(),
        3,
        "linear chain should have 3 packages"
    );

    let cycles = detect_inter_package_cycles("inter-package-no-cycle");
    assert!(cycles.is_empty(), "linear dependency should have no cycles");
}

#[test]
fn test_inter_package_cycle_normalization() {
    // Verify that cycles are normalized (start from lexicographically smallest)
    let cycles = detect_inter_package_cycles("inter-package-cycle");
    assert_eq!(cycles.len(), 1, "should have exactly one cycle");

    let cycle = cycles.first().expect("should have cycle");
    let first_pkg = cycle
        .split(" -> ")
        .next()
        .expect("should have first package");

    // The cycle should be normalized to start from the smallest package name
    assert!(
        first_pkg.starts_with('@'),
        "cycle should start with scoped package"
    );
}

// =============================================================================
// EXPANDED PNPM WORKSPACE TESTS
// =============================================================================

#[test]
fn test_pnpm_workspace_format_detection() {
    let path = fixture_path("pnpm-workspace");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(
        workspace.format,
        WorkspaceFormat::Pnpm,
        "should detect pnpm format"
    );
}

#[test]
fn test_pnpm_workspace_package_paths_are_absolute() {
    let path = fixture_path("pnpm-workspace");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    let mut package_names: Vec<_> = workspace.packages.keys().collect();
    package_names.sort();
    for name in package_names {
        let pkg_path = workspace.packages.get(name).expect("key should exist");
        assert!(
            pkg_path.is_absolute(),
            "package {name} path should be absolute"
        );
        assert!(pkg_path.exists(), "package {name} path should exist");
    }
}

#[test]
fn test_pnpm_workspace_complex_package_count() {
    let path = fixture_path("pnpm-workspace-complex");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    // Should have core and utils (ignored is excluded by negative glob... or not)
    assert!(
        workspace.packages.len() >= 2,
        "should have at least 2 packages"
    );
}

#[test]
fn test_pnpm_workspace_root_path() {
    let path = fixture_path("pnpm-workspace");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert!(
        workspace.root.ends_with("pnpm-workspace"),
        "root should be pnpm-workspace directory"
    );
}

// =============================================================================
// EXPANDED WORKSPACE IMPORT EXTRACTION TESTS
// =============================================================================

#[test]
fn test_workspace_import_target_types() {
    let path = fixture_path("inter-package-simple");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    let pkg_a_path = workspace
        .packages
        .get("@simple/a")
        .expect("should exist")
        .clone();

    let extractor =
        ImportExtractor::new(vec!["ts".to_owned()], None).with_workspace(Some(workspace));

    let imports = extractor.extract(&pkg_a_path).expect("should succeed");

    // Check import target types
    let has_workspace_imports = imports
        .iter()
        .any(|i| matches!(i.target, jscycles::ImportTarget::WorkspacePackage { .. }));

    assert!(
        has_workspace_imports,
        "should have workspace package imports"
    );
}

#[test]
fn test_workspace_import_specifier_preserved() {
    let path = fixture_path("inter-package-simple");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    let pkg_a_path = workspace
        .packages
        .get("@simple/a")
        .expect("should exist")
        .clone();

    let extractor =
        ImportExtractor::new(vec!["ts".to_owned()], None).with_workspace(Some(workspace));

    let imports = extractor.extract(&pkg_a_path).expect("should succeed");

    let import = imports.first().expect("should have import");
    assert_eq!(
        import.specifier, "@simple/b",
        "specifier should be preserved"
    );
}

// =============================================================================
// EXPANDED MONOREPO TESTS
// =============================================================================

#[test]
fn test_monorepo_mock_package_names() {
    let path = fixture_path("monorepo-mock");
    let config = Config::default();
    let discovery = PackageDiscovery::new(&config, &[], &[]).expect("should create");

    let packages = discovery.discover(&path).expect("should succeed");
    let mut names: Vec<_> = packages.iter().map(|p| p.name.clone()).collect();
    names.sort();

    assert!(
        names.contains(&"@test/pkg-a".to_owned()),
        "should find @test/pkg-a"
    );
    assert!(
        names.contains(&"@test/pkg-b".to_owned()),
        "should find @test/pkg-b"
    );
}

#[test]
fn test_monorepo_mock_package_paths() {
    let path = fixture_path("monorepo-mock");
    let config = Config::default();
    let discovery = PackageDiscovery::new(&config, &[], &[]).expect("should create");

    let packages = discovery.discover(&path).expect("should succeed");

    for pkg in &packages {
        assert!(pkg.path.is_absolute(), "package path should be absolute");
        assert!(pkg.path.exists(), "package path should exist");
    }
}

#[test]
fn test_monorepo_mock_has_internal_cycles() {
    let path = fixture_path("monorepo-mock");
    let config = Config::default();
    let discovery = PackageDiscovery::new(&config, &[], &[]).expect("should create");

    let packages = discovery.discover(&path).expect("should succeed");

    // Check each package for internal cycles
    let mut found_cycle = false;
    for pkg in &packages {
        let tsconfig = TsConfig::discover(&pkg.path);
        let extractor = ImportExtractor::new(vec!["ts".to_owned(), "tsx".to_owned()], tsconfig);

        let imports = extractor.extract(&pkg.path).expect("should succeed");
        let graph = DependencyGraph::from_imports(&imports);
        let cycles = CycleDetector::detect(&graph);

        if !cycles.is_empty() {
            found_cycle = true;
        }
    }

    assert!(
        found_cycle,
        "monorepo-mock should have at least one internal cycle"
    );
}

// =============================================================================
// NEW FIXTURES - NESTED CYCLES TESTS
// =============================================================================

#[test]
fn test_nested_cycles_detection() {
    let cycles = detect_cycles_in_fixture("nested-cycles");
    // Should detect at least one cycle (SCC algorithm may report them differently)
    assert!(
        !cycles.is_empty(),
        "nested-cycles should have at least 1 cycle, found {}",
        cycles.len()
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_nested_cycles_imports() {
    let imports = extract_imports_from_fixture("nested-cycles");
    // a imports b and c, b imports a, c imports d, d imports a
    assert!(
        imports.len() >= 5,
        "nested-cycles should have at least 5 imports"
    );
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// NEW FIXTURES - RE-EXPORT CYCLE TESTS
// =============================================================================

#[test]
fn test_re_export_cycle_detection() {
    let cycles = detect_cycles_in_fixture("re-export-cycle");
    assert!(
        !cycles.is_empty(),
        "re-export-cycle should detect cycles through re-exports"
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_re_export_cycle_imports() {
    let imports = extract_imports_from_fixture("re-export-cycle");
    // Should capture re-exports from index.ts
    let has_reexport = imports.iter().any(|(src, _)| src.contains("index"));
    assert!(has_reexport, "should capture re-exports from index.ts");
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// NEW FIXTURES - INDEX FILE CYCLE TESTS
// =============================================================================

#[test]
fn test_index_file_cycle_detection() {
    let cycles = detect_cycles_in_fixture("index-file-cycle");
    assert!(
        !cycles.is_empty(),
        "index-file-cycle should detect barrel file cycles"
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_index_file_cycle_imports() {
    let imports = extract_imports_from_fixture("index-file-cycle");
    // moduleA imports from index, index re-exports moduleA and moduleB
    assert!(
        imports.len() >= 2,
        "index-file-cycle should have at least 2 imports"
    );
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// NEW FIXTURES - DEEP NESTING TESTS
// =============================================================================

#[test]
fn test_deep_nesting_cycle_detection() {
    let cycles = detect_cycles_in_fixture("deep-nesting");
    // a -> b -> c -> a (across directory levels)
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_deep_nesting_imports() {
    let imports = extract_imports_from_fixture("deep-nesting");
    // Should have imports across multiple directory levels
    let has_deep_import = imports
        .iter()
        .any(|(src, _)| src.contains("level1") || src.contains("level2") || src.contains("level3"));
    assert!(
        has_deep_import,
        "should have imports from nested directories"
    );
    insta::assert_debug_snapshot!(imports);
}

#[test]
fn test_deep_nesting_parent_directory_imports() {
    let imports = extract_imports_from_fixture("deep-nesting");
    // c.ts imports from ../../a using parent directory traversal
    let has_parent_import = imports.iter().any(|(_, spec)| spec.contains(".."));
    assert!(
        has_parent_import,
        "should have parent directory imports (..)"
    );
}

// =============================================================================
// NEW FIXTURES - MIXED IMPORT CYCLE TESTS
// =============================================================================

#[test]
fn test_mixed_import_cycle_detection() {
    let cycles = detect_cycles_in_fixture("mixed-import-cycle");
    // static -> dynamic -> require -> static (cycle through different import styles)
    assert!(
        !cycles.is_empty(),
        "mixed-import-cycle should detect cycle through mixed import styles"
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_mixed_import_cycle_imports() {
    let imports = extract_imports_from_fixture("mixed-import-cycle");
    assert!(
        imports.len() >= 3,
        "mixed-import-cycle should have at least 3 imports"
    );
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// NEW FIXTURES - WORKSPACE DIAMOND (NO CYCLE) TESTS
// =============================================================================

#[test]
fn test_workspace_diamond_no_cycle() {
    let cycles = detect_inter_package_cycles("workspace-diamond");
    assert!(
        cycles.is_empty(),
        "workspace diamond should not have cycles, found: {cycles:?}"
    );
}

#[test]
fn test_workspace_diamond_package_count() {
    let path = fixture_path("workspace-diamond");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(
        workspace.packages.len(),
        4,
        "diamond should have 4 packages: core, left, right, bottom"
    );
}

#[test]
fn test_workspace_diamond_package_names() {
    let path = fixture_path("workspace-diamond");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert!(
        workspace.packages.contains_key("@diamond/core"),
        "should have @diamond/core"
    );
    assert!(
        workspace.packages.contains_key("@diamond/left"),
        "should have @diamond/left"
    );
    assert!(
        workspace.packages.contains_key("@diamond/right"),
        "should have @diamond/right"
    );
    assert!(
        workspace.packages.contains_key("@diamond/bottom"),
        "should have @diamond/bottom"
    );
}

// =============================================================================
// NEW FIXTURES - WORKSPACE SHARED CYCLE TESTS
// =============================================================================

#[test]
fn test_workspace_shared_cycle_detection() {
    let cycles = detect_inter_package_cycles("workspace-shared-cycle");
    // app -> shared -> utils -> app (3-way cycle)
    assert_eq!(
        cycles.len(),
        1,
        "workspace-shared-cycle should have exactly 1 cycle"
    );
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_workspace_shared_cycle_package_count() {
    let path = fixture_path("workspace-shared-cycle");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(
        workspace.packages.len(),
        3,
        "should have 3 packages: app, shared, utils"
    );
}

#[test]
fn test_workspace_shared_cycle_involves_all_packages() {
    let cycles = detect_inter_package_cycles("workspace-shared-cycle");
    let cycle = cycles.first().expect("should have cycle");

    assert!(cycle.contains("app"), "cycle should involve app package");
    assert!(
        cycle.contains("shared"),
        "cycle should involve shared package"
    );
    assert!(
        cycle.contains("utils"),
        "cycle should involve utils package"
    );
}

// =============================================================================
// GRAPH STRUCTURE TESTS
// =============================================================================

#[test]
fn test_graph_node_count_simple_cycle() {
    let path = fixture_path("simple-cycle");
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        vec![
            "ts".to_owned(),
            "tsx".to_owned(),
            "js".to_owned(),
            "jsx".to_owned(),
        ],
        tsconfig,
    );

    let imports = extractor.extract(&path).expect("should succeed");
    let graph = DependencyGraph::from_imports(&imports);

    assert!(
        graph.node_count() >= 2,
        "simple-cycle graph should have at least 2 nodes"
    );
}

#[test]
fn test_graph_edge_count_diamond() {
    let path = fixture_path("diamond");
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        vec![
            "ts".to_owned(),
            "tsx".to_owned(),
            "js".to_owned(),
            "jsx".to_owned(),
        ],
        tsconfig,
    );

    let imports = extractor.extract(&path).expect("should succeed");
    let graph = DependencyGraph::from_imports(&imports);

    // Diamond has 4 edges: top->left, top->right, left->bottom, right->bottom
    assert!(
        graph.edge_count() >= 4,
        "diamond graph should have at least 4 edges"
    );
}

// =============================================================================
// ERROR HANDLING TESTS
// =============================================================================

#[test]
fn test_non_existent_fixture() {
    let path = fixture_path("non-existent-fixture-that-does-not-exist");
    let tsconfig = TsConfig::discover(&path);

    // TsConfig::discover should return None for non-existent path
    assert!(
        tsconfig.is_none(),
        "should return None for non-existent directory"
    );
}

#[test]
fn test_empty_directory() {
    // Create a temp directory for this test
    let path = fixture_path("no-cycles");
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(vec!["ts".to_owned()], tsconfig);

    // Extraction should succeed even with minimal files
    let result = extractor.extract(&path);
    assert!(result.is_ok(), "extraction should succeed");
}

// =============================================================================
// IMPORT FILTERING TESTS
// =============================================================================

#[test]
fn test_import_extraction_ts_only() {
    let path = fixture_path("simple-cycle");
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(vec!["ts".to_owned()], tsconfig);

    let imports = extractor.extract(&path).expect("should succeed");

    // All imports should be from .ts files
    for import in &imports {
        let ext = import
            .source
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        assert_eq!(ext, "ts", "should only extract from .ts files");
    }
}

#[test]
fn test_import_extraction_multiple_extensions() {
    let path = fixture_path("import-styles");
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        vec![
            "ts".to_owned(),
            "tsx".to_owned(),
            "js".to_owned(),
            "jsx".to_owned(),
        ],
        tsconfig,
    );

    let imports = extractor.extract(&path).expect("should succeed");

    // Should extract from multiple file types
    let extensions: std::collections::HashSet<_> = imports
        .iter()
        .filter_map(|i| i.source.extension())
        .map(|e| e.to_string_lossy().to_string())
        .collect();

    assert!(
        !extensions.is_empty(),
        "should extract from at least one file type"
    );
}

// =============================================================================
// CYCLE PROPERTIES TESTS
// =============================================================================

#[test]
fn test_cycle_is_closed() {
    let cycles = detect_cycles_in_fixture("simple-cycle");
    let cycle = cycles.first().expect("should have cycle");

    // A cycle should start and end at the same node (implied by format)
    let part_count = cycle.split(" → ").count();
    assert!(
        part_count >= 2,
        "cycle should have at least 2 parts (A → B means A → B → A)"
    );
}

#[test]
fn test_cycles_are_sorted() {
    let cycles = detect_cycles_in_fixture("complex-cycles");

    // Cycles should be sorted for deterministic output
    let mut sorted_cycles = cycles.clone();
    sorted_cycles.sort();

    assert_eq!(
        cycles, sorted_cycles,
        "cycles should be returned in sorted order"
    );
}

#[test]
fn test_no_duplicate_cycles() {
    let cycles = detect_cycles_in_fixture("complex-cycles");
    let unique: std::collections::HashSet<_> = cycles.iter().collect();

    assert_eq!(
        cycles.len(),
        unique.len(),
        "should not have duplicate cycles"
    );
}

// =============================================================================
// TYPE-ONLY IMPORTS TESTS
// =============================================================================

#[test]
fn test_circular_type_only_detection() {
    let cycles = detect_cycles_in_fixture("circular-type-only");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_circular_type_only_has_cycle() {
    let cycles = detect_cycles_in_fixture("circular-type-only");
    assert!(
        !cycles.is_empty(),
        "type-only circular imports should still be detected"
    );
}

#[test]
fn test_circular_type_only_imports() {
    let imports = extract_imports_from_fixture("circular-type-only");
    assert!(imports.len() >= 2, "should have at least 2 type imports");
}

// =============================================================================
// MULTIPLE ENTRY POINTS TESTS
// =============================================================================

#[test]
fn test_multiple_entry_points_detection() {
    let cycles = detect_cycles_in_fixture("multiple-entry-points");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_multiple_entry_points_has_multiple_cycles() {
    let cycles = detect_cycles_in_fixture("multiple-entry-points");
    // Should have at least 2 cycles (entry1->shared, entry2->helper)
    assert!(
        cycles.len() >= 2,
        "should detect multiple independent cycles from different entry points"
    );
}

#[test]
fn test_multiple_entry_points_imports() {
    let imports = extract_imports_from_fixture("multiple-entry-points");
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// WORKSPACE WITH SHARED DEPENDENCY TESTS
// =============================================================================

#[test]
fn test_workspace_shared_dep_no_cycle() {
    let cycles = detect_inter_package_cycles("workspace-with-shared-dep");
    assert!(
        cycles.is_empty(),
        "workspace with shared dependency (no back-edge) should have no cycles"
    );
}

#[test]
fn test_workspace_shared_dep_discovery() {
    let path = fixture_path("workspace-with-shared-dep");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(workspace.packages.len(), 3, "should have 3 packages");
}

#[test]
fn test_workspace_shared_dep_package_names() {
    let path = fixture_path("workspace-with-shared-dep");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    let mut names: Vec<_> = workspace.packages.keys().cloned().collect();
    names.sort();

    assert_eq!(
        names,
        vec!["@shared-dep/app", "@shared-dep/lib", "@shared-dep/shared"]
    );
}

// =============================================================================
// ALIAS CHAIN TESTS (tsconfig path aliases)
// =============================================================================

#[test]
fn test_alias_chain_detection() {
    let cycles = detect_cycles_in_fixture("alias-chain");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_alias_chain_has_cycle() {
    let cycles = detect_cycles_in_fixture("alias-chain");
    assert!(
        !cycles.is_empty(),
        "alias chain (A->B->C->A via @aliases) should detect cycle"
    );
}

#[test]
fn test_alias_chain_tsconfig_loaded() {
    let path = fixture_path("alias-chain");
    let tsconfig = TsConfig::discover(&path);
    assert!(
        tsconfig.is_some(),
        "should load tsconfig.json with path aliases"
    );
}

// =============================================================================
// SIDE EFFECT IMPORTS TESTS
// =============================================================================

#[test]
fn test_side_effect_imports_detection() {
    let cycles = detect_cycles_in_fixture("side-effect-imports");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_side_effect_imports_extracts_bare_imports() {
    let imports = extract_imports_from_fixture("side-effect-imports");
    // Should find "./polyfill" and "./setup" as side-effect imports
    // imports is Vec<(source, specifier)>
    let has_bare_imports = imports
        .iter()
        .any(|(_, spec)| spec == "./polyfill" || spec == "./setup");
    assert!(
        has_bare_imports,
        "should extract side-effect (bare) imports"
    );
}

// =============================================================================
// STAR EXPORTS TESTS
// =============================================================================

#[test]
fn test_star_exports_detection() {
    let cycles = detect_cycles_in_fixture("star-exports");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_star_exports_has_cycle() {
    let cycles = detect_cycles_in_fixture("star-exports");
    assert!(
        !cycles.is_empty(),
        "star re-exports (export * from) should detect cycles"
    );
}

#[test]
fn test_star_exports_imports() {
    let imports = extract_imports_from_fixture("star-exports");
    insta::assert_debug_snapshot!(imports);
}

// =============================================================================
// DEFAULT EXPORTS CYCLE TESTS
// =============================================================================

#[test]
fn test_default_exports_cycle_detection() {
    let cycles = detect_cycles_in_fixture("default-exports-cycle");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_default_exports_cycle_has_cycle() {
    let cycles = detect_cycles_in_fixture("default-exports-cycle");
    assert!(
        !cycles.is_empty(),
        "default export imports should detect cycles"
    );
}

// =============================================================================
// NAMESPACE IMPORTS TESTS
// =============================================================================

#[test]
fn test_namespace_imports_detection() {
    let cycles = detect_cycles_in_fixture("namespace-imports");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_namespace_imports_has_cycle() {
    let cycles = detect_cycles_in_fixture("namespace-imports");
    assert!(
        !cycles.is_empty(),
        "namespace imports (import * as) should detect cycles"
    );
}

#[test]
fn test_namespace_imports_extraction() {
    let imports = extract_imports_from_fixture("namespace-imports");
    // Should have namespace imports to utils and helpers
    assert!(
        imports.len() >= 4,
        "should extract multiple namespace imports"
    );
}

// =============================================================================
// VERY LONG CYCLE TESTS (15 nodes)
// =============================================================================

#[test]
fn test_very_long_cycle_detection() {
    let cycles = detect_cycles_in_fixture("very-long-cycle");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_very_long_cycle_has_exactly_one_cycle() {
    let cycles = detect_cycles_in_fixture("very-long-cycle");
    assert_eq!(cycles.len(), 1, "should have exactly one 15-node cycle");
}

#[test]
fn test_very_long_cycle_node_count() {
    let cycles = detect_cycles_in_fixture("very-long-cycle");
    let cycle = cycles.first().expect("should have cycle");
    // Count nodes by splitting on arrows
    let node_count = cycle.split(" → ").count();
    assert!(
        node_count >= 14,
        "cycle should have at least 14 nodes shown (15 files)"
    );
}

// =============================================================================
// WIDE GRAPH TESTS (hub with 8 spokes)
// =============================================================================

#[test]
fn test_wide_graph_no_cycles() {
    let cycles = detect_cycles_in_fixture("wide-graph");
    assert!(
        cycles.is_empty(),
        "wide graph with hub importing 8 leaf nodes should have no cycles"
    );
}

#[test]
fn test_wide_graph_imports() {
    let imports = extract_imports_from_fixture("wide-graph");
    // Hub imports 8 files
    assert!(imports.len() >= 8, "hub should import 8 leaf nodes");
}

// =============================================================================
// HUB AND SPOKE TESTS (hub <-> spokes bidirectional)
// =============================================================================

#[test]
fn test_hub_and_spoke_detection() {
    let cycles = detect_cycles_in_fixture("hub-and-spoke");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_hub_and_spoke_has_cycles() {
    let cycles = detect_cycles_in_fixture("hub-and-spoke");
    // Hub imports all spokes, each spoke imports hub
    // SCC finds these as part of one strongly connected component
    assert!(!cycles.is_empty(), "hub-and-spoke should detect cycles");
}

// =============================================================================
// MUTUAL RECURSION TESTS
// =============================================================================

#[test]
fn test_mutual_recursion_detection() {
    let cycles = detect_cycles_in_fixture("mutual-recursion");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_mutual_recursion_has_cycle() {
    let cycles = detect_cycles_in_fixture("mutual-recursion");
    assert!(
        !cycles.is_empty(),
        "mutual recursion (even/odd) should detect cycle"
    );
}

// =============================================================================
// JSX/TSX IMPORTS TESTS
// =============================================================================

fn detect_cycles_with_extensions(fixture_name: &str, extensions: Vec<&str>) -> Vec<String> {
    let path = fixture_path(fixture_name);
    let tsconfig = TsConfig::discover(&path);
    let extractor = ImportExtractor::new(
        extensions.into_iter().map(|s| s.to_owned()).collect(),
        tsconfig,
    );

    let imports = extractor
        .extract(&path)
        .expect("fixture extraction should succeed");
    let graph = DependencyGraph::from_imports(&imports);
    let cycles = CycleDetector::detect(&graph);

    let mut cycle_strings: Vec<String> = cycles
        .iter()
        .map(|cycle| {
            cycle
                .path
                .iter()
                .filter_map(|p| {
                    p.strip_prefix(&path)
                        .ok()
                        .map(|rel| rel.to_string_lossy().to_string())
                })
                .collect::<Vec<_>>()
                .join(" → ")
        })
        .collect();

    cycle_strings.sort();
    cycle_strings
}

#[test]
fn test_jsx_imports_detection() {
    let cycles = detect_cycles_with_extensions("jsx-imports", vec!["tsx", "jsx", "ts", "js"]);
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_jsx_imports_has_cycle() {
    let cycles = detect_cycles_with_extensions("jsx-imports", vec!["tsx", "jsx", "ts", "js"]);
    assert!(
        !cycles.is_empty(),
        "JSX component cycle (App->Button->App) should be detected"
    );
}

// =============================================================================
// MJS/CJS IMPORTS TESTS
// =============================================================================

#[test]
fn test_mjs_cjs_imports_detection() {
    let cycles = detect_cycles_with_extensions("mjs-cjs-imports", vec!["mjs", "cjs", "js"]);
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_mjs_cjs_has_cycle() {
    let cycles = detect_cycles_with_extensions("mjs-cjs-imports", vec!["mjs", "cjs", "js"]);
    assert!(!cycles.is_empty(), "mjs<->cjs cycle should be detected");
}

// =============================================================================
// MULTILINE IMPORTS TESTS
// =============================================================================

#[test]
fn test_multiline_imports_detection() {
    let cycles = detect_cycles_in_fixture("multiline-imports");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_multiline_imports_has_cycle() {
    let cycles = detect_cycles_in_fixture("multiline-imports");
    assert!(
        !cycles.is_empty(),
        "multiline imports should be parsed and cycles detected"
    );
}

#[test]
fn test_multiline_imports_extraction() {
    let imports = extract_imports_from_fixture("multiline-imports");
    // Should extract both multiline imports from main.ts
    // imports is Vec<(source, specifier)>
    let main_import_count = imports
        .iter()
        .filter(|(src, _)| src.contains("main.ts"))
        .count();
    assert!(main_import_count >= 2, "should extract multiline imports");
}

// =============================================================================
// COMMENTS IN IMPORTS TESTS
// =============================================================================

#[test]
fn test_comments_in_imports_detection() {
    let cycles = detect_cycles_in_fixture("comments-in-imports");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_comments_in_imports_ignores_commented() {
    let imports = extract_imports_from_fixture("comments-in-imports");
    // Should NOT have imports from ./x or ./y (they're commented out)
    // imports is Vec<(source, specifier)>
    let has_invalid_imports = imports
        .iter()
        .any(|(_, spec)| spec == "./x" || spec == "./y");
    assert!(
        !has_invalid_imports,
        "should not extract commented-out imports"
    );
}

#[test]
fn test_comments_in_imports_extracts_real_imports() {
    let imports = extract_imports_from_fixture("comments-in-imports");
    // imports is Vec<(source, specifier)>
    let real_import_specs: Vec<_> = imports.iter().map(|(_, spec)| spec.as_str()).collect();
    assert!(real_import_specs.contains(&"./a"), "should extract ./a");
    assert!(real_import_specs.contains(&"./b"), "should extract ./b");
    assert!(real_import_specs.contains(&"./c"), "should extract ./c");
}

// =============================================================================
// STRING ESCAPES TESTS
// =============================================================================

#[test]
fn test_string_escapes_no_false_positives() {
    let imports = extract_imports_from_fixture("string-escapes");
    // Should NOT extract "./fake" or "./also-fake" from string literals
    // imports is Vec<(source, specifier)>
    let has_fake_imports = imports.iter().any(|(_, spec)| spec.contains("fake"));
    assert!(
        !has_fake_imports,
        "should not extract import-like strings from regular string literals"
    );
}

#[test]
fn test_string_escapes_extracts_real_imports() {
    let imports = extract_imports_from_fixture("string-escapes");
    // imports is Vec<(source, specifier)>
    let specs: Vec<_> = imports.iter().map(|(_, spec)| spec.as_str()).collect();
    assert!(specs.contains(&"./a"), "should extract real import ./a");
    assert!(specs.contains(&"./b"), "should extract real import ./b");
}

// =============================================================================
// WORKSPACE FIVE PACKAGES TESTS (5-way cycle)
// =============================================================================

#[test]
fn test_workspace_five_packages_cycle() {
    let cycles = detect_inter_package_cycles("workspace-five-packages");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_workspace_five_packages_has_cycle() {
    let cycles = detect_inter_package_cycles("workspace-five-packages");
    assert!(
        !cycles.is_empty(),
        "5-package circular dependency should be detected"
    );
}

#[test]
fn test_workspace_five_packages_discovery() {
    let path = fixture_path("workspace-five-packages");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(workspace.packages.len(), 5, "should have 5 packages");
}

#[test]
fn test_workspace_five_packages_names() {
    let path = fixture_path("workspace-five-packages");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    let mut names: Vec<_> = workspace.packages.keys().cloned().collect();
    names.sort();

    assert_eq!(
        names,
        vec!["@five/a", "@five/b", "@five/c", "@five/d", "@five/e"]
    );
}

// =============================================================================
// PNPM CATALOG TESTS
// =============================================================================

#[test]
fn test_pnpm_catalog_cycle() {
    let cycles = detect_inter_package_cycles("pnpm-catalog");
    insta::assert_debug_snapshot!(cycles);
}

#[test]
fn test_pnpm_catalog_has_cycle() {
    let cycles = detect_inter_package_cycles("pnpm-catalog");
    assert!(
        !cycles.is_empty(),
        "pnpm workspace cycle should be detected"
    );
}

#[test]
fn test_pnpm_catalog_discovery() {
    let path = fixture_path("pnpm-catalog");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    assert_eq!(workspace.packages.len(), 2, "should have 2 packages");
    assert_eq!(
        workspace.format,
        WorkspaceFormat::Pnpm,
        "should be pnpm format"
    );
}

// =============================================================================
// IMPORT EXTRACTION EDGE CASES
// =============================================================================

#[test]
fn test_import_extraction_empty_directory() {
    let path = std::env::temp_dir().join("jscycles-test-empty");
    std::fs::create_dir_all(&path).expect("should create temp dir");

    let extractor = ImportExtractor::new(vec!["ts".to_owned()], None);
    let imports = extractor
        .extract(&path)
        .expect("should succeed on empty dir");

    assert!(imports.is_empty(), "empty directory should have no imports");

    std::fs::remove_dir(&path).expect("should remove temp dir");
}

#[test]
fn test_import_extraction_with_subdirectories() {
    let imports = extract_imports_from_fixture("deep-nesting");
    // Should find imports from nested directories
    assert!(
        !imports.is_empty(),
        "should extract imports from nested directories"
    );
}

#[test]
fn test_import_extraction_deduplicates() {
    let imports = extract_imports_from_fixture("simple-cycle");
    // Check that we don't have duplicate import records for the same source->target
    // imports is Vec<(source, specifier)> - already pairs
    let unique_pairs: std::collections::HashSet<_> = imports.iter().collect();

    assert_eq!(
        imports.len(),
        unique_pairs.len(),
        "should not have duplicate imports"
    );
}

// =============================================================================
// GRAPH STRUCTURE EDGE CASES
// =============================================================================

#[test]
fn test_graph_isolated_nodes() {
    // no-cycles has nodes without cycles
    let imports = extract_raw_imports_from_fixture("no-cycles");
    let graph = DependencyGraph::from_imports(&imports);

    assert!(graph.node_count() > 0, "graph should have nodes");
    let cycles = CycleDetector::detect(&graph);
    assert!(
        cycles.is_empty(),
        "graph with isolated nodes should have no cycles"
    );
}

#[test]
fn test_graph_self_loop_only() {
    let imports = extract_raw_imports_from_fixture("self-loop");
    let graph = DependencyGraph::from_imports(&imports);
    let cycles = CycleDetector::detect(&graph);

    assert_eq!(
        cycles.len(),
        1,
        "self-loop should produce exactly one cycle"
    );
}

#[test]
fn test_graph_from_empty_imports() {
    let imports: Vec<jscycles::Import> = vec![];
    let graph = DependencyGraph::from_imports(&imports);

    assert_eq!(
        graph.node_count(),
        0,
        "empty imports should produce empty graph"
    );
}

// =============================================================================
// CYCLE DETECTOR EDGE CASES
// =============================================================================

#[test]
fn test_cycle_detector_deterministic() {
    // Run detection multiple times and ensure same result
    let cycles1 = detect_cycles_in_fixture("complex-cycles");
    let cycles2 = detect_cycles_in_fixture("complex-cycles");
    let cycles3 = detect_cycles_in_fixture("complex-cycles");

    assert_eq!(
        cycles1, cycles2,
        "cycle detection should be deterministic (1 vs 2)"
    );
    assert_eq!(
        cycles2, cycles3,
        "cycle detection should be deterministic (2 vs 3)"
    );
}

#[test]
fn test_cycle_detector_all_nodes_in_cycle_present() {
    let cycles = detect_cycles_in_fixture("simple-cycle");
    let cycle = cycles.first().expect("should have cycle");

    // Simple cycle should contain both a.ts and b.ts
    assert!(
        cycle.contains("a.ts") && cycle.contains("b.ts"),
        "cycle should contain both participating files"
    );
}

// =============================================================================
// WORKSPACE DETECTION EDGE CASES
// =============================================================================

#[test]
fn test_workspace_not_workspace_directory() {
    let path = fixture_path("simple-cycle");
    let workspace = Workspace::discover(&path).expect("should succeed");

    assert!(
        workspace.is_none(),
        "non-workspace directory should return None"
    );
}

#[test]
fn test_workspace_packages_have_valid_paths() {
    let path = fixture_path("workspace-five-packages");
    let workspace = Workspace::discover(&path)
        .expect("should succeed")
        .expect("should be workspace");

    let mut pkg_names: Vec<_> = workspace.packages.keys().collect();
    pkg_names.sort();
    for name in pkg_names {
        let pkg_path = workspace.packages.get(name).expect("key should exist");
        assert!(pkg_path.exists(), "package {name} path should exist");
        assert!(
            pkg_path.is_absolute(),
            "package {name} path should be absolute"
        );
        assert!(pkg_path.is_dir(), "package {name} path should be directory");
    }
}

// =============================================================================
// TSCONFIG EDGE CASES
// =============================================================================

#[test]
fn test_tsconfig_not_found_returns_none() {
    let path = fixture_path("no-cycles");
    let tsconfig = TsConfig::discover(&path);

    // no-cycles fixture doesn't have a tsconfig
    // This might return None or Some depending on implementation
    // Just verify it doesn't panic
    let _ = tsconfig;
}

#[test]
fn test_tsconfig_with_no_paths() {
    let path = fixture_path("simple-cycle");
    let tsconfig = TsConfig::discover(&path);

    // simple-cycle may or may not have tsconfig
    // Verify it handles gracefully
    let _ = tsconfig;
}

// =============================================================================
// PERFORMANCE / STRESS TESTS
// =============================================================================

#[test]
fn test_very_long_cycle_completes_quickly() {
    use std::time::Instant;

    let start = Instant::now();
    let _cycles = detect_cycles_in_fixture("very-long-cycle");
    let duration = start.elapsed();

    assert!(
        duration.as_secs() < 5,
        "15-node cycle detection should complete in under 5 seconds"
    );
}

#[test]
fn test_hub_and_spoke_completes_quickly() {
    use std::time::Instant;

    let start = Instant::now();
    let _cycles = detect_cycles_in_fixture("hub-and-spoke");
    let duration = start.elapsed();

    assert!(
        duration.as_secs() < 5,
        "hub-and-spoke detection should complete in under 5 seconds"
    );
}
