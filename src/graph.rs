//! Dependency graph construction for cycle detection.
//!
//! Builds directed graphs for both file-level and package-level dependencies.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::imports::{Import, ImportTarget};

/// A directed dependency graph.
#[derive(Debug, Default)]
pub struct DependencyGraph {
    /// Adjacency list: file -> set of files it imports.
    edges: HashMap<PathBuf, HashSet<PathBuf>>,

    /// All nodes in the graph.
    nodes: HashSet<PathBuf>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a dependency graph from extracted imports.
    #[inline]
    pub fn from_imports(imports: &[Import]) -> Self {
        let mut graph = Self::new();

        for import in imports {
            graph.add_node(import.source.clone());

            if let ImportTarget::Resolved(target) = &import.target {
                graph.add_node(target.clone());
                graph.add_edge(import.source.clone(), target.clone());
            }
        }

        graph
    }

    /// Add a node to the graph.
    #[inline]
    pub fn add_node(&mut self, node: PathBuf) {
        let _ = self.nodes.insert(node);
    }

    /// Add a directed edge from source to target.
    #[inline]
    pub fn add_edge(&mut self, from: PathBuf, to: PathBuf) {
        let _ = self.nodes.insert(from.clone());
        let _ = self.nodes.insert(to.clone());
        let _ = self.edges.entry(from).or_default().insert(to);
    }

    /// Get all nodes in the graph.
    #[inline]
    pub fn nodes(&self) -> &HashSet<PathBuf> {
        &self.nodes
    }

    /// Get the files imported by the given file.
    #[inline]
    pub fn successors(&self, node: &Path) -> impl Iterator<Item = &PathBuf> {
        self.edges
            .get(node)
            .map(|set| set.iter())
            .into_iter()
            .flatten()
    }

    /// Check if there's an edge from source to target.
    #[inline]
    pub fn has_edge(&self, from: &Path, to: &Path) -> bool {
        self.edges
            .get(from)
            .is_some_and(|targets| targets.contains(to))
    }

    /// Get the number of nodes in the graph.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges in the graph.
    #[inline]
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(HashSet::len).sum()
    }
}

/// Evidence for a package dependency edge - which files create the import.
#[derive(Debug, Clone, Default)]
pub struct EdgeEvidence {
    /// Files that import from the source package to the target package.
    /// Each entry is (source_file, target_package_subpath).
    files: Vec<(PathBuf, Option<String>)>,
}

impl EdgeEvidence {
    /// Create new empty edge evidence.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a file that contributes to this edge.
    #[inline]
    pub fn add_file(&mut self, source_file: PathBuf, subpath: Option<String>) {
        self.files.push((source_file, subpath));
    }

    /// Get all files that contribute to this edge.
    #[inline]
    pub fn files(&self) -> &[(PathBuf, Option<String>)] {
        &self.files
    }

    /// Get the number of files that contribute to this edge.
    #[inline]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

/// A directed package-level dependency graph.
///
/// Nodes are package names and edges represent inter-package imports.
/// Each edge tracks which files create the dependency.
#[derive(Debug, Default)]
pub struct PackageDependencyGraph {
    /// Adjacency list: package name -> set of packages it imports.
    edges: HashMap<String, HashSet<String>>,

    /// All package names in the graph.
    nodes: HashSet<String>,

    /// Evidence for each edge: (from_pkg, to_pkg) -> files that create this edge.
    edge_evidence: HashMap<(String, String), EdgeEvidence>,
}

impl PackageDependencyGraph {
    /// Create a new empty package dependency graph.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a package dependency graph from imports across multiple packages.
    ///
    /// Takes a mapping of package name to its imports and extracts inter-package
    /// dependencies from `WorkspacePackage` imports.
    #[inline]
    pub fn from_imports(imports_by_package: &HashMap<String, Vec<Import>>) -> Self {
        let mut graph = Self::new();

        // Sort keys for deterministic iteration
        let mut keys: Vec<_> = imports_by_package.keys().collect();
        keys.sort();

        for source_pkg in keys {
            graph.add_node(source_pkg.clone());

            let Some(imports) = imports_by_package.get(source_pkg) else {
                continue;
            };
            for import in imports {
                Self::add_workspace_edge(&mut graph, source_pkg, import);
            }
        }

        graph
    }

    /// Add an edge for a workspace package import.
    fn add_workspace_edge(graph: &mut Self, source_pkg: &str, import: &Import) {
        let ImportTarget::WorkspacePackage {
            package_name,
            subpath,
        } = &import.target
        else {
            return;
        };
        graph.add_node(package_name.clone());
        graph.add_edge_with_evidence(
            source_pkg.to_owned(),
            package_name.clone(),
            import.source.clone(),
            subpath.clone(),
        );
    }

    /// Add a node to the graph.
    #[inline]
    pub fn add_node(&mut self, node: String) {
        let _ = self.nodes.insert(node);
    }

    /// Add a directed edge from source to target.
    #[inline]
    pub fn add_edge(&mut self, from: String, to: String) {
        // Don't add self-edges for packages
        if from == to {
            return;
        }
        let _ = self.nodes.insert(from.clone());
        let _ = self.nodes.insert(to.clone());
        let _ = self.edges.entry(from).or_default().insert(to);
    }

    /// Add a directed edge with file evidence.
    #[inline]
    pub fn add_edge_with_evidence(
        &mut self,
        from: String,
        to: String,
        source_file: PathBuf,
        subpath: Option<String>,
    ) {
        // Don't add self-edges for packages
        if from == to {
            return;
        }
        let _ = self.nodes.insert(from.clone());
        let _ = self.nodes.insert(to.clone());
        let _ = self
            .edges
            .entry(from.clone())
            .or_default()
            .insert(to.clone());

        // Record file evidence for this edge
        self.edge_evidence
            .entry((from, to))
            .or_default()
            .add_file(source_file, subpath);
    }

    /// Get the evidence (source files) for an edge.
    #[inline]
    pub fn edge_evidence(&self, from: &str, to: &str) -> Option<&EdgeEvidence> {
        self.edge_evidence.get(&(from.to_owned(), to.to_owned()))
    }

    /// Get all nodes in the graph.
    #[inline]
    pub fn nodes(&self) -> &HashSet<String> {
        &self.nodes
    }

    /// Get the packages imported by the given package.
    #[inline]
    pub fn successors(&self, node: &str) -> impl Iterator<Item = &String> {
        self.edges
            .get(node)
            .map(|set| set.iter())
            .into_iter()
            .flatten()
    }

    /// Check if there's an edge from source to target.
    #[inline]
    pub fn has_edge(&self, from: &str, to: &str) -> bool {
        self.edges
            .get(from)
            .is_some_and(|targets| targets.contains(to))
    }

    /// Get the number of nodes in the graph.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges in the graph.
    #[inline]
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(HashSet::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_graph() {
        let graph = DependencyGraph::new();
        assert_eq!(graph.node_count(), 0, "empty graph should have 0 nodes");
        assert_eq!(graph.edge_count(), 0, "empty graph should have 0 edges");
    }

    #[test]
    fn test_add_edge() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));

        assert_eq!(
            graph.node_count(),
            2,
            "graph should have 2 nodes after adding edge"
        );
        assert_eq!(graph.edge_count(), 1, "graph should have 1 edge");
        assert!(
            graph.has_edge(Path::new("a.ts"), Path::new("b.ts")),
            "edge a.ts -> b.ts should exist"
        );
        assert!(
            !graph.has_edge(Path::new("b.ts"), Path::new("a.ts")),
            "reverse edge should not exist"
        );
    }

    #[test]
    fn test_successors() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("c.ts"));

        let successor_count = graph.successors(Path::new("a.ts")).count();
        assert_eq!(successor_count, 2, "a.ts should have 2 successors");
    }

    #[test]
    fn test_from_imports() {
        let imports = vec![
            Import {
                source: PathBuf::from("a.ts"),
                target: ImportTarget::Resolved(PathBuf::from("b.ts")),
                specifier: "./b".to_owned(),
            },
            Import {
                source: PathBuf::from("a.ts"),
                target: ImportTarget::External("react".to_owned()),
                specifier: "react".to_owned(),
            },
        ];

        let graph = DependencyGraph::from_imports(&imports);
        assert_eq!(
            graph.node_count(),
            2,
            "graph should have 2 nodes from imports"
        );
        assert_eq!(
            graph.edge_count(),
            1,
            "graph should have 1 edge (external ignored)"
        );
    }

    #[test]
    fn test_package_graph_empty() {
        let graph = PackageDependencyGraph::new();
        assert_eq!(
            graph.node_count(),
            0,
            "empty package graph should have 0 nodes"
        );
        assert_eq!(
            graph.edge_count(),
            0,
            "empty package graph should have 0 edges"
        );
    }

    #[test]
    fn test_package_graph_from_imports() {
        let mut imports_by_package = HashMap::new();
        let _ = imports_by_package.insert(
            "@myorg/app".to_owned(),
            vec![Import {
                source: PathBuf::from("app/index.ts"),
                target: ImportTarget::WorkspacePackage {
                    package_name: "@myorg/utils".to_owned(),
                    subpath: None,
                },
                specifier: "@myorg/utils".to_owned(),
            }],
        );
        let _ = imports_by_package.insert("@myorg/utils".to_owned(), vec![]);

        let graph = PackageDependencyGraph::from_imports(&imports_by_package);
        assert_eq!(graph.node_count(), 2, "package graph should have 2 nodes");
        assert_eq!(graph.edge_count(), 1, "package graph should have 1 edge");
        assert!(
            graph.has_edge("@myorg/app", "@myorg/utils"),
            "edge @myorg/app -> @myorg/utils should exist"
        );
    }

    #[test]
    fn test_package_graph_no_self_loops() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@myorg/app".to_owned(), "@myorg/app".to_owned());
        assert_eq!(
            graph.edge_count(),
            0,
            "self-edges should not be added to package graph"
        );
    }

    // =========================================================================
    // Additional graph tests for comprehensive coverage
    // =========================================================================

    #[test]
    fn test_dependency_graph_multiple_edges_from_single_node() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("main.ts"), PathBuf::from("a.ts"));
        graph.add_edge(PathBuf::from("main.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("main.ts"), PathBuf::from("c.ts"));

        assert_eq!(graph.node_count(), 4, "should have 4 nodes");
        assert_eq!(graph.edge_count(), 3, "should have 3 edges");

        assert_eq!(
            graph.successors(Path::new("main.ts")).count(),
            3,
            "main.ts should have 3 successors"
        );
    }

    #[test]
    fn test_dependency_graph_duplicate_edges() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts")); // duplicate

        assert_eq!(graph.node_count(), 2, "should have 2 nodes");
        assert_eq!(graph.edge_count(), 1, "duplicate edge should be ignored");
    }

    #[test]
    fn test_dependency_graph_bidirectional() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("a.ts"));

        assert_eq!(graph.node_count(), 2, "should have 2 nodes");
        assert_eq!(graph.edge_count(), 2, "should have 2 directional edges");
        assert!(
            graph.has_edge(Path::new("a.ts"), Path::new("b.ts")),
            "a -> b should exist"
        );
        assert!(
            graph.has_edge(Path::new("b.ts"), Path::new("a.ts")),
            "b -> a should exist"
        );
    }

    #[test]
    fn test_dependency_graph_self_loop() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("recursive.ts"), PathBuf::from("recursive.ts"));

        assert_eq!(graph.node_count(), 1, "should have 1 node");
        assert_eq!(graph.edge_count(), 1, "self-loop should count as 1 edge");
        assert!(
            graph.has_edge(Path::new("recursive.ts"), Path::new("recursive.ts")),
            "self-loop should exist"
        );
    }

    #[test]
    fn test_dependency_graph_isolated_nodes() {
        let mut graph = DependencyGraph::new();
        graph.add_node(PathBuf::from("isolated.ts"));
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));

        assert_eq!(
            graph.node_count(),
            3,
            "should have 3 nodes including isolated"
        );
        assert_eq!(graph.edge_count(), 1, "isolated node adds no edges");

        assert!(
            graph.successors(Path::new("isolated.ts")).next().is_none(),
            "isolated node should have no successors"
        );
    }

    #[test]
    fn test_dependency_graph_from_imports_filters_external() {
        let imports = vec![
            Import {
                source: PathBuf::from("app.ts"),
                target: ImportTarget::Resolved(PathBuf::from("utils.ts")),
                specifier: "./utils".to_owned(),
            },
            Import {
                source: PathBuf::from("app.ts"),
                target: ImportTarget::External("react".to_owned()),
                specifier: "react".to_owned(),
            },
            Import {
                source: PathBuf::from("app.ts"),
                target: ImportTarget::External("lodash".to_owned()),
                specifier: "lodash".to_owned(),
            },
        ];

        let graph = DependencyGraph::from_imports(&imports);
        assert_eq!(
            graph.node_count(),
            2,
            "should only include resolved imports as nodes"
        );
        assert_eq!(
            graph.edge_count(),
            1,
            "should only have edge for resolved import"
        );
    }

    #[test]
    fn test_dependency_graph_from_imports_unresolved() {
        let imports = vec![Import {
            source: PathBuf::from("app.ts"),
            target: ImportTarget::Unresolved("./missing".to_owned()),
            specifier: "./missing".to_owned(),
        }];

        let graph = DependencyGraph::from_imports(&imports);
        assert_eq!(graph.node_count(), 1, "only source file should be a node");
        assert_eq!(
            graph.edge_count(),
            0,
            "unresolved import should not create edge"
        );
    }

    #[test]
    fn test_dependency_graph_complex_chain() {
        // A -> B -> C -> D -> E
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("c.ts"));
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("d.ts"));
        graph.add_edge(PathBuf::from("d.ts"), PathBuf::from("e.ts"));

        assert_eq!(graph.node_count(), 5, "should have 5 nodes in chain");
        assert_eq!(graph.edge_count(), 4, "should have 4 edges in chain");

        // Test non-transitive: A should not have edge to C
        assert!(
            !graph.has_edge(Path::new("a.ts"), Path::new("c.ts")),
            "no transitive edge"
        );
    }

    #[test]
    fn test_package_graph_complex_structure() {
        let mut graph = PackageDependencyGraph::new();

        // @app imports @utils and @shared
        graph.add_edge("@myorg/app".to_owned(), "@myorg/utils".to_owned());
        graph.add_edge("@myorg/app".to_owned(), "@myorg/shared".to_owned());

        // @utils imports @shared
        graph.add_edge("@myorg/utils".to_owned(), "@myorg/shared".to_owned());

        // @shared imports @core
        graph.add_edge("@myorg/shared".to_owned(), "@myorg/core".to_owned());

        assert_eq!(graph.node_count(), 4, "should have 4 package nodes");
        assert_eq!(graph.edge_count(), 4, "should have 4 inter-package edges");

        assert!(
            graph.has_edge("@myorg/app", "@myorg/utils"),
            "app -> utils should exist"
        );
        assert!(
            graph.has_edge("@myorg/shared", "@myorg/core"),
            "shared -> core should exist"
        );
    }

    #[test]
    fn test_package_graph_bidirectional_cycle() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@myorg/a".to_owned(), "@myorg/b".to_owned());
        graph.add_edge("@myorg/b".to_owned(), "@myorg/a".to_owned());

        assert_eq!(graph.node_count(), 2, "should have 2 nodes");
        assert_eq!(graph.edge_count(), 2, "bidirectional edges");
        assert!(graph.has_edge("@myorg/a", "@myorg/b"), "a -> b");
        assert!(graph.has_edge("@myorg/b", "@myorg/a"), "b -> a");
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test setup requires multiple imports"
    )]
    fn test_package_graph_from_imports_with_workspace_packages() {
        let mut imports_by_package = HashMap::new();

        // Package @myorg/app imports @myorg/utils and @myorg/shared
        let _ = imports_by_package.insert(
            "@myorg/app".to_owned(),
            vec![
                Import {
                    source: PathBuf::from("app/index.ts"),
                    target: ImportTarget::WorkspacePackage {
                        package_name: "@myorg/utils".to_owned(),
                        subpath: None,
                    },
                    specifier: "@myorg/utils".to_owned(),
                },
                Import {
                    source: PathBuf::from("app/index.ts"),
                    target: ImportTarget::WorkspacePackage {
                        package_name: "@myorg/shared".to_owned(),
                        subpath: Some("helpers".to_owned()),
                    },
                    specifier: "@myorg/shared/helpers".to_owned(),
                },
                Import {
                    source: PathBuf::from("app/index.ts"),
                    target: ImportTarget::External("react".to_owned()),
                    specifier: "react".to_owned(),
                },
            ],
        );

        // Package @myorg/utils imports @myorg/shared
        let _ = imports_by_package.insert(
            "@myorg/utils".to_owned(),
            vec![Import {
                source: PathBuf::from("utils/index.ts"),
                target: ImportTarget::WorkspacePackage {
                    package_name: "@myorg/shared".to_owned(),
                    subpath: None,
                },
                specifier: "@myorg/shared".to_owned(),
            }],
        );

        // Package @myorg/shared has no inter-package imports
        let _ = imports_by_package.insert("@myorg/shared".to_owned(), vec![]);

        let graph = PackageDependencyGraph::from_imports(&imports_by_package);

        assert_eq!(graph.node_count(), 3, "should have 3 package nodes");
        assert_eq!(graph.edge_count(), 3, "should have 3 inter-package edges");

        assert!(graph.has_edge("@myorg/app", "@myorg/utils"), "app -> utils");
        assert!(
            graph.has_edge("@myorg/app", "@myorg/shared"),
            "app -> shared"
        );
        assert!(
            graph.has_edge("@myorg/utils", "@myorg/shared"),
            "utils -> shared"
        );
    }

    #[test]
    fn test_package_graph_ignores_self_imports() {
        let mut imports_by_package = HashMap::new();

        // Package @myorg/utils imports itself (common for internal re-exports)
        let _ = imports_by_package.insert(
            "@myorg/utils".to_owned(),
            vec![
                Import {
                    source: PathBuf::from("utils/helpers.ts"),
                    target: ImportTarget::WorkspacePackage {
                        package_name: "@myorg/utils".to_owned(),
                        subpath: Some("types".to_owned()),
                    },
                    specifier: "@myorg/utils/types".to_owned(),
                },
                Import {
                    source: PathBuf::from("utils/index.ts"),
                    target: ImportTarget::WorkspacePackage {
                        package_name: "@myorg/shared".to_owned(),
                        subpath: None,
                    },
                    specifier: "@myorg/shared".to_owned(),
                },
            ],
        );

        let _ = imports_by_package.insert("@myorg/shared".to_owned(), vec![]);

        let graph = PackageDependencyGraph::from_imports(&imports_by_package);

        assert_eq!(graph.node_count(), 2, "should have 2 nodes");
        assert_eq!(
            graph.edge_count(),
            1,
            "self-import should be ignored, only utils->shared"
        );
        assert!(
            !graph.has_edge("@myorg/utils", "@myorg/utils"),
            "no self-edge"
        );
        assert!(
            graph.has_edge("@myorg/utils", "@myorg/shared"),
            "utils -> shared"
        );
    }

    #[test]
    fn test_package_graph_successors() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@root/app".to_owned(), "@root/a".to_owned());
        graph.add_edge("@root/app".to_owned(), "@root/b".to_owned());
        graph.add_edge("@root/app".to_owned(), "@root/c".to_owned());
        graph.add_node("@root/isolated".to_owned());

        let mut app_successors: Vec<_> = graph.successors("@root/app").cloned().collect();
        app_successors.sort();
        assert_eq!(
            app_successors,
            vec!["@root/a", "@root/b", "@root/c"],
            "app should have 3 successors"
        );

        assert!(
            graph.successors("@root/isolated").next().is_none(),
            "isolated node has no successors"
        );

        assert!(
            graph.successors("@root/nonexistent").next().is_none(),
            "nonexistent node has no successors"
        );
    }

    #[test]
    fn test_package_graph_duplicate_edges() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@myorg/a".to_owned(), "@myorg/b".to_owned());
        graph.add_edge("@myorg/a".to_owned(), "@myorg/b".to_owned()); // duplicate

        assert_eq!(graph.node_count(), 2, "should have 2 nodes");
        assert_eq!(graph.edge_count(), 1, "duplicate edge should be ignored");
    }

    // =========================================================================
    // Tests for EdgeEvidence and file-level tracking
    // =========================================================================

    #[test]
    fn test_edge_evidence_new() {
        let evidence = EdgeEvidence::new();
        assert_eq!(evidence.file_count(), 0, "new evidence should be empty");
        assert!(evidence.files().is_empty(), "files should be empty");
    }

    #[test]
    fn test_edge_evidence_add_file() {
        let mut evidence = EdgeEvidence::new();
        evidence.add_file(PathBuf::from("src/index.ts"), None);
        evidence.add_file(PathBuf::from("src/utils.ts"), Some("helpers".to_owned()));

        assert_eq!(evidence.file_count(), 2, "should have 2 files");

        let files = evidence.files();
        assert_eq!(files.len(), 2, "files slice should have 2 entries");

        assert_eq!(
            files.first().map(|(p, _)| p.to_string_lossy().to_string()),
            Some("src/index.ts".to_owned()),
            "first file path should match"
        );
        assert_eq!(
            files.first().map(|(_, s)| s.clone()),
            Some(None),
            "first file should have no subpath"
        );

        assert_eq!(
            files.get(1).map(|(p, _)| p.to_string_lossy().to_string()),
            Some("src/utils.ts".to_owned()),
            "second file path should match"
        );
        assert_eq!(
            files.get(1).and_then(|(_, s)| s.clone()),
            Some("helpers".to_owned()),
            "second file should have subpath"
        );
    }

    #[test]
    fn test_edge_evidence_clone() {
        let mut evidence = EdgeEvidence::new();
        evidence.add_file(PathBuf::from("a.ts"), None);

        let cloned = evidence.clone();
        assert_eq!(
            cloned.file_count(),
            evidence.file_count(),
            "cloned evidence should have same count"
        );
    }

    #[test]
    fn test_add_edge_with_evidence() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            PathBuf::from("a/index.ts"),
            None,
        );

        assert_eq!(graph.node_count(), 2, "should have 2 nodes");
        assert_eq!(graph.edge_count(), 1, "should have 1 edge");
        assert!(graph.has_edge("@pkg/a", "@pkg/b"), "edge should exist");

        let evidence = graph.edge_evidence("@pkg/a", "@pkg/b");
        assert!(evidence.is_some(), "evidence should exist");
        assert_eq!(
            evidence.map(EdgeEvidence::file_count),
            Some(1),
            "should have 1 file"
        );
    }

    #[test]
    fn test_add_edge_with_evidence_multiple_files() {
        let mut graph = PackageDependencyGraph::new();

        // Add same edge from different files
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            PathBuf::from("a/index.ts"),
            None,
        );
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            PathBuf::from("a/utils.ts"),
            Some("helpers".to_owned()),
        );

        assert_eq!(graph.edge_count(), 1, "should still have 1 edge");

        let evidence = graph.edge_evidence("@pkg/a", "@pkg/b");
        assert_eq!(
            evidence.map(EdgeEvidence::file_count),
            Some(2),
            "should have 2 files contributing to edge"
        );
    }

    #[test]
    fn test_add_edge_with_evidence_ignores_self_edges() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/a".to_owned(),
            PathBuf::from("a/index.ts"),
            None,
        );

        assert_eq!(graph.edge_count(), 0, "self-edge should be ignored");
        assert!(
            graph.edge_evidence("@pkg/a", "@pkg/a").is_none(),
            "no evidence for self-edge"
        );
    }

    #[test]
    fn test_edge_evidence_not_found() {
        let graph = PackageDependencyGraph::new();
        assert!(
            graph.edge_evidence("@pkg/a", "@pkg/b").is_none(),
            "no evidence for non-existent edge"
        );
    }

    #[test]
    fn test_from_imports_records_evidence() {
        let mut imports_by_package = HashMap::new();

        let _ = imports_by_package.insert(
            "@myorg/app".to_owned(),
            vec![
                Import {
                    source: PathBuf::from("app/index.ts"),
                    target: ImportTarget::WorkspacePackage {
                        package_name: "@myorg/utils".to_owned(),
                        subpath: None,
                    },
                    specifier: "@myorg/utils".to_owned(),
                },
                Import {
                    source: PathBuf::from("app/helpers.ts"),
                    target: ImportTarget::WorkspacePackage {
                        package_name: "@myorg/utils".to_owned(),
                        subpath: Some("helpers".to_owned()),
                    },
                    specifier: "@myorg/utils/helpers".to_owned(),
                },
            ],
        );
        let _ = imports_by_package.insert("@myorg/utils".to_owned(), vec![]);

        let graph = PackageDependencyGraph::from_imports(&imports_by_package);

        let evidence = graph.edge_evidence("@myorg/app", "@myorg/utils");
        assert!(evidence.is_some(), "evidence should exist for edge");
        assert_eq!(
            evidence.map(EdgeEvidence::file_count),
            Some(2),
            "should have 2 files"
        );

        let files = evidence.map(EdgeEvidence::files).unwrap_or(&[]);
        assert_eq!(files.len(), 2, "should have 2 file entries");

        // Check subpaths are recorded
        let has_none_subpath = files.iter().any(|(_, s)| s.is_none());
        let has_helpers_subpath = files.iter().any(|(_, s)| s.as_deref() == Some("helpers"));

        assert!(has_none_subpath, "should have file with no subpath");
        assert!(has_helpers_subpath, "should have file with helpers subpath");
    }
}
