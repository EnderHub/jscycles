//! Rust Template Project
//!
//! A project template with maximum lint strictness configured for agentic programming.
//!
//! # Usage
//!
//! Run all checks before committing:
//! ```bash
//! cargo xtask ci
//! ```

/// Entry point for the application.
///
/// This is a minimal example that satisfies all configured lints.
/// Replace this with your actual application logic.
fn main() {
    // Production code restrictions:
    // - No println!/eprintln! (use logging)
    // - No panic!() directly (use Result)
    // - No unwrap()/expect() (use proper error handling)
    //
    // See CLAUDE.md for alternatives.
}

#[cfg(test)]
mod tests {
    /// Example test demonstrating allowed patterns.
    ///
    /// Tests can use `unwrap()`, `expect()`, and assertions with messages.
    #[test]
    fn it_works() {
        let input = "42";
        let value: i32 = input.parse().unwrap();
        assert_eq!(value, 42i32, "parsed value should be 42");
    }
}
