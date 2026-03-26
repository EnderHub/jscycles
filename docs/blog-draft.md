# Why Circular Dependencies Matter More Than You Think

## 1. The Bug That Shouldn't Exist

The error message was simple: `Cannot read property 'validateUser' of undefined`.

The code was correct. TypeScript compiled it. The tests passed. The function existed, was exported, and was imported exactly where it needed to be.

It took four hours to find the problem. `auth.ts` imported from `user.ts`. `user.ts` imported from `permissions.ts`. `permissions.ts` imported from `auth.ts`. A cycle.

When Node.js loaded these modules, it hit the cycle and returned a partially initialized module. The function existed in the source. It didn't exist at runtime. No error at compile time. No warning at build time. Just a crash in production at 2am.

This is what circular dependencies do. They create bugs that shouldn't exist. The code is correct. The types check. The tests pass. And then it breaks anyway.

## 2. Why JavaScript Makes This Worse

Some languages prevent circular dependencies structurally. Go refuses to compile with package import cycles. Rust's Cargo forbids cycles between crates. These constraints are enforced by the toolchain, not by discipline.

JavaScript has no such constraints.

ES modules execute top-to-bottom. When module A imports module B, execution pauses while B loads. If B imports A, it gets whatever A has exported so far. If A hasn't reached its exports yet, B gets an empty object or undefined.

The result depends on import order. Change which file loads first, and the behavior changes. Add a new import somewhere else in the codebase, and the load order shifts. Code that worked yesterday breaks today with no changes to the affected files.

CommonJS has the same problem with a different flavor. `require()` returns whatever `module.exports` contains at the moment of the call. If the cycle hasn't finished initializing, you get a partial object.

TypeScript adds types but doesn't add safety here. The type checker sees the final shape of every module. It doesn't simulate the runtime initialization order. Your types say the function exists. The runtime says it doesn't.

Bundlers make it worse in different ways. Webpack concatenates modules and hoists declarations, which sometimes masks cycles and sometimes exposes them. Esbuild and Vite preserve more of the original module semantics, which means cycles that "worked" in Webpack suddenly break.

Tree shaking adds another layer. A cycle might work in development but break in production because the tree shaker removed a module that was keeping the initialization order stable.

The result is a class of bugs that are hard to reproduce, hard to diagnose, and hard to fix. They appear randomly. They disappear when you add console.log statements. They come back when you remove them.

## 3. The Compound Interest of Technical Debt

Every codebase has some cycles. The question is whether you find them early or late.

Early means when they're introduced. One developer adds an import that creates a cycle. The feedback is immediate. They refactor before committing. The cycle never enters the codebase.

Late means weeks or months after introduction. The cycle has been there for a while. Other code has grown around it. The original developer has moved on. Nobody remembers why the import was added.

Fixing a cycle early takes minutes. Fixing a cycle late takes hours or days. The code has to be understood, refactored, and tested. Other code that accidentally depended on the cycle has to be fixed. The risk of regression is high.

This is compound interest working against you. One cycle makes the next one easier to introduce. Once you have ten cycles, adding an eleventh barely registers. Once you have fifty, nobody tries to fix them anymore.

The threshold for action keeps rising. First it's "we'll fix cycles when we see them." Then it's "we'll fix cycles in the next sprint." Then it's "we'll fix cycles in the next quarter." Then it's "we'll fix cycles when we rewrite this module." Then it's never.

The only way to beat compound interest is to never let it start.

## 4. Why Teams Stop Checking

The tools exist. Madge has detected circular dependencies since 2013. ESLint has the `import/no-cycle` rule. Every team knows cycles are bad.

So why do they accumulate?

The answer is feedback loops. A tool that runs in two seconds gets run constantly. A tool that runs in two minutes gets run occasionally. A tool that runs in ten minutes gets run before releases. A tool that runs in thirty minutes doesn't get run.

Madge is a good tool. On a small codebase, it's fast enough. On a large codebase, it's not. On a monorepo with thousands of files, it can take several minutes. That's long enough to skip. That's long enough to remove from CI when the build is slow.

ESLint's `import/no-cycle` is worse. It runs on every file, but it has to traverse the entire graph from each file to find cycles. On a large codebase, this makes ESLint unusably slow. Teams disable the rule.

The intent is good. The execution fails because the feedback loop is too slow.

There's a second problem: output quality. When you run a cycle detector for the first time on an established codebase, you get a wall of cycles. Fifty cycles. A hundred cycles. Too many to process.

The team looks at the wall, feels overwhelmed, and closes the terminal. The detector told the truth. The truth was too much to handle. Nothing changes.

Effective tooling needs two properties: fast enough to run constantly, and incremental enough to show progress. If you can't have both, you get neither.

## 5. The Monorepo Blind Spot

File-level cycles are well understood. `a.ts` imports `b.ts` imports `a.ts`. The cycle is visible in the import statements.

Package-level cycles are architectural. `@myorg/auth` depends on `@myorg/user` depends on `@myorg/auth`. The cycle is invisible unless you look at `package.json` files across multiple directories.

Most cycle detectors only find file-level cycles. They don't understand workspaces. They don't know that `@myorg/auth` is a local package, not an npm dependency. They see an import from `@myorg/user` and ignore it.

This matters because package-level cycles are worse than file-level cycles.

A file-level cycle is a local problem. Two files in the same directory import each other. The fix is local: move code, extract a third file, use dependency injection.

A package-level cycle is an architectural problem. Two packages that should be independent are coupled. The package boundary is a lie. You can't deploy one without the other. You can't test one without the other. You can't understand one without the other.

Package boundaries are supposed to enforce constraints. This package handles authentication. That package handles user profiles. The auth package can depend on the user package, but not vice versa. When both depend on each other, the constraint is gone.

The refactoring trap follows. You want to extract the auth package into a shared library. You can't, because it depends on code that depends on it. The cycle has locked you in.

Teams build monorepos to enable modularity. Package cycles destroy that modularity. The code is physically separated into packages but logically welded together.

## 6. What We Built

jscycles is a circular dependency detector written in Rust.

The design principles are simple:

**Fast enough to run constantly.** A codebase that takes madge three minutes takes jscycles two seconds. That's fast enough for CI. That's fast enough for pre-commit hooks. That's fast enough to run on every file save if you want.

**Monorepo-native.** Package-level cycles are detected alongside file-level cycles. Workspaces are discovered automatically from `package.json`, `pnpm-workspace.yaml`, or TypeScript project references.

**Zero configuration for common cases.** Point it at a directory and it works. TypeScript path aliases are resolved automatically. Package names are detected automatically.

**Honest output.** File cycles and package cycles are reported separately. Clean packages are shown as clean. The output is structured enough to parse programmatically.

What we chose not to build matters too.

No image output. Madge can generate dependency graphs as images. This is useful for documentation but not for CI. We focused on detection, not visualization.

No webpack configuration. Madge can read webpack configs to understand aliases. This adds complexity and edge cases. We read `tsconfig.json` instead, which covers the common case.

No auto-fixing. Cycle detection is straightforward. Cycle fixing requires understanding intent. We tell you where the cycles are. You decide how to fix them.

## 7. The Algorithm

Cycle detection is a graph problem. The question is which algorithm to use.

The naive approach is depth-first search from each node. Start at file A, follow imports, see if you get back to A. This works but does redundant work. If you've already determined that file B is not part of a cycle, you shouldn't re-traverse it when checking file C.

The correct approach is Tarjan's strongly connected components algorithm. It does a single depth-first traversal of the entire graph and identifies all strongly connected components. A strongly connected component is a maximal set of nodes where every node can reach every other node. Any SCC with more than one node contains a cycle.

Tarjan's algorithm runs in O(V + E) time. V is the number of files. E is the number of imports. It finds all cycles in a single pass. No redundant work.

The algorithm maintains a stack of nodes being explored and assigns each node two numbers: an index (when it was discovered) and a lowlink (the smallest index reachable from it). When a node's lowlink equals its index, it's the root of an SCC. Pop the stack to get the component.

The implementation details matter for correctness:

**Import extraction.** We need to find all imports in each file. This means parsing the AST and extracting:
- Static imports: `import x from './y'`
- Named imports: `import { x } from './y'`
- Namespace imports: `import * as x from './y'`
- Side-effect imports: `import './y'`
- Re-exports: `export { x } from './y'` and `export * from './y'`
- Dynamic imports: `import('./y')`
- CommonJS: `require('./y')`

Each of these creates a dependency edge in the graph.

**Path resolution.** An import like `./utils` might resolve to `./utils.ts`, `./utils.tsx`, `./utils.js`, `./utils/index.ts`, or several other possibilities. TypeScript path aliases like `@/utils` need to be resolved using `tsconfig.json`. Workspace package imports like `@myorg/utils` need to be recognized as local packages.

**Cycle normalization.** The same cycle can be reported starting from different nodes. `A → B → C → A` and `B → C → A → B` are the same cycle. We normalize by starting from the lexicographically smallest file. This makes output consistent across runs and makes cycles comparable.

## 8. TypeScript Path Resolution

TypeScript path aliases look simple. They're not.

A basic `tsconfig.json` might have:

```json
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"],
      "@components/*": ["src/components/*"]
    }
  }
}
```

An import of `@/utils/format` should resolve to `src/utils/format.ts`. Straightforward so far.

The complexity comes from edge cases:

**Multiple candidates.** The path `src/utils/format` might be `src/utils/format.ts`, `src/utils/format.tsx`, `src/utils/format.js`, `src/utils/format.jsx`, `src/utils/format/index.ts`, or `src/utils/format/index.tsx`. We try each in order until one exists.

**Wildcard matching.** `@/*` matches `@/anything`. `@components/*` matches `@components/anything`. But `@utils` (no wildcard) only matches exactly `@utils`. The wildcard position matters.

**baseUrl interaction.** Paths are relative to `baseUrl`, not to the `tsconfig.json` location. If `baseUrl` is `./src` and paths has `@/*: ["./*"]`, then `@/utils` resolves to `src/utils`, not `./utils`.

**Extends and references.** A `tsconfig.json` can extend another config. Path aliases might be defined in the base config. TypeScript project references add another layer of config files. We walk up the directory tree and merge configs.

**Graceful degradation.** When we can't resolve an alias, we treat it as an external module. This is correct: if `@/missing` doesn't resolve to a file, it's either a mistake (TypeScript will catch it) or an external module. Either way, it's not part of the dependency graph.

The `--no-tsconfig` flag disables all this. Use it when your path aliases are handled by a bundler with different resolution rules.

## 9. Workspace Detection

A monorepo has multiple packages. jscycles needs to know which packages exist and what they're called.

**npm, yarn, and bun** use the `workspaces` field in `package.json`:

```json
{
  "workspaces": ["packages/*", "apps/*"]
}
```

This means "every directory matching `packages/*` or `apps/*` that contains a `package.json` is a workspace package."

**pnpm** uses `pnpm-workspace.yaml`:

```yaml
packages:
  - "packages/*"
  - "apps/*"
```

Same semantics, different file.

**TypeScript** uses project references in `tsconfig.json`:

```json
{
  "references": [
    { "path": "./packages/core" },
    { "path": "./packages/utils" }
  ]
}
```

This is more explicit: each referenced project is a package.

We check for all three formats and merge the results. Most monorepos use one format consistently, but some use multiple.

Once we know the packages, we build a map from package name to directory. When we see an import of `@myorg/utils`, we check if that's a workspace package. If it is, the import creates an edge in the package dependency graph. If it's not, it's an external dependency and we ignore it.

The `--only` and `--exclude` flags filter packages by name pattern:

```bash
jscycles --only "@myorg/feature-*" --exclude "@myorg/legacy-*"
```

This checks only packages matching `@myorg/feature-*`, excluding any that also match `@myorg/legacy-*`. Useful for focusing on specific areas of a large monorepo.

## 10. Performance

The performance difference is large enough to change behavior.

| Project Size | madge | jscycles | Factor |
|--------------|-------|----------|--------|
| 100 files    | ~2s   | ~20ms    | 100x   |
| 1,000 files  | ~15s  | ~100ms   | 150x   |
| 10,000 files | ~3min | ~2s      | 90x    |

These are rough numbers from real codebases. Your results will vary based on file size, import density, and disk speed.

Where does the difference come from?

**Language runtime.** Madge runs on Node.js. Node is fast for JavaScript, but it's still an interpreted language with garbage collection pauses. Rust compiles to native code with no runtime overhead.

**Parallelism.** Madge parses files sequentially. jscycles uses Rayon to parse files in parallel. On an 8-core machine, this is roughly 8x faster for the parsing phase.

**AST extraction.** We use ast-grep for parsing. It's a tree-sitter based parser optimized for code search and extraction. We only extract imports, not the full AST.

**Memory allocation.** Rust's ownership model lets us avoid most heap allocations during the hot loop. Strings are borrowed, not copied. The graph is built in place.

The scaling characteristics matter too. Madge's time grows faster than linearly with codebase size because of repeated work and memory pressure. jscycles stays closer to linear.

Cold start versus warm cache: the numbers above are cold start (no OS file cache). With a warm cache, both tools are faster, but the ratio stays similar.

## 11. Output and Integration

### Human-Readable Output

The default output is designed for humans:

```
=== File-level cycles ===
✓ @myorg/utils: no cycles
✓ @myorg/config: no cycles
✗ @myorg/core: 2 cycles
  components/Button.tsx → hooks/useTheme.ts → components/Button.tsx
  services/api.ts → services/auth.ts → services/api.ts
✓ @myorg/ui: no cycles

=== Package-level cycles ===
✗ @myorg/core → @myorg/utils → @myorg/core

Summary: 4 packages, 2 file cycles, 1 package cycle
```

Green checkmarks for clean packages. Red X for packages with problems. Cycles are shown as paths, not just sets of files.

The `--inner` flag shows only file-level cycles. The `--outer` flag shows only package-level cycles. Use these to focus your attention.

### JSON Output

The `--json` flag produces machine-readable output:

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

Use this for custom reporting, dashboards, or integration with other tools.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | No cycles found |
| 1 | Cycles detected |
| 2 | Error (invalid config, parse failure, etc.) |

This makes CI integration simple:

```bash
jscycles --quiet || exit 1
```

The `--quiet` flag suppresses output when there are no cycles. CI stays clean until there's a problem.

### Pre-Commit Hooks

For local enforcement, add to `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: jscycles
        name: Check circular dependencies
        entry: jscycles --quiet
        language: system
        pass_filenames: false
```

This runs on every commit. Cycles are caught before they're pushed.

### GitHub Actions

```yaml
- name: Check circular dependencies
  run: jscycles --quiet
```

That's it. The exit code handles the rest.

For incremental checking in large monorepos:

```yaml
- name: Get changed packages
  id: changes
  run: |
    PACKAGES=$(git diff --name-only origin/main | grep -E '^packages/' | cut -d/ -f2 | sort -u | xargs -I{} echo "packages/{}" | tr '\n' ' ')
    echo "packages=$PACKAGES" >> $GITHUB_OUTPUT

- name: Check circular dependencies
  run: jscycles ${{ steps.changes.outputs.packages }}
```

This only checks packages that have changes, keeping CI fast on large monorepos.

## 12. Fixing Cycles: Patterns and Strategies

Detecting cycles is the first step. Fixing them is the second.

### Pattern 1: Type-Only Cycles

Sometimes the cycle is only in types, not runtime code:

```typescript
// user.ts
import type { Permission } from './permission';
export interface User {
  permissions: Permission[];
}

// permission.ts
import type { User } from './user';
export interface Permission {
  grantedTo: User[];
}
```

This cycle exists in the import graph but has no runtime impact. TypeScript's `import type` is erased at compile time. The runtime code has no circular dependency.

**Fix:** Use `import type` consistently for type-only imports. Some cycle detectors (including jscycles) still report these. Consider whether the coupling is acceptable.

### Pattern 2: Barrel File Cycles

Barrel files (`index.ts` that re-exports everything) create apparent cycles:

```typescript
// components/index.ts
export * from './Button';
export * from './Input';

// components/Button.tsx
import { Input } from './index';  // Creates cycle through barrel
```

The Button imports from the barrel, which re-exports Button. Technically a cycle, though often harmless.

**Fix:** Import directly instead of through the barrel:

```typescript
import { Input } from './Input';
```

Or restructure to avoid the barrel importing from files that import from it.

### Pattern 3: Shared Utilities That Grew

A utility module starts small and grows dependencies:

```typescript
// utils/format.ts
import { getUserLocale } from '../user/locale';

// user/profile.ts
import { formatDate } from '../utils/format';
```

If `user/locale.ts` imports anything from `user/profile.ts`, you have a cycle through the utility.

**Fix:** Extract the locale-independent parts of `format.ts` into a separate module. Or move `getUserLocale` to a lower-level module that doesn't depend on user.

### Pattern 4: Feature Modules Reaching Into Each Other

Two features that seem independent are coupled:

```typescript
// features/checkout/cart.ts
import { getShippingEstimate } from '../shipping/estimate';

// features/shipping/estimate.ts
import { getCartTotal } from '../checkout/cart';
```

Both features need information from each other.

**Fix options:**

1. **Extract shared state.** Create a `features/order` module that both depend on.

2. **Dependency inversion.** Define an interface in shipping, implement it in checkout:

```typescript
// features/shipping/types.ts
export interface CartProvider {
  getTotal(): number;
}

// features/shipping/estimate.ts
export function getShippingEstimate(cart: CartProvider) { ... }

// features/checkout/cart.ts
import { getShippingEstimate } from '../shipping/estimate';
getShippingEstimate({ getTotal: () => this.total });
```

3. **Move code.** If shipping needs cart total, maybe shipping estimate belongs in checkout.

### Breaking Package-Level Cycles

Package cycles need architectural fixes, not just code moves.

If `@myorg/auth` and `@myorg/user` depend on each other:

1. **Identify the shared concepts.** What do both packages need? User identity? Session state?

2. **Extract to a third package.** Create `@myorg/identity` that both depend on.

3. **Define interfaces at the lower level.** If auth needs user data, define a `UserProvider` interface in auth. User implements it.

4. **Consider merging.** If two packages can't exist independently, maybe they shouldn't be separate packages.

### When Cycles Are Acceptable

Not all cycles need fixing:

**Type-only cycles** have no runtime impact. If you're using `import type` consistently, the cycle is documentation, not a bug.

**Test file cycles** don't ship to production. A test importing from code that imports from test utilities is annoying but not dangerous.

**Intentional co-location** is rare but valid. Some code is genuinely circular by nature. Document it and move on.

The goal is zero accidental cycles, not zero cycles.

## 13. Comparison With Other Tools

### madge

The original. Mature, widely used, generates visual graphs.

**Strengths:** Image output for documentation. Webpack config support. Established ecosystem.

**Weaknesses:** Slow on large codebases. No workspace awareness. No package-level cycle detection.

**Use together:** Use madge for visualization, jscycles for detection.

### eslint-plugin-import (no-cycle rule)

Inline feedback in your editor.

**Strengths:** Catches cycles as you type. Integrates with existing ESLint setup.

**Weaknesses:** Very slow on large codebases (often disabled). Per-file analysis means repeated work. No package-level awareness.

**Use together:** Enable for small projects, disable for large ones and use jscycles in CI instead.

### NX

Full monorepo toolkit with module boundary enforcement.

**Strengths:** Comprehensive solution. Boundary rules prevent cycles by design. Caching, affected detection, task orchestration.

**Weaknesses:** Requires adopting NX. Heavy for just cycle detection.

**Use together:** If you're using NX, use its boundary rules. jscycles can supplement for file-level cycles within packages.

### dpdm

TypeScript-focused cycle detector with tree output.

**Strengths:** Good TypeScript support. Tree visualization.

**Weaknesses:** No monorepo support. Slower than jscycles.

**Use together:** dpdm for tree visualization if you need it, jscycles for detection.

### circular-dependency-plugin

Webpack plugin that catches cycles during bundling.

**Strengths:** Catches cycles at build time. No separate tool to run.

**Weaknesses:** Webpack only. Doesn't work with Vite, esbuild, or non-bundled code.

**Use together:** Enable in webpack builds as a second layer of defense.

## 14. Configuration

Most projects need no configuration. For complex setups, create `jscycles.yaml`:

```yaml
# Where to find packages
scan:
  include:
    - "packages/*"
    - "apps/*"
  exclude:
    - "**/node_modules"
    - "**/dist"
    - "**/build"

# Defaults for all packages
defaults:
  extensions:
    - ts
    - tsx
    - js
    - jsx

# Per-package overrides
packages:
  "@myorg/legacy-*":
    extensions:
      - js
    ignore:
      - "**/*.test.js"

  "@myorg/feature-*":
    ignore:
      - "**/*.test.ts"
      - "**/*.spec.ts"
      - "**/__tests__/**"
      - "**/__mocks__/**"
```

CLI arguments override config file. Config file overrides defaults.

## 15. The Feedback Loop

Tools shape behavior. Behavior shapes codebases.

A cycle detector that takes three minutes doesn't get run. Cycles accumulate. The codebase degrades. Eventually someone proposes a rewrite.

A cycle detector that takes two seconds gets run constantly. Cycles are caught at introduction. The codebase stays clean. The rewrite never becomes necessary.

This isn't about jscycles specifically. It's about feedback loops.

Fast tests get run. Slow tests get skipped. Fast linters get enabled. Slow linters get disabled. Fast formatters run on save. Slow formatters run never.

The tools you can run constantly are the tools that shape your codebase. Everything else is aspirational.

jscycles is fast enough to run constantly. That's the point.

## 16. Getting Started

Install:

```bash
cargo install jscycles
```

Run:

```bash
jscycles
```

That's it. No configuration required for most projects.

If you have cycles, you'll see them. If you don't, you'll see a clean report.

Add to CI:

```bash
jscycles --quiet || exit 1
```

Now cycles are caught before they merge.

The goal isn't to have a fast cycle detector. The goal is to have a codebase where cycles don't exist because they can't survive the feedback loop.

---

## Appendix: CLI Reference

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
  -v, --verbose              Show detailed progress
  -h, --help                 Print help
  -V, --version              Print version
```
