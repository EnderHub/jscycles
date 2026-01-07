# CLAUDE.md - Agent Instructions

This repository uses **maximum lint strictness** for Rust development. Read this before making changes.

## Quick Start

**Before every commit, run:**
```bash
cargo xtask ci
```

This runs all checks: format, clippy, test, deny, doc.

## Lint Policy: FORBID

Almost all lints are set to `forbid` level - they **cannot be overridden** with `#[allow(...)]`.

### What's Banned (No Exceptions)

| Banned | Use Instead |
|--------|-------------|
| `unwrap()` | `unwrap_or()`, `unwrap_or_default()`, `?`, `if let`, `match` |
| `expect()` | Same as above |
| `panic!()` | Return `Result`, handle errors explicitly |
| `todo!()` | Don't commit incomplete code |
| `unimplemented!()` | Implement it or don't include it |
| `println!()` | Use a logging framework or return values |
| `eprintln!()` | Use a logging framework |
| `dbg!()` | Remove before commit |
| `unsafe` | Forbidden entirely |
| Array indexing `[i]` | Use `.get(i)` and handle `None` |

### Allowed in Tests

| Allowed | Notes |
|---------|-------|
| `assert!()` | Must include a message: `assert!(cond, "message")` |
| `assert_eq!()` | Must include a message: `assert_eq!(a, b, "message")` |
| `assert_ne!()` | Must include a message: `assert_ne!(a, b, "message")` |
| `unwrap()` | Automatically allowed via `clippy.toml` |
| `expect()` | Automatically allowed via `clippy.toml` |
| `panic!()` | Automatically allowed via `clippy.toml` |

### Nursery Lints

Nursery lints are at `deny` level (not `forbid`) because they have false positives. You can use `#[expect(..., reason = "...")]` to override them when necessary.

### Macro Compatibility (Serde, Dioxus)

Some lints are set to `deny` (not `forbid`) for derive/component macro compatibility. These macros internally use `#[allow(...)]` which conflicts with `forbid` levels:

- `missing-docs`, `missing-docs-in-private-items` - dioxus component macro
- `unused-*` lints - dioxus component macro uses `#[allow(unused)]`
- `unused-qualifications` - serde/dioxus macros trigger this
- `absolute-paths` - serde macros use absolute paths
- `useless-attribute` - serde uses `#[allow]` internally

### Pattern Matching

`pattern-type-mismatch` and `ref-patterns` are set to `deny` (not `forbid`) because they conflict with each other and `match_ref_pats` when matching enums with non-Copy fields. Match ergonomics is idiomatic Rust 2024, and these are style lints rather than safety lints.

### Code Organization

## Available Commands

```bash
cargo xtask ci           # Full CI pipeline (run before every commit)
cargo xtask fmt          # Check formatting
cargo xtask clippy       # Run clippy
cargo xtask test         # Run tests
cargo xtask deny         # Check dependencies (licenses, security)
cargo xtask doc          # Build documentation
cargo xtask install-tools # Install cargo-deny
```

**Aliases:**
```bash
cargo ci                 # Same as cargo xtask ci
cargo lint               # Same as cargo xtask clippy
```

## Configuration Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Lint levels (`[lints.rust]`, `[lints.clippy]`) |
| `clippy.toml` | Clippy thresholds and options |
| `rustfmt.toml` | Formatting rules |
| `deny.toml` | Dependency policy (licenses, security, bans) |
| `rust-toolchain.toml` | Pinned Rust version |

## Dependency Policy (cargo-deny)

- **Allowed licenses**: MIT, Apache-2.0, BSD, ISC, CC0, Unlicense, Zlib, MPL-2.0
- **Denied**: GPL, LGPL, AGPL, proprietary, unlicensed
- **Security**: Crates with known CVEs are denied
- **Sources**: Only crates.io allowed (no git dependencies)

## Code Style Enforced

- Max line length: 100 chars
- Max function lines: 50
- Max function arguments: 5
- Max nesting depth: 3
- Max cognitive complexity: 10
- Identifier min length: 2 chars (except `i`, `j`, `k`, `n`, `x`, `y`, `z`)
- All items must be documented (including private)

## Making Changes

1. Write code that compiles
2. Run `cargo xtask ci`
3. Fix any errors (you cannot `#[allow]` most lints)
4. If a nursery lint is a false positive, use `#[expect(clippy::lint_name, reason = "explanation")]`
5. Commit only when CI passes

## Common Patterns

### Instead of unwrap
```rust
// Bad
let value = some_option.unwrap();

// Good
let value = some_option.unwrap_or_default();
let value = some_option.unwrap_or(fallback);
let Some(value) = some_option else { return };
let value = some_option?;
```

### Instead of indexing
```rust
// Bad
let item = vec[0];

// Good
let Some(item) = vec.first() else { return };
let item = vec.get(0).copied().unwrap_or_default();
```

### Testing
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        let data = setup().unwrap();  // allowed in tests via clippy.toml
        let result = compute(&data);
        assert_eq!(result, 42, "compute() should return 42");
    }
}
```

## Why This Strict?

This template is designed for **agentic programming** where AI agents write code. Strict lints:

1. Prevent agents from taking shortcuts
2. Force explicit error handling
3. Catch bugs at compile time
4. Ensure consistent code quality
5. Make code review trivial (if CI passes, code is acceptable)
