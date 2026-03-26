# jscycles Blog Post Outline

## Working Title
"Why Circular Dependencies Matter More Than You Think (And How We Fixed Detection)"

---

## 1. The Hidden Cost of Circular Dependencies

**Goal**: Establish the problem as more serious than most teams realize.

- Open with a war story: production crash, cryptic error, hours of debugging
- Circular deps don't fail at compile time in JS/TS - they fail at runtime
- The failure mode is undefined imports, not clear error messages
- Small codebases: you find them by accident
- Large codebases: they multiply silently
- Monorepos: inter-package cycles create invisible coupling

**Key points to expand**:
- Why JavaScript's module system makes this worse than other languages
- The difference between "works in dev" and "works in production" with tree shaking
- How bundlers (webpack, esbuild, vite) handle cycles differently
- Real examples of cycle-induced bugs (initialization order, undefined at runtime)

---

## 2. Why Teams Stop Checking

**Goal**: Explain why this problem persists despite known solutions.

- madge exists and works - so why do cycles still accumulate?
- The feedback loop problem: slow tools don't get run
- CI that takes 3 minutes per check gets skipped or removed
- Manual checks become "run it before release" which becomes "run it never"
- The backlog trap: once you have 50 cycles, fixing them becomes a project

**Key points to expand**:
- Psychology of developer tools: instant feedback vs delayed feedback
- The compound interest of technical debt
- Why "we'll fix it later" means "we'll never fix it"
- How cycles spread: one cycle makes the next one easier to introduce

---

## 3. The Monorepo Blind Spot

**Goal**: Introduce the inter-package cycle problem that most tools miss.

- File-level cycles are well understood
- Package-level cycles are architectural problems disguised as code problems
- Most tools (madge, eslint-plugin-import) don't understand workspaces
- A cycle between @myorg/core and @myorg/utils means they're not really separate packages
- This breaks the mental model of your architecture

**Key points to expand**:
- How package boundaries are supposed to work
- The difference between "depends on" and "coupled with"
- Why NX module boundary rules exist (and why they're not enough)
- Real examples: auth package depending on user package depending on auth
- The refactoring trap: can't extract a package that's part of a cycle

---

## 4. What We Built

**Goal**: Introduce jscycles and its design philosophy.

- Written in Rust for raw speed
- Parallel processing with Rayon
- Tarjan's SCC algorithm for correct cycle detection
- First-class monorepo support: workspaces, package filtering, inter-package cycles
- TypeScript path alias resolution built in

**Key points to expand**:
- Why Rust? (not just "it's fast" - memory safety, fearless concurrency, no GC pauses)
- Why Tarjan's algorithm? (linear time, finds ALL cycles, not just some)
- Why parallel? (modern machines have cores sitting idle)
- Design decisions: what we chose NOT to build (webpack config, image output)

---

## 5. The Algorithm

**Goal**: Technical deep dive for engineers who want to understand the internals.

- Import extraction: what counts as a dependency?
  - Static imports: `import x from './y'`
  - Re-exports: `export * from './y'`
  - Dynamic imports: `import('./y')`
  - CommonJS: `require('./y')`
- Graph construction
  - Nodes: files
  - Edges: imports
  - Resolution: relative paths, aliases, index files
- Tarjan's strongly connected components
  - Single DFS pass
  - O(V + E) time complexity
  - Finds ALL SCCs, not just cycles
  - SCC with >1 node = cycle
- Cycle normalization
  - Start from lexicographically smallest file
  - Makes cycles comparable across runs

**Key points to expand**:
- Code examples showing each import type
- Visual diagram of graph construction
- Step-by-step Tarjan walkthrough with example
- Why SCC is better than naive cycle detection (DFS from each node)
- How we handle edge cases: self-imports, re-exports, barrel files

---

## 6. TypeScript Path Resolution

**Goal**: Explain the complexity of resolving TS imports correctly.

- `tsconfig.json` paths are not simple string replacement
- baseUrl matters
- Wildcards: `@/*` vs `@/specific`
- Multiple candidates: try .ts, .tsx, .js, .jsx, /index.*
- Monorepo tsconfigs: extends, references, multiple configs

**Key points to expand**:
- Real examples of path alias patterns
- How we discover tsconfig.json (walk up the tree)
- Handling missing or invalid tsconfigs gracefully
- The `--no-tsconfig` escape hatch and when to use it

---

## 7. Workspace Detection

**Goal**: Show how jscycles understands monorepo structures.

- npm/yarn: `package.json` workspaces field
- pnpm: `pnpm-workspace.yaml`
- TypeScript: project references
- Automatic detection: no configuration required
- Package name resolution: `@myorg/utils` -> `packages/utils`

**Key points to expand**:
- How each workspace format works
- Glob pattern handling for workspace definitions
- Handling nested workspaces
- The difference between "workspace package" and "external dependency"

---

## 8. Performance

**Goal**: Show the real numbers and explain why they matter.

- Benchmarks: madge vs jscycles on real codebases
  - 100 files: 2s vs 20ms (100x)
  - 1,000 files: 15s vs 100ms (150x)
  - 10,000 files: 3min vs 2s (90x)
- Where the time goes in madge (sequential parsing, JS overhead, repeated work)
- Where we saved time (parallel parsing, zero-copy AST, no runtime overhead)

**Key points to expand**:
- Methodology: how we measured
- Variance: cold start vs warm cache
- Memory usage comparison
- Scaling characteristics: does it stay fast as the codebase grows?

---

## 9. Integration Patterns

**Goal**: Show how to actually use this in a real workflow.

### Local Development
- Run manually during development
- Editor integration (future?)
- Pre-commit hooks

### CI/CD
- Basic gate: fail if cycles exist
- Incremental: only check changed packages
- Baseline: allow existing cycles, fail on new ones
- JSON output for custom reporting

### Monorepo Strategies
- Check all packages
- Check only changed packages (with --only)
- Separate inner vs outer cycle policies
- Different rules for different package types

**Key points to expand**:
- Example GitHub Actions workflow
- Example pre-commit hook
- How to introduce this to a codebase with existing cycles
- Gradual adoption: start with --outer, then add --inner

---

## 10. Fixing Cycles: Patterns and Strategies

**Goal**: Don't just detect - help people fix.

### Common Cycle Patterns
- Circular type imports (often fixable with `import type`)
- Shared utilities that grew dependencies
- Feature modules that reached into each other
- Barrel file cycles (`index.ts` re-exports creating false cycles)

### Breaking Strategies
- Extract shared code to a new module
- Dependency inversion: depend on interfaces, not implementations
- Move code to the "lower" module
- Lazy loading / dynamic imports for runtime-only deps

### When Cycles Are Acceptable
- Type-only cycles (no runtime impact)
- Test files (don't ship to production)
- Intentional co-location (rare but valid)

**Key points to expand**:
- Code examples for each pattern
- Decision tree: how to choose a fix strategy
- The "barrel file" problem and how to handle it
- When to use `// jscycles-ignore` (if we add that)

---

## 11. Comparison with Other Tools

**Goal**: Honest comparison, not marketing.

| Tool | Strengths | Weaknesses |
|------|-----------|------------|
| madge | Mature, image output, webpack support | Slow, no workspace awareness |
| eslint-plugin-import | Inline feedback, IDE integration | Very slow, per-file not whole-graph |
| NX | Full monorepo solution, boundary rules | Heavy, requires NX adoption |
| dpdm | TypeScript support, tree output | No monorepo support |
| circular-dependency-plugin | Catches during build | Webpack only, no standalone |

**Key points to expand**:
- When to use each tool
- How jscycles complements (not replaces) other tools
- The case for multiple layers of defense

---

## 12. Future Direction

**Goal**: Show this is actively developed, invite contribution.

- What we're considering
  - Ignore comments / config
  - Baseline files (track known cycles)
  - Watch mode
  - Editor plugins
  - More output formats
- What we're NOT building
  - Full bundler replacement
  - Dependency visualization (use madge for that)
  - Auto-fixing (too dangerous)

---

## 13. Conclusion

**Goal**: Tie back to the business impact.

- Fast tools change behavior
- Behavior change prevents problems
- Prevention is cheaper than debugging
- The goal isn't faster detection - it's zero cycles by default

---

## Appendix Ideas

- Full CLI reference
- Configuration file reference
- Troubleshooting guide
- Migration guide from madge
- Contributing guide

---

## Style Notes (from Ender blog)

- Short paragraphs, often single sentences
- Numbered sections with clear purpose
- Technical but accessible
- No marketing fluff - just facts
- Confident, declarative voice
- Show real examples, real numbers
- Acknowledge tradeoffs honestly

---

## Open Questions

- [ ] Do we have a real war story to open with?
- [ ] Can we get permission to share real benchmark numbers from a customer codebase?
- [ ] Do we want to include the "how we built it" story (the AI-assisted development angle)?
- [ ] Should this be one long post or a series?
- [ ] What's the target audience: senior engineers? tech leads? both?
