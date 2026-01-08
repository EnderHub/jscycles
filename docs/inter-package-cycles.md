# Inter-Package Cycle Detection

## Overview

jscycles now detects two types of circular dependencies:

1. **Inner cycles** - File-level cycles within a single package (original functionality)
2. **Outer cycles** - Package-level cycles between workspace packages (new)

## Architecture

### Workspace Detection (`src/workspace.rs`)

`Workspace::discover(path)` searches upward for workspace config files in this priority:

1. `pnpm-workspace.yaml` - parses `packages` array of globs
2. `package.json` with `workspaces` field - array of globs or `{packages: [...]}` object
3. `tsconfig.json` with `references` field - array of `{path: "..."}` objects

Returns `Option<Workspace>` containing:
- `root: PathBuf` - workspace root directory
- `format: WorkspaceFormat` - which format was detected
- `packages: HashMap<String, PathBuf>` - package name to path mapping

Key methods:
- `is_workspace_package(specifier)` - checks if import specifier is a workspace package
- `extract_package_name(specifier)` - extracts package name from `@scope/pkg/subpath`

### Import Target Extension (`src/imports.rs`)

`ImportTarget` enum has new variant:

```rust
WorkspacePackage {
    package_name: String,      // e.g., "@myorg/utils"
    subpath: Option<String>,   // e.g., "helpers" from "@myorg/utils/helpers"
}
```

`ImportExtractor` has new field and method:
- `workspace: Option<Workspace>`
- `with_workspace(Option<Workspace>)` - builder method

In `resolve_import()`, workspace packages are identified before falling back to `External`.

### Package Dependency Graph (`src/graph.rs`)

`PackageDependencyGraph` mirrors `DependencyGraph` but for packages:
- Nodes are package names (`String`)
- Edges are inter-package imports
- `from_imports(HashMap<String, Vec<Import>>)` builds graph from all packages' imports
- Self-edges are ignored (a package importing itself)

### Package Cycle Detection (`src/cycles.rs`)

`PackageCycle` struct holds `packages: Vec<String>` (the cycle path).

`PackageCycleDetector::detect(graph)` runs Tarjan's SCC algorithm on the package graph (same algorithm as file-level, just operating on `String` nodes instead of `PathBuf`).

### Unified Output (`src/output.rs`)

```rust
pub enum CycleFilter { All, Inner, Outer }

pub struct UnifiedResults {
    pub file_results: Vec<PackageResult>,  // existing file-level results
    pub package_cycles: Vec<PackageCycle>, // new package-level cycles
}
```

`UnifiedOutputFormatter` trait with `format_unified()` method implemented for both `HumanFormatter` and `JsonFormatter`.

### CLI Integration (`src/main.rs`)

New flags `--inner` and `--outer` (mutually exclusive).

Flow:
1. `Workspace::discover()` at startup
2. Pass workspace to each `ImportExtractor` via `with_workspace()`
3. Collect all imports by package name
4. Build `PackageDependencyGraph::from_imports()`
5. Run `PackageCycleDetector::detect()`
6. Output via `UnifiedResults` with `CycleFilter`

## tsconfig `extends` Support (`src/tsconfig.rs`)

`TsConfig::load()` now:
1. Parses `extends` field from JSON
2. Recursively loads parent config via `load_merged_options()`
3. Merges `compilerOptions` - child overrides parent for `baseUrl`, paths are merged
4. Resolves extends paths: relative (`./base.json`), node_modules (`@tsconfig/node18`)
5. Max depth of 10 to prevent infinite loops

New error variants in `TsConfigError`:
- `ExtendsNotFound(String)`
- `ExtendsDepth`

## Testing

Existing integration tests in `tests/integration.rs` cover monorepo discovery. The `test_monorepo_per_package_cycles` test verifies per-package cycle detection works.

Unit tests exist in each module for the new functionality.
