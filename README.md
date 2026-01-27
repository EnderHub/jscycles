# jscycles

Fast circular dependency detection for JavaScript/TypeScript projects.

A drop-in replacement for `madge --circular` with **50-100x performance improvement**.

## Why jscycles?

Circular dependencies in JavaScript/TypeScript projects cause:
- Runtime errors from undefined imports
- Difficult-to-debug initialization order issues
- Webpack/bundler warnings and failed builds
- Increased cognitive load when understanding code flow

Tools like `madge` detect these cycles but become painfully slow on large monorepos. jscycles uses Rust, parallel processing, and Tarjan's strongly connected components algorithm to analyze thousands of files in milliseconds.

## Performance

| Project Size | madge --circular | jscycles |
|--------------|------------------|----------|
| 100 files    | ~2s              | ~20ms    |
| 1,000 files  | ~15s             | ~100ms   |
| 10,000 files | ~3min            | ~2s      |

## Installation

### From source

```bash
cargo install --path .
```

## Quick Start

```bash
# Check current directory for cycles
jscycles

# Check specific directory
jscycles src/

# Output as JSON for CI integration
jscycles --json

# Only check specific packages in a monorepo
jscycles --only "@myorg/core-*"

# Show only inter-package cycles (outer cycles)
jscycles --outer

# Show only file-level cycles (inner cycles)
jscycles --inner
```

## Usage

```
jscycles [OPTIONS] [PATHS]...

Arguments:
  [PATHS]...  Paths to check (defaults to current directory)

Options:
      --only <PATTERN>       Only check packages matching glob (repeatable)
      --exclude <PATTERN>    Exclude packages matching glob (repeatable)
      --extensions <EXT>     File extensions to analyze [default: ts,tsx,js,jsx]
      --tsconfig <PATH>      Path to tsconfig.json
      --no-tsconfig          Skip tsconfig.json auto-detection
      --json                 Output as JSON
      --stdin                Read paths from stdin
      --inner                Show only file-level (inner) cycles
      --outer                Show only package-level (outer) cycles
  -c, --config <PATH>        Config file path [default: jscycles.yaml]
  -q, --quiet                Only output if cycles found
  -h, --help                 Print help
  -V, --version              Print version
```

## Examples

### Basic usage

```bash
# Check a single package
jscycles packages/core

# Check multiple directories
jscycles packages/core packages/utils packages/ui
```

### Monorepo filtering

```bash
# Only check packages matching a pattern
jscycles --only "packages/feature-*"

# Exclude test packages
jscycles --exclude "*-test" --exclude "*-e2e"

# Combine filters
jscycles --only "@myorg/*" --exclude "@myorg/legacy-*"
```

### Inter-package cycle detection

jscycles detects two types of cycles:

- **Inner cycles**: File-level circular dependencies within a package
- **Outer cycles**: Package-level circular dependencies between workspace packages

```bash
# Show both cycle types (default)
jscycles

# Show only file-level cycles
jscycles --inner

# Show only package-level cycles
jscycles --outer
```

Workspace packages are automatically detected from:
- npm/yarn/bun: `package.json` workspaces field
- pnpm: `pnpm-workspace.yaml`
- TypeScript: `tsconfig.json` project references

### CI/CD Integration

```bash
# JSON output for parsing
jscycles --json > cycles.json

# Quiet mode - only output if cycles exist (for CI gates)
jscycles --quiet || echo "Circular dependencies detected!"

# Check specific packages from a file list
cat changed-packages.txt | jscycles --stdin
```

### Custom file extensions

```bash
# Only TypeScript files
jscycles --extensions ts,tsx

# Include additional extensions
jscycles --extensions ts,tsx,js,jsx,mjs,cjs
```

## Configuration

Create a `jscycles.yaml` in your project root for persistent configuration:

```yaml
# Package discovery settings
scan:
  # Glob patterns for directories containing packages
  include:
    - "packages/*"
    - "apps/*"
    - "libs/*"

  # Directories to skip (defaults shown)
  exclude:
    - "**/node_modules"
    - "**/dist"
    - "**/build"

# Default settings for all packages
defaults:
  # File extensions to analyze
  extensions:
    - ts
    - tsx
    - js
    - jsx

# Package-specific overrides (keyed by glob pattern)
packages:
  # Legacy packages might use different extensions
  "@myorg/legacy-*":
    extensions:
      - js
    ignore:
      - "**/*.test.js"

  # Ignore test files in all feature packages
  "@myorg/feature-*":
    ignore:
      - "**/*.test.ts"
      - "**/*.spec.ts"
      - "**/__tests__/**"
      - "**/__mocks__/**"
```

### Configuration precedence

1. CLI arguments (highest priority)
2. Package-specific config in `jscycles.yaml`
3. Default config in `jscycles.yaml`
4. Built-in defaults (lowest priority)

## TypeScript Path Aliases

jscycles automatically discovers and uses `tsconfig.json` for path alias resolution:

```json
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"],
      "@components/*": ["src/components/*"],
      "@utils": ["src/utils/index.ts"]
    }
  }
}
```

With this config, imports like `@/services/api` resolve to `src/services/api.ts`.

### tsconfig options

```bash
# Use a specific tsconfig
jscycles --tsconfig tsconfig.build.json

# Disable tsconfig discovery (treat path aliases as external)
jscycles --no-tsconfig
```

### Resolution behavior

1. **Auto-discovery**: Searches upward from each package for `tsconfig.json`
2. **Extension resolution**: Tries `.ts`, `.tsx`, `.js`, `.jsx` in order
3. **Index files**: Resolves `@/utils` to `src/utils/index.ts` if directory
4. **Graceful fallback**: Unresolvable aliases are treated as external modules

## Output Formats

### Human-readable (default)

```
=== File-level cycles ===
✓ @myorg/utils: no cycles
✗ @myorg/core: 2 cycles
  components/Button.tsx → hooks/useTheme.ts → components/Button.tsx
  services/api.ts → services/auth.ts → services/api.ts
✓ @myorg/ui: no cycles

=== Package-level cycles ===
✗ @myorg/core → @myorg/utils → @myorg/core

Summary: 3 packages, 2 file cycles, 1 package cycles
```

With color support in TTY:
- Green checkmark for clean packages
- Red X for packages with cycles

### JSON output

```bash
jscycles --json
```

```json
{
  "has_cycles": true,
  "file_cycles": {
    "packages": [
      {
        "name": "@myorg/utils",
        "path": "packages/utils",
        "cycles": []
      },
      {
        "name": "@myorg/core",
        "path": "packages/core",
        "cycles": [
          ["components/Button.tsx", "hooks/useTheme.ts", "components/Button.tsx"],
          ["services/api.ts", "services/auth.ts", "services/api.ts"]
        ]
      }
    ]
  },
  "package_cycles": [
    ["@myorg/core", "@myorg/utils", "@myorg/core"]
  ]
}
```

## How It Works

### Algorithm

jscycles uses **Tarjan's strongly connected components (SCC)** algorithm:

1. **Parse**: Extract imports from all files using ast-grep
2. **Build graph**: Create a directed graph of file dependencies
3. **Find SCCs**: Run Tarjan's algorithm to find strongly connected components
4. **Extract cycles**: SCCs with >1 node contain cycles; single nodes are checked for self-loops
5. **Normalize**: Cycles are normalized to start from the lexicographically smallest file

### Import extraction

Supported import syntaxes:
- `import x from './module'`
- `import { x } from './module'`
- `import * as x from './module'`
- `import './module'` (side-effect imports)
- `export { x } from './module'`
- `export * from './module'`
- Dynamic `import('./module')`
- `require('./module')` (CommonJS)

### What's considered a dependency

| Import | Classification |
|--------|----------------|
| `./relative/path` | Resolved to file path |
| `../parent/path` | Resolved to file path |
| `@/aliased/path` | Resolved via tsconfig paths |
| `lodash` | External (node_modules) |
| `@myorg/utils` | Workspace package (if in workspace) |
| `@scope/package` | External (node_modules) |

File paths participate in file-level (inner) cycle detection. Workspace package imports participate in package-level (outer) cycle detection. External node_modules are ignored.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | No circular dependencies found |
| 1 | Circular dependencies detected |
| 2 | Error occurred (invalid config, parse error, etc.) |

## Library Usage

jscycles can be used as a Rust library:

```rust
use jscycles::{
    Config, CycleDetector, DependencyGraph, ImportExtractor,
    PackageDiscovery, TsConfig, Workspace,
};
use jscycles::cycles::PackageCycleDetector;
use jscycles::graph::PackageDependencyGraph;

// Load configuration
let config = Config::load_or_default(Path::new("jscycles.yaml"));

// Discover workspace for inter-package cycles
let workspace = Workspace::discover(&PathBuf::from("."))?;

// Discover packages
let discovery = PackageDiscovery::new(&config, &[], &[])?;
let packages = discovery.discover(&PathBuf::from("."))?;

// Collect imports from all packages
let mut imports_by_package = HashMap::new();
for package in &packages {
    let tsconfig = TsConfig::discover(&package.path);
    let extractor = ImportExtractor::new(
        vec!["ts".into(), "tsx".into()],
        tsconfig,
    ).with_workspace(workspace.clone());

    let imports = extractor.extract(&package.path)?;

    // File-level cycles
    let graph = DependencyGraph::from_imports(&imports);
    let cycles = CycleDetector::detect(&graph);
    println!("{}: {} file cycles", package.name, cycles.len());

    imports_by_package.insert(package.name.clone(), imports);
}

// Package-level cycles
if workspace.is_some() {
    let pkg_graph = PackageDependencyGraph::from_imports(&imports_by_package);
    let pkg_cycles = PackageCycleDetector::detect(&pkg_graph);
    println!("{} inter-package cycles", pkg_cycles.len());
}
```

## Comparison with madge

| Feature | madge | jscycles |
|---------|-------|----------|
| Performance | Slow on large repos | 50-100x faster |
| Language | JavaScript | Rust |
| Parallel processing | No | Yes (rayon) |
| tsconfig paths | Yes | Yes |
| JSON output | Yes | Yes |
| Webpack config | Yes | No |
| Image output | Yes | No |
| Monorepo support | Limited | First-class |
| Inter-package cycles | No | Yes |
| Workspace detection | No | npm/pnpm/TypeScript |

## Related Tools

- **[madge](https://github.com/pahen/madge)** - Popular JavaScript dependency graph and circular dependency detector (see [comparison](#comparison-with-madge) above)
- **[NX](https://nx.dev/features/enforce-module-boundaries)** - Monorepo build system with module boundary enforcement and circular dependency detection via ESLint rules
- **[eslint-plugin-import](https://github.com/import-js/eslint-plugin-import)** - ESLint plugin with `no-cycle` rule for detecting circular dependencies inline during development
- **[dpdm](https://github.com/acrazing/dpdm)** - JavaScript/TypeScript circular dependency detector with tree output
- **[circular-dependency-plugin](https://github.com/aackerman/circular-dependency-plugin)** - Webpack plugin that detects cycles during bundling

## Troubleshooting

### "No packages found"

- Ensure you're running from a directory with `package.json` files
- Check your `scan.include` patterns in `jscycles.yaml`
- Use `--only` to explicitly specify package patterns

### Path aliases not resolving

- Ensure `tsconfig.json` exists and has `compilerOptions.paths`
- Check that `baseUrl` is set correctly
- Use `--tsconfig` to specify an explicit path
- Verify the alias pattern matches (e.g., `@/*` vs `@/`)

### False positives

- Add patterns to `ignore` in package config
- Use `--exclude` to skip problematic packages
- Check for `export * from` re-exports that create apparent cycles

## Development

```bash
# Run all checks (format, clippy, test, deny, doc)
cargo xtask ci
```

See [CLAUDE.md](CLAUDE.md) for detailed development guidelines.

## License

MIT OR Apache-2.0
