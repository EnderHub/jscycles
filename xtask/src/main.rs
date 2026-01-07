//! xtask - Build automation for agentic Rust development.
//!
//! This binary provides a single entry point for all CI/validation tasks.
//! Agents and developers should run `cargo xtask ci` to validate changes.
//!
//! # Available Commands
//!
//! - `ci` - Run all checks (format, clippy, test, deny)
//! - `fmt` - Check code formatting
//! - `clippy` - Run clippy with all targets
//! - `test` - Run all tests
//! - `deny` - Check dependencies (licenses, security, duplicates)
//! - `doc` - Build documentation
//! - `install-tools` - Install required cargo tools

use std::env;
use std::process::ExitCode;
use xshell::{Shell, cmd};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let task = args.first().map(String::as_str);

    let sh = match Shell::new() {
        Ok(sh) => sh,
        Err(err) => {
            eprintln!("Failed to create shell: {err}");
            return ExitCode::FAILURE;
        },
    };

    let result = match task {
        Some("ci") => ci(&sh),
        Some("fmt") => fmt(&sh),
        Some("clippy") => clippy(&sh),
        Some("test") => test(&sh),
        Some("deny") => deny(&sh),
        Some("doc") => doc(&sh),
        Some("install-tools") => install_tools(&sh),
        Some("help" | "-h" | "--help") | None => {
            print_help();
            Ok(())
        },
        Some(unknown) => {
            eprintln!("Unknown command: {unknown}");
            print_help();
            Err(())
        },
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

fn print_help() {
    eprintln!(
        r"
xtask - Build automation for agentic Rust development

USAGE:
    cargo xtask <COMMAND>

COMMANDS:
    ci             Run all checks (fmt, clippy, test, deny)
    fmt            Check code formatting
    clippy         Run clippy with all targets
    test           Run all tests
    deny           Check dependencies (licenses, security, duplicates)
    doc            Build documentation
    install-tools  Install required cargo tools (cargo-deny)
    help           Print this help message

EXAMPLES:
    cargo xtask ci           # Run full CI pipeline
    cargo xtask clippy       # Just run clippy
    cargo xtask install-tools # Install cargo-deny if missing
"
    );
}

/// Run the full CI pipeline.
fn ci(sh: &Shell) -> Result<(), ()> {
    eprintln!("\n=== Running full CI pipeline ===\n");

    fmt(sh)?;
    clippy(sh)?;
    test(sh)?;
    deny(sh)?;
    doc(sh)?;

    eprintln!("\n=== All checks passed ===\n");
    Ok(())
}

/// Check code formatting.
fn fmt(sh: &Shell) -> Result<(), ()> {
    eprintln!(">> cargo fmt --check");
    cmd!(sh, "cargo fmt --check")
        .run()
        .map_err(|err| eprintln!("fmt failed: {err}"))
}

/// Run clippy on all targets.
fn clippy(sh: &Shell) -> Result<(), ()> {
    eprintln!(">> cargo clippy --all-targets --all-features");
    cmd!(sh, "cargo clippy --all-targets --all-features")
        .run()
        .map_err(|err| eprintln!("clippy failed: {err}"))
}

/// Run all tests.
fn test(sh: &Shell) -> Result<(), ()> {
    eprintln!(">> cargo test --all-features");
    cmd!(sh, "cargo test --all-features")
        .run()
        .map_err(|err| eprintln!("test failed: {err}"))
}

/// Check dependencies with cargo-deny.
fn deny(sh: &Shell) -> Result<(), ()> {
    eprintln!(">> cargo deny check");

    // Check if cargo-deny is installed
    if cmd!(sh, "cargo deny --version").quiet().run().is_err() {
        eprintln!("cargo-deny not installed. Run: cargo xtask install-tools");
        return Err(());
    }

    cmd!(sh, "cargo deny check")
        .run()
        .map_err(|err| eprintln!("deny failed: {err}"))
}

/// Build documentation.
fn doc(sh: &Shell) -> Result<(), ()> {
    eprintln!(">> cargo doc --no-deps --all-features");
    cmd!(sh, "cargo doc --no-deps --all-features")
        .run()
        .map_err(|err| eprintln!("doc failed: {err}"))
}

/// Install required cargo tools.
fn install_tools(sh: &Shell) -> Result<(), ()> {
    eprintln!(">> Installing cargo-deny...");
    cmd!(sh, "cargo install cargo-deny")
        .run()
        .map_err(|err| eprintln!("install failed: {err}"))
}
