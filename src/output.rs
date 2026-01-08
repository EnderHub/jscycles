//! Output formatting for cycle detection results.
//!
//! Provides human-readable and JSON output formats for both file-level
//! and package-level cycle detection results.

use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;

use crate::cycles::{Cycle, PackageCycle, PackageCycleWithFiles};

/// Results for a single package.
#[derive(Debug, Clone)]
pub struct PackageResult {
    /// Package name.
    pub name: String,

    /// Package path.
    pub path: PathBuf,

    /// Detected cycles.
    pub cycles: Vec<Cycle>,
}

/// Aggregated results for all packages.
#[derive(Debug, Clone)]
pub struct Results {
    /// Per-package results.
    pub packages: Vec<PackageResult>,
}

impl Results {
    /// Create empty results.
    #[inline]
    pub fn new() -> Self {
        Self {
            packages: Vec::new(),
        }
    }

    /// Check if any cycles were found.
    #[inline]
    pub fn has_cycles(&self) -> bool {
        self.packages.iter().any(|p| !p.cycles.is_empty())
    }

    /// Get total number of cycles across all packages.
    #[inline]
    pub fn total_cycles(&self) -> usize {
        self.packages.iter().map(|p| p.cycles.len()).sum()
    }

    /// Get number of packages with cycles.
    #[inline]
    pub fn packages_with_cycles(&self) -> usize {
        self.packages
            .iter()
            .filter(|p| !p.cycles.is_empty())
            .count()
    }
}

impl Default for Results {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Filter for which types of cycles to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CycleFilter {
    /// Show all cycles (default).
    #[default]
    All,
    /// Show only file-level (inner) cycles.
    Inner,
    /// Show only package-level (outer) cycles.
    Outer,
}

/// Unified results including both file-level and package-level cycles.
#[derive(Debug, Clone, Default)]
pub struct UnifiedResults {
    /// Per-package file-level results.
    pub file_results: Vec<PackageResult>,

    /// Package-level cycles (between packages).
    pub package_cycles: Vec<PackageCycle>,

    /// Package-level cycles with file detail (optional, for verbose output).
    pub package_cycles_with_files: Vec<PackageCycleWithFiles>,
}

impl UnifiedResults {
    /// Create empty unified results.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any file-level cycles were found.
    #[inline]
    pub fn has_file_cycles(&self) -> bool {
        self.file_results.iter().any(|p| !p.cycles.is_empty())
    }

    /// Check if any package-level cycles were found.
    #[inline]
    pub fn has_package_cycles(&self) -> bool {
        !self.package_cycles.is_empty() || !self.package_cycles_with_files.is_empty()
    }

    /// Check if any cycles were found (file or package level).
    #[inline]
    pub fn has_any_cycles(&self) -> bool {
        self.has_file_cycles() || self.has_package_cycles()
    }

    /// Get total number of file-level cycles.
    #[inline]
    pub fn total_file_cycles(&self) -> usize {
        self.file_results.iter().map(|p| p.cycles.len()).sum()
    }

    /// Get number of packages with file-level cycles.
    #[inline]
    pub fn packages_with_cycles(&self) -> usize {
        self.file_results
            .iter()
            .filter(|p| !p.cycles.is_empty())
            .count()
    }
}

/// Trait for output formatters.
pub trait OutputFormatter {
    /// Format results to the given writer.
    ///
    /// # Errors
    ///
    /// Returns an IO error if writing fails.
    fn format(&self, results: &Results, writer: &mut dyn Write) -> std::io::Result<()>;
}

/// Trait for unified output formatters (file + package cycles).
pub trait UnifiedOutputFormatter {
    /// Format unified results to the given writer.
    ///
    /// # Errors
    ///
    /// Returns an IO error if writing fails.
    fn format_unified(
        &self,
        results: &UnifiedResults,
        filter: CycleFilter,
        writer: &mut dyn Write,
    ) -> std::io::Result<()>;
}

/// Human-readable output formatter.
#[derive(Debug, Default)]
pub struct HumanFormatter {
    /// Whether to use colors (for TTY output).
    pub use_color: bool,
    /// Whether to show verbose output (including clean packages).
    pub verbose: bool,
}

impl HumanFormatter {
    /// Create a new human formatter.
    #[inline]
    pub fn new(use_color: bool) -> Self {
        Self {
            use_color,
            verbose: false,
        }
    }

    /// Set verbose mode (show clean packages).
    #[inline]
    #[must_use]
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Format a cycle path as a string.
    fn format_cycle(cycle: &Cycle, base_path: &std::path::Path) -> String {
        cycle
            .path
            .iter()
            .map(|p| {
                p.strip_prefix(base_path)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" -> ")
    }

    /// Format a single package result.
    /// Returns Ok(true) if something was written, Ok(false) if skipped.
    fn format_package(
        &self,
        package: &PackageResult,
        writer: &mut dyn Write,
    ) -> std::io::Result<bool> {
        if package.cycles.is_empty() {
            if self.verbose {
                self.format_clean_package(package, writer)?;
                return Ok(true);
            }
            return Ok(false);
        }
        self.format_cyclic_package(package, writer)?;
        Ok(true)
    }

    /// Format a package with no cycles.
    fn format_clean_package(
        &self,
        package: &PackageResult,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        if self.use_color {
            writeln!(writer, "\x1b[32m✓\x1b[0m {}: no cycles", package.name)
        } else {
            writeln!(writer, "✓ {}: no cycles", package.name)
        }
    }

    /// Format a package with cycles.
    fn format_cyclic_package(
        &self,
        package: &PackageResult,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        let cycle_word = if package.cycles.len() == 1 {
            "cycle"
        } else {
            "cycles"
        };
        let name = &package.name;
        let count = package.cycles.len();

        if self.use_color {
            writeln!(writer, "\x1b[31m✗\x1b[0m {name}: {count} {cycle_word}")?;
        } else {
            writeln!(writer, "✗ {name}: {count} {cycle_word}")?;
        }

        for cycle in &package.cycles {
            let formatted = Self::format_cycle(cycle, &package.path);
            writeln!(writer, "  {formatted}")?;
        }

        Ok(())
    }

    /// Format the summary line.
    fn format_summary(results: &Results, writer: &mut dyn Write) -> std::io::Result<()> {
        let total = results.packages.len();
        let with_cycles = results.packages_with_cycles();
        let total_cycles = results.total_cycles();

        writeln!(
            writer,
            "Summary: {total} packages checked, {with_cycles} with cycles, {total_cycles} total cycles"
        )
    }

    /// Format the package-level cycles section.
    fn format_package_cycles_section(
        &self,
        results: &UnifiedResults,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        // Use detailed cycles if available, otherwise fall back to basic
        if !results.package_cycles_with_files.is_empty() {
            return self.format_package_cycles_with_files(results, writer);
        }

        if results.package_cycles.is_empty() {
            if self.use_color {
                writeln!(writer, "\x1b[32m✓\x1b[0m No inter-package cycles")?;
            } else {
                writeln!(writer, "✓ No inter-package cycles")?;
            }
        } else {
            let count = results.package_cycles.len();
            let cycle_word = if count == 1 { "cycle" } else { "cycles" };
            if self.use_color {
                writeln!(
                    writer,
                    "\x1b[31m✗\x1b[0m {count} inter-package {cycle_word}"
                )?;
            } else {
                writeln!(writer, "✗ {count} inter-package {cycle_word}")?;
            }
            for cycle in &results.package_cycles {
                let formatted = cycle.packages.join(" -> ");
                writeln!(writer, "  {formatted}")?;
            }
        }
        writeln!(writer)
    }

    /// Format package cycles with file-level detail.
    fn format_package_cycles_with_files(
        &self,
        results: &UnifiedResults,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        let count = results.package_cycles_with_files.len();
        if count == 0 {
            if self.use_color {
                writeln!(writer, "\x1b[32m✓\x1b[0m No inter-package cycles")?;
            } else {
                writeln!(writer, "✓ No inter-package cycles")?;
            }
            return writeln!(writer);
        }

        let cycle_word = if count == 1 { "cycle" } else { "cycles" };
        if self.use_color {
            writeln!(
                writer,
                "\x1b[31m✗\x1b[0m {count} inter-package {cycle_word}"
            )?;
        } else {
            writeln!(writer, "✗ {count} inter-package {cycle_word}")?;
        }

        for cycle in &results.package_cycles_with_files {
            Self::format_single_cycle_with_files(cycle, writer)?;
        }

        writeln!(writer)
    }

    /// Format a single cycle with file-level detail.
    fn format_single_cycle_with_files(
        cycle: &PackageCycleWithFiles,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        // Print the cycle summary
        let formatted = cycle.packages.join(" -> ");
        writeln!(writer, "\n  {formatted}")?;

        // Print file details for each edge
        for edge in &cycle.edges {
            Self::format_cycle_edge(edge, writer)?;
        }

        Ok(())
    }

    /// Format a single edge in a cycle.
    fn format_cycle_edge(
        edge: &crate::cycles::PackageCycleEdge,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        writeln!(writer, "    {} -> {}", edge.from_package, edge.to_package)?;
        for (file, subpath) in &edge.files {
            Self::format_edge_file(file, subpath.as_deref(), writer)?;
        }
        Ok(())
    }

    /// Format a single file in an edge.
    fn format_edge_file(
        file: &std::path::Path,
        subpath: Option<&str>,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        let file_display = file.to_string_lossy();
        if let Some(sub) = subpath {
            writeln!(writer, "      - {file_display} (imports /{sub})")
        } else {
            writeln!(writer, "      - {file_display}")
        }
    }

    /// Format the file-level cycles section.
    fn format_file_cycles_section(
        &self,
        results: &UnifiedResults,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        if results.file_results.is_empty() {
            writeln!(writer, "No packages found.")?;
            return Ok(());
        }

        for package in &results.file_results {
            let written = self.format_package(package, writer)?;
            if written {
                writeln!(writer)?;
            }
        }

        Ok(())
    }

    /// Format the unified summary line.
    fn format_unified_summary(
        results: &UnifiedResults,
        filter: CycleFilter,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        let total_pkgs = results.file_results.len();
        let with_file_cycles = results.packages_with_cycles();
        let total_file_cycles = results.total_file_cycles();
        let pkg_cycles = results.package_cycles.len();

        match filter {
            CycleFilter::All => writeln!(
                writer,
                "Summary: {total_pkgs} packages, {with_file_cycles} with file cycles \
                ({total_file_cycles} total), {pkg_cycles} inter-package cycles"
            ),
            CycleFilter::Inner => writeln!(
                writer,
                "Summary: {total_pkgs} packages, {with_file_cycles} with cycles, \
                {total_file_cycles} total file cycles"
            ),
            CycleFilter::Outer => {
                writeln!(writer, "Summary: {pkg_cycles} inter-package cycles")
            },
        }
    }
}

impl OutputFormatter for HumanFormatter {
    fn format(&self, results: &Results, writer: &mut dyn Write) -> std::io::Result<()> {
        if results.packages.is_empty() {
            writeln!(writer, "No packages found.")?;
            return Ok(());
        }

        for package in &results.packages {
            let written = self.format_package(package, writer)?;
            if written {
                writeln!(writer)?;
            }
        }

        Self::format_summary(results, writer)
    }
}

impl UnifiedOutputFormatter for HumanFormatter {
    fn format_unified(
        &self,
        results: &UnifiedResults,
        filter: CycleFilter,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        let show_file = matches!(filter, CycleFilter::All | CycleFilter::Inner);
        let show_package = matches!(filter, CycleFilter::All | CycleFilter::Outer);

        // Package-level cycles section
        if show_package {
            self.format_package_cycles_section(results, writer)?;
        }

        // File-level cycles section
        if show_file {
            self.format_file_cycles_section(results, writer)?;
        }

        // Summary
        Self::format_unified_summary(results, filter, writer)
    }
}

/// JSON output formatter.
#[derive(Debug, Default)]
pub struct JsonFormatter;

/// JSON output structure.
#[derive(Debug, Serialize)]
struct JsonOutput {
    /// Whether any cycles were found.
    has_cycles: bool,

    /// Cycles per package.
    packages: Vec<JsonPackage>,
}

/// JSON package structure.
#[derive(Debug, Serialize)]
struct JsonPackage {
    /// Package name.
    name: String,

    /// Package path.
    path: String,

    /// Detected cycles.
    cycles: Vec<Vec<String>>,
}

impl JsonFormatter {
    /// Create a new JSON formatter.
    #[inline]
    pub fn new() -> Self {
        Self
    }

    /// Convert a package result to JSON format.
    fn convert_package(package: &PackageResult) -> JsonPackage {
        let cycles: Vec<Vec<String>> = package
            .cycles
            .iter()
            .map(|c| Self::convert_cycle(c, &package.path))
            .collect();

        JsonPackage {
            name: package.name.clone(),
            path: package.path.to_string_lossy().to_string(),
            cycles,
        }
    }

    /// Convert a cycle to relative path strings.
    fn convert_cycle(cycle: &Cycle, base_path: &std::path::Path) -> Vec<String> {
        cycle
            .path
            .iter()
            .map(|path| {
                path.strip_prefix(base_path)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string()
            })
            .collect()
    }

    /// Convert a package cycle with files to JSON format.
    fn convert_cycle_with_files(cycle: &PackageCycleWithFiles) -> JsonPackageCycleWithFiles {
        let edges = cycle
            .edges
            .iter()
            .map(|edge| JsonPackageCycleEdge {
                from_package: edge.from_package.clone(),
                to_package: edge.to_package.clone(),
                files: edge
                    .files
                    .iter()
                    .map(|(path, subpath)| JsonEdgeFile {
                        path: path.to_string_lossy().to_string(),
                        subpath: subpath.clone(),
                    })
                    .collect(),
            })
            .collect();

        JsonPackageCycleWithFiles {
            packages: cycle.packages.clone(),
            edges,
        }
    }

    /// Convert a basic package cycle to JSON format (no file detail).
    fn convert_basic_cycle(cycle: &PackageCycle) -> JsonPackageCycleWithFiles {
        // Create edges without file evidence
        let mut edges = Vec::new();
        for window in cycle.packages.windows(2) {
            let (Some(from), Some(to)) = (window.first(), window.get(1)) else {
                continue;
            };
            edges.push(JsonPackageCycleEdge {
                from_package: from.clone(),
                to_package: to.clone(),
                files: Vec::new(),
            });
        }

        JsonPackageCycleWithFiles {
            packages: cycle.packages.clone(),
            edges,
        }
    }
}

impl OutputFormatter for JsonFormatter {
    fn format(&self, results: &Results, writer: &mut dyn Write) -> std::io::Result<()> {
        let packages: Vec<JsonPackage> =
            results.packages.iter().map(Self::convert_package).collect();

        let output = JsonOutput {
            has_cycles: results.has_cycles(),
            packages,
        };

        let json = serde_json::to_string_pretty(&output)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;

        writeln!(writer, "{json}")?;
        Ok(())
    }
}

/// JSON edge with file detail.
#[derive(Debug, Serialize)]
struct JsonPackageCycleEdge {
    /// Source package.
    from_package: String,

    /// Target package.
    to_package: String,

    /// Files creating this edge.
    files: Vec<JsonEdgeFile>,
}

/// JSON file in an edge.
#[derive(Debug, Serialize)]
struct JsonEdgeFile {
    /// Path to the source file.
    path: String,

    /// Optional subpath being imported.
    #[serde(skip_serializing_if = "Option::is_none")]
    subpath: Option<String>,
}

/// JSON package cycle with file detail.
#[derive(Debug, Serialize)]
struct JsonPackageCycleWithFiles {
    /// Packages in the cycle.
    packages: Vec<String>,

    /// Edges with file evidence.
    edges: Vec<JsonPackageCycleEdge>,
}

/// Unified JSON output structure.
#[derive(Debug, Serialize)]
#[expect(
    clippy::struct_field_names,
    reason = "JSON field names are part of API"
)]
struct JsonUnifiedOutput {
    /// Whether any cycles were found.
    has_cycles: bool,

    /// Whether any file-level cycles were found.
    has_file_cycles: bool,

    /// Whether any package-level cycles were found.
    has_package_cycles: bool,

    /// File-level cycles per package.
    file_cycles: Vec<JsonPackage>,

    /// Package-level cycles (with file detail if available).
    package_cycles: Vec<JsonPackageCycleWithFiles>,
}

impl UnifiedOutputFormatter for JsonFormatter {
    fn format_unified(
        &self,
        results: &UnifiedResults,
        filter: CycleFilter,
        writer: &mut dyn Write,
    ) -> std::io::Result<()> {
        let file_cycles: Vec<JsonPackage> = match filter {
            CycleFilter::All | CycleFilter::Inner => results
                .file_results
                .iter()
                .map(Self::convert_package)
                .collect(),
            CycleFilter::Outer => Vec::new(),
        };

        // Use detailed cycles if available, otherwise convert basic cycles
        let package_cycles: Vec<JsonPackageCycleWithFiles> = match filter {
            CycleFilter::All | CycleFilter::Outer => {
                if results.package_cycles_with_files.is_empty() {
                    results
                        .package_cycles
                        .iter()
                        .map(Self::convert_basic_cycle)
                        .collect()
                } else {
                    results
                        .package_cycles_with_files
                        .iter()
                        .map(Self::convert_cycle_with_files)
                        .collect()
                }
            },
            CycleFilter::Inner => Vec::new(),
        };

        let output = JsonUnifiedOutput {
            has_cycles: results.has_any_cycles(),
            has_file_cycles: results.has_file_cycles(),
            has_package_cycles: results.has_package_cycles(),
            file_cycles,
            package_cycles,
        };

        let json = serde_json::to_string_pretty(&output)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;

        writeln!(writer, "{json}")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_results() {
        let results = Results::new();
        assert!(!results.has_cycles(), "empty results should have no cycles");
        assert_eq!(
            results.total_cycles(),
            0,
            "empty results should have 0 total cycles"
        );
    }

    #[test]
    fn test_has_cycles() {
        let mut results = Results::new();
        results.packages.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("a.ts"),
                PathBuf::from("b.ts"),
                PathBuf::from("a.ts"),
            ])],
        });
        assert!(
            results.has_cycles(),
            "results with cycles should report has_cycles"
        );
        assert_eq!(
            results.total_cycles(),
            1,
            "results should have 1 total cycle"
        );
        assert_eq!(
            results.packages_with_cycles(),
            1,
            "results should have 1 package with cycles"
        );
    }

    #[test]
    fn test_human_formatter() {
        let formatter = HumanFormatter::new(false);
        let results = Results::new();
        let mut output = Vec::new();
        formatter
            .format(&results, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");
        assert!(
            output_str.contains("No packages found"),
            "empty results should show 'No packages found'"
        );
    }

    #[test]
    fn test_json_formatter() {
        let formatter = JsonFormatter::new();
        let mut results = Results::new();
        results.packages.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![],
        });

        let mut output = Vec::new();
        formatter
            .format(&results, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");
        assert!(
            output_str.contains("\"has_cycles\": false"),
            "JSON output should contain has_cycles: false"
        );
    }

    // =========================================================================
    // Additional output formatter tests for comprehensive coverage
    // =========================================================================

    #[test]
    fn test_unified_results_new() {
        let results = UnifiedResults::new();
        assert!(
            results.file_results.is_empty(),
            "new unified results should have no file results"
        );
        assert!(
            results.package_cycles.is_empty(),
            "new unified results should have no package cycles"
        );
    }

    #[test]
    fn test_unified_results_has_cycles() {
        let mut results = UnifiedResults::new();
        assert!(
            !results.has_file_cycles(),
            "empty should have no file cycles"
        );
        assert!(
            !results.has_package_cycles(),
            "empty should have no package cycles"
        );
        assert!(!results.has_any_cycles(), "empty should have no cycles");

        // Add file cycle
        results.file_results.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("a.ts"),
                PathBuf::from("b.ts"),
                PathBuf::from("a.ts"),
            ])],
        });

        assert!(
            results.has_file_cycles(),
            "should have file cycles after adding"
        );
        assert!(
            results.has_any_cycles(),
            "should have any cycles after adding file cycle"
        );
        assert!(
            !results.has_package_cycles(),
            "should not have package cycles yet"
        );

        // Add package cycle
        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]));

        assert!(
            results.has_package_cycles(),
            "should have package cycles after adding"
        );
        assert!(
            results.has_any_cycles(),
            "should have any cycles after adding package cycle"
        );
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "test requires multiple assertions")]
    fn test_unified_results_totals() {
        let mut results = UnifiedResults::new();
        assert_eq!(
            results.total_file_cycles(),
            0,
            "empty should have 0 file cycles"
        );
        assert_eq!(
            results.packages_with_cycles(),
            0,
            "empty should have 0 packages with cycles"
        );

        // Add packages with cycles
        results.file_results.push(PackageResult {
            name: "@test/pkg-a".to_owned(),
            path: PathBuf::from("/test/a"),
            cycles: vec![
                Cycle::new(vec![
                    PathBuf::from("a.ts"),
                    PathBuf::from("b.ts"),
                    PathBuf::from("a.ts"),
                ]),
                Cycle::new(vec![
                    PathBuf::from("c.ts"),
                    PathBuf::from("d.ts"),
                    PathBuf::from("c.ts"),
                ]),
            ],
        });

        results.file_results.push(PackageResult {
            name: "@test/pkg-b".to_owned(),
            path: PathBuf::from("/test/b"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("x.ts"),
                PathBuf::from("y.ts"),
                PathBuf::from("x.ts"),
            ])],
        });

        results.file_results.push(PackageResult {
            name: "@test/pkg-c".to_owned(),
            path: PathBuf::from("/test/c"),
            cycles: vec![],
        });

        assert_eq!(
            results.total_file_cycles(),
            3,
            "should have 3 total file cycles"
        );
        assert_eq!(
            results.packages_with_cycles(),
            2,
            "should have 2 packages with cycles"
        );
    }

    #[test]
    fn test_cycle_filter_default() {
        let filter = CycleFilter::default();
        assert_eq!(filter, CycleFilter::All, "default filter should be All");
    }

    #[test]
    fn test_human_formatter_with_cycles() {
        let formatter = HumanFormatter::new(false);
        let mut results = Results::new();
        results.packages.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("/test/a.ts"),
                PathBuf::from("/test/b.ts"),
                PathBuf::from("/test/a.ts"),
            ])],
        });

        let mut output = Vec::new();
        formatter
            .format(&results, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("@test/pkg"),
            "should contain package name"
        );
        assert!(output_str.contains("1 cycle"), "should show cycle count");
        assert!(
            output_str.contains("a.ts") && output_str.contains("b.ts"),
            "should show cycle files"
        );
    }

    #[test]
    fn test_human_formatter_with_color() {
        let formatter = HumanFormatter::new(true).with_verbose(true);
        let mut results = Results::new();
        results.packages.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![],
        });

        let mut output = Vec::new();
        formatter
            .format(&results, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        // Check for ANSI color codes (green for success)
        assert!(
            output_str.contains("\x1b[32m"),
            "should contain green color code for no cycles"
        );
    }

    #[test]
    fn test_json_formatter_with_cycles() {
        let formatter = JsonFormatter::new();
        let mut results = Results::new();
        results.packages.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("/test/a.ts"),
                PathBuf::from("/test/b.ts"),
                PathBuf::from("/test/a.ts"),
            ])],
        });

        let mut output = Vec::new();
        formatter
            .format(&results, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("\"has_cycles\": true"),
            "JSON should report has_cycles: true"
        );
        assert!(
            output_str.contains("@test/pkg"),
            "JSON should contain package name"
        );
        assert!(
            output_str.contains("a.ts"),
            "JSON should contain cycle file"
        );
    }

    #[test]
    fn test_unified_human_formatter_all_filter() {
        let formatter = HumanFormatter::new(false);
        let mut results = UnifiedResults::new();

        results.file_results.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("/test/a.ts"),
                PathBuf::from("/test/b.ts"),
                PathBuf::from("/test/a.ts"),
            ])],
        });

        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]));

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::All, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("inter-package"),
            "should show inter-package cycles with All filter"
        );
        assert!(
            output_str.contains("@test/pkg"),
            "should show file-level package with All filter"
        );
    }

    #[test]
    fn test_unified_human_formatter_inner_filter() {
        let formatter = HumanFormatter::new(false);
        let mut results = UnifiedResults::new();

        results.file_results.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("/test/a.ts"),
                PathBuf::from("/test/b.ts"),
                PathBuf::from("/test/a.ts"),
            ])],
        });

        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]));

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Inner, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("@test/pkg"),
            "should show file-level package with Inner filter"
        );
        assert!(
            output_str.contains("file cycles"),
            "should mention file cycles in summary"
        );
    }

    #[test]
    fn test_unified_human_formatter_outer_filter() {
        let formatter = HumanFormatter::new(false);
        let mut results = UnifiedResults::new();

        results.file_results.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("/test/a.ts"),
                PathBuf::from("/test/b.ts"),
                PathBuf::from("/test/a.ts"),
            ])],
        });

        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]));

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("inter-package"),
            "should show inter-package cycles with Outer filter"
        );
    }

    #[test]
    fn test_unified_json_formatter_all_filter() {
        let formatter = JsonFormatter::new();
        let mut results = UnifiedResults::new();

        results.file_results.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("/test/a.ts"),
                PathBuf::from("/test/b.ts"),
                PathBuf::from("/test/a.ts"),
            ])],
        });

        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]));

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::All, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("\"has_cycles\": true"),
            "JSON should report has_cycles"
        );
        assert!(
            output_str.contains("\"has_file_cycles\": true"),
            "JSON should report has_file_cycles"
        );
        assert!(
            output_str.contains("\"has_package_cycles\": true"),
            "JSON should report has_package_cycles"
        );
        assert!(
            output_str.contains("\"file_cycles\""),
            "JSON should include file_cycles"
        );
        assert!(
            output_str.contains("\"package_cycles\""),
            "JSON should include package_cycles"
        );
    }

    #[test]
    fn test_unified_json_formatter_inner_filter() {
        let formatter = JsonFormatter::new();
        let mut results = UnifiedResults::new();

        results.file_results.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![],
        });

        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]));

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Inner, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        // With Inner filter, package_cycles should be empty in output
        assert!(
            output_str.contains("\"package_cycles\": []"),
            "JSON should have empty package_cycles with Inner filter"
        );
    }

    #[test]
    fn test_unified_json_formatter_outer_filter() {
        let formatter = JsonFormatter::new();
        let mut results = UnifiedResults::new();

        results.file_results.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("a.ts"),
                PathBuf::from("a.ts"),
            ])],
        });

        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]));

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        // With Outer filter, file_cycles should be empty in output
        assert!(
            output_str.contains("\"file_cycles\": []"),
            "JSON should have empty file_cycles with Outer filter"
        );
        assert!(
            output_str.contains("@pkg/a"),
            "JSON should include package cycle"
        );
    }

    #[test]
    fn test_human_formatter_multiple_cycles() {
        let formatter = HumanFormatter::new(false);
        let mut results = Results::new();
        results.packages.push(PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![
                Cycle::new(vec![
                    PathBuf::from("/test/a.ts"),
                    PathBuf::from("/test/b.ts"),
                    PathBuf::from("/test/a.ts"),
                ]),
                Cycle::new(vec![
                    PathBuf::from("/test/c.ts"),
                    PathBuf::from("/test/d.ts"),
                    PathBuf::from("/test/c.ts"),
                ]),
            ],
        });

        let mut output = Vec::new();
        formatter
            .format(&results, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("2 cycles"),
            "should show plural 'cycles' for multiple"
        );
    }

    #[test]
    fn test_results_default() {
        let results = Results::default();
        assert!(
            results.packages.is_empty(),
            "default results should be empty"
        );
        assert!(
            !results.has_cycles(),
            "default results should have no cycles"
        );
    }

    #[test]
    fn test_unified_results_default() {
        let results = UnifiedResults::default();
        assert!(
            results.file_results.is_empty(),
            "default unified results should have no file results"
        );
        assert!(
            results.package_cycles.is_empty(),
            "default unified results should have no package cycles"
        );
    }

    #[test]
    fn test_package_result_clone() {
        let result = PackageResult {
            name: "@test/pkg".to_owned(),
            path: PathBuf::from("/test"),
            cycles: vec![Cycle::new(vec![
                PathBuf::from("a.ts"),
                PathBuf::from("a.ts"),
            ])],
        };

        let cloned = result.clone();
        assert_eq!(cloned.name, result.name, "cloned name should match");
        assert_eq!(cloned.path, result.path, "cloned path should match");
        assert_eq!(
            cloned.cycles.len(),
            result.cycles.len(),
            "cloned cycles should match"
        );
    }

    #[test]
    fn test_human_formatter_no_package_cycles() {
        let formatter = HumanFormatter::new(false);
        let results = UnifiedResults::new();

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("No inter-package cycles"),
            "should show no inter-package cycles message"
        );
    }

    #[test]
    fn test_human_formatter_package_cycle_format() {
        let formatter = HumanFormatter::new(false);
        let mut results = UnifiedResults::new();

        results.package_cycles.push(PackageCycle::new(vec![
            "@myorg/a".to_owned(),
            "@myorg/b".to_owned(),
            "@myorg/c".to_owned(),
            "@myorg/a".to_owned(),
        ]));

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        assert!(
            output_str.contains("@myorg/a -> @myorg/b -> @myorg/c -> @myorg/a"),
            "should format package cycle with arrows"
        );
    }

    // =========================================================================
    // Tests for file-level detail in output
    // =========================================================================

    #[test]
    #[expect(clippy::too_many_lines, reason = "test requires multiple assertions")]
    fn test_human_formatter_cycle_with_files() {
        use crate::cycles::{PackageCycleEdge, PackageCycleWithFiles};

        let formatter = HumanFormatter::new(false);
        let mut results = UnifiedResults::new();

        // Create a cycle with file details
        let mut edge1 = PackageCycleEdge::new("@pkg/a".to_owned(), "@pkg/b".to_owned());
        edge1
            .files
            .push((PathBuf::from("/project/a/index.ts"), None));

        let mut edge2 = PackageCycleEdge::new("@pkg/b".to_owned(), "@pkg/a".to_owned());
        edge2.files.push((
            PathBuf::from("/project/b/utils.ts"),
            Some("helpers".to_owned()),
        ));

        results
            .package_cycles_with_files
            .push(PackageCycleWithFiles {
                packages: vec![
                    "@pkg/a".to_owned(),
                    "@pkg/b".to_owned(),
                    "@pkg/a".to_owned(),
                ],
                edges: vec![edge1, edge2],
            });

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        // Check cycle summary
        assert!(
            output_str.contains("@pkg/a -> @pkg/b -> @pkg/a"),
            "should show cycle summary: {output_str}"
        );

        // Check edge headers
        assert!(
            output_str.contains("@pkg/a -> @pkg/b"),
            "should show first edge: {output_str}"
        );
        assert!(
            output_str.contains("@pkg/b -> @pkg/a"),
            "should show second edge: {output_str}"
        );

        // Check file paths
        assert!(
            output_str.contains("/project/a/index.ts"),
            "should show first file path: {output_str}"
        );
        assert!(
            output_str.contains("/project/b/utils.ts"),
            "should show second file path: {output_str}"
        );

        // Check subpath is shown
        assert!(
            output_str.contains("(imports /helpers)"),
            "should show subpath: {output_str}"
        );
    }

    #[test]
    fn test_human_formatter_cycle_with_files_multiple_files() {
        use crate::cycles::{PackageCycleEdge, PackageCycleWithFiles};

        let formatter = HumanFormatter::new(false);
        let mut results = UnifiedResults::new();

        // Create a cycle with multiple files per edge
        let mut edge1 = PackageCycleEdge::new("@pkg/a".to_owned(), "@pkg/b".to_owned());
        edge1
            .files
            .push((PathBuf::from("/project/a/index.ts"), None));
        edge1.files.push((
            PathBuf::from("/project/a/helpers.ts"),
            Some("utils".to_owned()),
        ));
        edge1
            .files
            .push((PathBuf::from("/project/a/types.ts"), None));

        let mut edge2 = PackageCycleEdge::new("@pkg/b".to_owned(), "@pkg/a".to_owned());
        edge2.files.push((PathBuf::from("/project/b/lib.ts"), None));

        results
            .package_cycles_with_files
            .push(PackageCycleWithFiles {
                packages: vec![
                    "@pkg/a".to_owned(),
                    "@pkg/b".to_owned(),
                    "@pkg/a".to_owned(),
                ],
                edges: vec![edge1, edge2],
            });

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        // Check all files are shown
        assert!(
            output_str.contains("/project/a/index.ts"),
            "should show first file"
        );
        assert!(
            output_str.contains("/project/a/helpers.ts"),
            "should show second file"
        );
        assert!(
            output_str.contains("/project/a/types.ts"),
            "should show third file"
        );
        assert!(
            output_str.contains("/project/b/lib.ts"),
            "should show fourth file"
        );
    }

    #[test]
    fn test_human_formatter_uses_cycles_with_files_when_available() {
        use crate::cycles::{PackageCycleEdge, PackageCycleWithFiles};

        let formatter = HumanFormatter::new(false);
        let mut results = UnifiedResults::new();

        // Add both basic cycles and cycles with files - should use the detailed version
        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/x".to_owned(),
            "@pkg/y".to_owned(),
            "@pkg/x".to_owned(),
        ]));

        let mut edge = PackageCycleEdge::new("@pkg/a".to_owned(), "@pkg/b".to_owned());
        edge.files.push((PathBuf::from("/detailed.ts"), None));

        results
            .package_cycles_with_files
            .push(PackageCycleWithFiles {
                packages: vec![
                    "@pkg/a".to_owned(),
                    "@pkg/b".to_owned(),
                    "@pkg/a".to_owned(),
                ],
                edges: vec![
                    edge,
                    PackageCycleEdge::new("@pkg/b".to_owned(), "@pkg/a".to_owned()),
                ],
            });

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        // Should use detailed format when available
        assert!(
            output_str.contains("/detailed.ts"),
            "should use detailed format when available: {output_str}"
        );
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "test requires multiple assertions")]
    fn test_json_formatter_cycle_with_files() {
        use crate::cycles::{PackageCycleEdge, PackageCycleWithFiles};

        let formatter = JsonFormatter::new();
        let mut results = UnifiedResults::new();

        let mut edge = PackageCycleEdge::new("@pkg/a".to_owned(), "@pkg/b".to_owned());
        edge.files
            .push((PathBuf::from("/src/index.ts"), Some("lib".to_owned())));

        results
            .package_cycles_with_files
            .push(PackageCycleWithFiles {
                packages: vec![
                    "@pkg/a".to_owned(),
                    "@pkg/b".to_owned(),
                    "@pkg/a".to_owned(),
                ],
                edges: vec![
                    edge,
                    PackageCycleEdge::new("@pkg/b".to_owned(), "@pkg/a".to_owned()),
                ],
            });

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        // Parse as JSON to verify structure
        let json: serde_json::Value =
            serde_json::from_str(&output_str).expect("output should be valid JSON");

        assert!(
            json.get("has_package_cycles")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            "has_package_cycles should be true"
        );

        let package_cycles = json
            .get("package_cycles")
            .and_then(|v| v.as_array())
            .expect("package_cycles should be array");
        assert_eq!(package_cycles.len(), 1, "should have 1 cycle");

        let cycle = package_cycles.first().expect("should have first cycle");
        let edges = cycle
            .get("edges")
            .and_then(|v| v.as_array())
            .expect("should have edges");
        assert_eq!(edges.len(), 2, "should have 2 edges");

        let first_edge = edges.first().expect("should have first edge");
        assert_eq!(
            first_edge.get("from_package").and_then(|v| v.as_str()),
            Some("@pkg/a"),
            "from_package should be @pkg/a"
        );
        assert_eq!(
            first_edge.get("to_package").and_then(|v| v.as_str()),
            Some("@pkg/b"),
            "to_package should be @pkg/b"
        );

        let files = first_edge
            .get("files")
            .and_then(|v| v.as_array())
            .expect("should have files");
        assert_eq!(files.len(), 1, "should have 1 file");

        let file = files.first().expect("should have first file");
        assert_eq!(
            file.get("path").and_then(|v| v.as_str()),
            Some("/src/index.ts"),
            "file path should match"
        );
        assert_eq!(
            file.get("subpath").and_then(|v| v.as_str()),
            Some("lib"),
            "file subpath should match"
        );
    }

    #[test]
    fn test_json_formatter_basic_cycle_without_files() {
        let formatter = JsonFormatter::new();
        let mut results = UnifiedResults::new();

        // Only basic cycles, no detailed version
        results.package_cycles.push(PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]));

        let mut output = Vec::new();
        formatter
            .format_unified(&results, CycleFilter::Outer, &mut output)
            .expect("test formatting should succeed");
        let output_str = String::from_utf8(output).expect("test output should be valid UTF-8");

        let json: serde_json::Value =
            serde_json::from_str(&output_str).expect("output should be valid JSON");

        let package_cycles = json
            .get("package_cycles")
            .and_then(|v| v.as_array())
            .expect("package_cycles should be array");
        assert_eq!(package_cycles.len(), 1, "should have 1 cycle");

        let cycle = package_cycles.first().expect("should have first cycle");
        let edges = cycle
            .get("edges")
            .and_then(|v| v.as_array())
            .expect("should have edges");

        // Edges should exist but have empty files
        let first_edge = edges.first().expect("should have first edge");
        let files = first_edge
            .get("files")
            .and_then(|v| v.as_array())
            .expect("should have files array");
        assert!(files.is_empty(), "files should be empty for basic cycle");
    }

    #[test]
    fn test_unified_results_has_package_cycles_with_files() {
        use crate::cycles::{PackageCycleEdge, PackageCycleWithFiles};

        let mut results = UnifiedResults::new();
        assert!(
            !results.has_package_cycles(),
            "empty should have no package cycles"
        );

        // Add cycle with files
        results
            .package_cycles_with_files
            .push(PackageCycleWithFiles {
                packages: vec![
                    "@pkg/a".to_owned(),
                    "@pkg/b".to_owned(),
                    "@pkg/a".to_owned(),
                ],
                edges: vec![PackageCycleEdge::new(
                    "@pkg/a".to_owned(),
                    "@pkg/b".to_owned(),
                )],
            });

        assert!(
            results.has_package_cycles(),
            "should have package cycles after adding cycles_with_files"
        );
        assert!(
            results.has_any_cycles(),
            "should have any cycles after adding"
        );
    }
}
