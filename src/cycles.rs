//! Cycle detection using Tarjan's strongly connected components algorithm.
//!
//! Finds all circular dependencies in both file-level and package-level graphs.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::graph::{DependencyGraph, EdgeEvidence, PackageDependencyGraph};

/// State container for Tarjan's algorithm.
///
/// Encapsulates all mutable state needed during the algorithm execution,
/// reducing parameter count in recursive calls.
struct TarjanState {
    /// Current index counter for node discovery.
    index_counter: usize,
    /// Stack of nodes in the current DFS path.
    stack: Vec<PathBuf>,
    /// Set of nodes currently on the stack (for O(1) lookup).
    on_stack: HashSet<PathBuf>,
    /// Discovery index for each node.
    index: HashMap<PathBuf, usize>,
    /// Lowest reachable index for each node.
    lowlink: HashMap<PathBuf, usize>,
    /// Collected strongly connected components.
    sccs: Vec<Vec<PathBuf>>,
}

impl TarjanState {
    /// Create a new empty state.
    #[inline]
    fn new() -> Self {
        Self {
            index_counter: 0,
            stack: Vec::new(),
            on_stack: HashSet::new(),
            index: HashMap::new(),
            lowlink: HashMap::new(),
            sccs: Vec::new(),
        }
    }
}

/// A circular dependency cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    /// Files in the cycle, starting and ending with the same file.
    /// e.g., `[a.ts, b.ts, c.ts, a.ts]`
    pub path: Vec<PathBuf>,
}

impl Cycle {
    /// Create a new cycle from a path.
    #[inline]
    pub fn new(path: Vec<PathBuf>) -> Self {
        Self { path }
    }

    /// Get the length of the cycle (excluding the repeated first element).
    #[inline]
    pub fn len(&self) -> usize {
        if self.path.is_empty() {
            0
        } else {
            self.path.len().saturating_sub(1)
        }
    }

    /// Check if the cycle is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }
}

/// Cycle detection engine using Tarjan's SCC algorithm.
pub struct CycleDetector;

impl CycleDetector {
    /// Find all cycles in the dependency graph.
    #[inline]
    pub fn detect(graph: &DependencyGraph) -> Vec<Cycle> {
        let sccs = Self::tarjan(graph);
        Self::extract_cycles(graph, sccs)
    }

    /// Tarjan's strongly connected components algorithm.
    fn tarjan(graph: &DependencyGraph) -> Vec<Vec<PathBuf>> {
        let mut state = TarjanState::new();

        // Sort nodes for deterministic output
        let mut nodes: Vec<_> = graph.nodes().iter().cloned().collect();
        nodes.sort();

        for node in &nodes {
            if !state.index.contains_key(node) {
                Self::strongconnect(graph, node, &mut state);
            }
        }

        state.sccs
    }

    /// Helper function for Tarjan's algorithm.
    fn strongconnect(graph: &DependencyGraph, node: &Path, state: &mut TarjanState) {
        // Set the depth index for this node
        let _ = state.index.insert(node.to_path_buf(), state.index_counter);
        let _ = state
            .lowlink
            .insert(node.to_path_buf(), state.index_counter);
        state.index_counter = state.index_counter.saturating_add(1);
        state.stack.push(node.to_path_buf());
        let _ = state.on_stack.insert(node.to_path_buf());

        // Consider successors
        let mut successors: Vec<_> = graph.successors(node).cloned().collect();
        successors.sort();

        for successor in successors {
            if !state.index.contains_key(&successor) {
                // Successor has not been visited; recurse
                Self::strongconnect(graph, &successor, state);
                // Update lowlink
                let node_lowlink = state.lowlink.get(node).copied().unwrap_or(usize::MAX);
                let successor_lowlink =
                    state.lowlink.get(&successor).copied().unwrap_or(usize::MAX);
                let _ = state
                    .lowlink
                    .insert(node.to_path_buf(), node_lowlink.min(successor_lowlink));
            } else if state.on_stack.contains(&successor) {
                // Successor is on the stack, hence in the current SCC
                let node_lowlink = state.lowlink.get(node).copied().unwrap_or(usize::MAX);
                let successor_index = state.index.get(&successor).copied().unwrap_or(usize::MAX);
                let _ = state
                    .lowlink
                    .insert(node.to_path_buf(), node_lowlink.min(successor_index));
            } else {
                // Successor already processed, not part of current SCC path
            }
        }

        // If node is a root node, pop the stack and generate an SCC
        let node_index = state.index.get(node).copied().unwrap_or(0);
        let node_lowlink = state.lowlink.get(node).copied().unwrap_or(0);

        if node_lowlink == node_index {
            let scc = Self::pop_scc_from_stack(node, state);
            state.sccs.push(scc);
        }
    }

    /// Pop a strongly connected component from the stack.
    fn pop_scc_from_stack(node: &Path, state: &mut TarjanState) -> Vec<PathBuf> {
        let mut scc = Vec::new();
        while let Some(w) = state.stack.pop() {
            let _ = state.on_stack.remove(&w);
            let is_root = w == node;
            scc.push(w);
            if is_root {
                break;
            }
        }
        scc
    }

    /// Extract actual cycles from strongly connected components.
    fn extract_cycles(graph: &DependencyGraph, sccs: Vec<Vec<PathBuf>>) -> Vec<Cycle> {
        let mut cycles = Vec::new();

        for scc in sccs {
            if let Some(cycle) = Self::extract_cycle_from_scc(graph, &scc) {
                cycles.push(cycle);
            }
        }

        // Sort cycles for deterministic output
        cycles.sort_by(|a, b| a.path.cmp(&b.path));
        cycles
    }

    /// Extract a cycle from a single SCC.
    fn extract_cycle_from_scc(graph: &DependencyGraph, scc: &[PathBuf]) -> Option<Cycle> {
        if scc.len() > 1 {
            // SCC with multiple nodes contains cycles
            return Self::find_cycle_in_scc(graph, scc).map(Self::normalize);
        }
        // Single node - check for self-loop
        let node = scc.first()?;
        if graph.has_edge(node, node) {
            return Some(Cycle::new(vec![node.clone(), node.clone()]));
        }
        None
    }

    /// Find a cycle within an SCC using DFS.
    fn find_cycle_in_scc(graph: &DependencyGraph, scc: &[PathBuf]) -> Option<Vec<PathBuf>> {
        let scc_set: HashSet<_> = scc.iter().collect();

        // Start from the lexicographically smallest node for determinism
        let mut sorted_scc = scc.to_vec();
        sorted_scc.sort();

        let start = sorted_scc.first()?;

        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut path: Vec<PathBuf> = Vec::new();

        Self::dfs_find_cycle(graph, start, start, &scc_set, &mut visited, &mut path)
    }

    /// DFS helper to find a cycle back to the start node.
    fn dfs_find_cycle(
        graph: &DependencyGraph,
        current: &Path,
        start: &Path,
        scc_set: &HashSet<&PathBuf>,
        visited: &mut HashSet<PathBuf>,
        path: &mut Vec<PathBuf>,
    ) -> Option<Vec<PathBuf>> {
        path.push(current.to_path_buf());
        let _ = visited.insert(current.to_path_buf());

        let mut successors: Vec<_> = graph
            .successors(current)
            .filter(|s| scc_set.contains(s))
            .cloned()
            .collect();
        successors.sort();

        let result =
            Self::try_find_cycle_in_successors(graph, start, scc_set, visited, path, &successors);

        if result.is_none() {
            drop(path.pop());
        }
        result
    }

    /// Try to find a cycle by exploring successors.
    fn try_find_cycle_in_successors(
        graph: &DependencyGraph,
        start: &Path,
        scc_set: &HashSet<&PathBuf>,
        visited: &mut HashSet<PathBuf>,
        path: &mut Vec<PathBuf>,
        successors: &[PathBuf],
    ) -> Option<Vec<PathBuf>> {
        for successor in successors {
            if let Some(cycle) =
                Self::check_successor(graph, successor, start, scc_set, visited, path)
            {
                return Some(cycle);
            }
        }
        None
    }

    /// Check a single successor for cycles.
    fn check_successor(
        graph: &DependencyGraph,
        successor: &PathBuf,
        start: &Path,
        scc_set: &HashSet<&PathBuf>,
        visited: &mut HashSet<PathBuf>,
        path: &mut Vec<PathBuf>,
    ) -> Option<Vec<PathBuf>> {
        if successor.as_path() == start && path.len() > 1 {
            // Found a cycle back to start
            let mut cycle = path.clone();
            cycle.push(start.to_path_buf());
            return Some(cycle);
        }
        if !visited.contains(successor) {
            return Self::dfs_find_cycle(graph, successor, start, scc_set, visited, path);
        }
        None
    }

    /// Normalize a cycle to start from the lexicographically smallest node.
    fn normalize(mut cycle: Vec<PathBuf>) -> Cycle {
        if cycle.len() <= 1 {
            return Cycle::new(cycle);
        }

        // Remove the last element (duplicate of first)
        let last = cycle.pop();

        // Find the index of the smallest element
        let min_idx = cycle
            .iter()
            .enumerate()
            .min_by_key(|(_, path)| *path)
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // Rotate to start from the smallest
        cycle.rotate_left(min_idx);

        // Add the first element back at the end
        if let Some(first) = cycle.first().cloned() {
            cycle.push(first);
        } else if let Some(l) = last {
            cycle.push(l);
        } else {
            // Both are None, leave cycle as is
        }

        Cycle::new(cycle)
    }
}

/// A circular dependency cycle between packages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCycle {
    /// Package names in the cycle, starting and ending with the same package.
    /// e.g., `["@myorg/a", "@myorg/b", "@myorg/c", "@myorg/a"]`
    pub packages: Vec<String>,
}

impl PackageCycle {
    /// Create a new package cycle from a list of packages.
    #[inline]
    pub fn new(packages: Vec<String>) -> Self {
        Self { packages }
    }

    /// Get the length of the cycle (excluding the repeated first element).
    #[inline]
    pub fn len(&self) -> usize {
        if self.packages.is_empty() {
            0
        } else {
            self.packages.len().saturating_sub(1)
        }
    }

    /// Check if the cycle is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

/// An edge in a package cycle with file-level evidence.
#[derive(Debug, Clone)]
pub struct PackageCycleEdge {
    /// Source package name.
    pub from_package: String,

    /// Target package name.
    pub to_package: String,

    /// Files in the source package that import the target package.
    /// Each entry is (source_file, optional_subpath).
    pub files: Vec<(PathBuf, Option<String>)>,
}

impl PackageCycleEdge {
    /// Create a new package cycle edge.
    #[inline]
    pub fn new(from_package: String, to_package: String) -> Self {
        Self {
            from_package,
            to_package,
            files: Vec::new(),
        }
    }

    /// Create a package cycle edge with file evidence.
    #[inline]
    pub fn with_evidence(
        from_package: String,
        to_package: String,
        evidence: &EdgeEvidence,
    ) -> Self {
        Self {
            from_package,
            to_package,
            files: evidence.files().to_vec(),
        }
    }
}

/// A circular dependency cycle between packages with file-level detail.
///
/// This extends `PackageCycle` to include which specific files create
/// each edge in the cycle.
#[derive(Debug, Clone)]
pub struct PackageCycleWithFiles {
    /// The package names in the cycle (same as `PackageCycle.packages`).
    pub packages: Vec<String>,

    /// Edges in the cycle with file evidence.
    /// Each edge shows which files in `from_package` import `to_package`.
    pub edges: Vec<PackageCycleEdge>,
}

impl PackageCycleWithFiles {
    /// Create a new package cycle with files from a basic cycle and graph.
    pub fn from_cycle(cycle: &PackageCycle, graph: &PackageDependencyGraph) -> Self {
        let mut edges = Vec::new();

        // Iterate through consecutive pairs in the cycle
        // packages = [A, B, C, A] means edges: A->B, B->C, C->A
        for window in cycle.packages.windows(2) {
            let (Some(from), Some(to)) = (window.first(), window.get(1)) else {
                continue;
            };

            let edge = graph.edge_evidence(from, to).map_or_else(
                || PackageCycleEdge::new(from.clone(), to.clone()),
                |evidence| PackageCycleEdge::with_evidence(from.clone(), to.clone(), evidence),
            );
            edges.push(edge);
        }

        Self {
            packages: cycle.packages.clone(),
            edges,
        }
    }

    /// Get the length of the cycle (excluding the repeated first element).
    #[inline]
    pub fn len(&self) -> usize {
        if self.packages.is_empty() {
            0
        } else {
            self.packages.len().saturating_sub(1)
        }
    }

    /// Check if the cycle is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

/// State container for package-level Tarjan's algorithm.
struct PackageTarjanState {
    /// Current index counter for node discovery.
    index_counter: usize,
    /// Stack of nodes in the current DFS path.
    stack: Vec<String>,
    /// Set of nodes currently on the stack (for O(1) lookup).
    on_stack: HashSet<String>,
    /// Discovery index for each node.
    index: HashMap<String, usize>,
    /// Lowest reachable index for each node.
    lowlink: HashMap<String, usize>,
    /// Collected strongly connected components.
    sccs: Vec<Vec<String>>,
}

impl PackageTarjanState {
    /// Create a new empty state.
    #[inline]
    fn new() -> Self {
        Self {
            index_counter: 0,
            stack: Vec::new(),
            on_stack: HashSet::new(),
            index: HashMap::new(),
            lowlink: HashMap::new(),
            sccs: Vec::new(),
        }
    }
}

/// Cycle detection engine for package-level dependencies.
///
/// Uses a combination of:
/// 1. Direct 2-cycle detection (A <-> B)
/// 2. Johnson's algorithm for finding all elementary cycles
pub struct PackageCycleDetector;

impl PackageCycleDetector {
    /// Find all cycles in the package dependency graph.
    ///
    /// Returns all elementary cycles (simple cycles without repeated nodes).
    #[inline]
    pub fn detect(graph: &PackageDependencyGraph) -> Vec<PackageCycle> {
        let mut all_cycles = Vec::new();

        // Find 2-cycles first (most common case)
        let two_cycles = Self::find_two_cycles(graph);
        all_cycles.extend(two_cycles);

        // Find longer cycles using Johnson's algorithm on SCCs
        let longer_cycles = Self::find_longer_cycles(graph);
        all_cycles.extend(longer_cycles);

        // Deduplicate (normalized cycles are comparable)
        all_cycles.sort_by(|a, b| a.packages.cmp(&b.packages));
        all_cycles.dedup();

        all_cycles
    }

    /// Find all cycles with file-level detail.
    ///
    /// Returns cycles including which files create each edge.
    #[inline]
    pub fn detect_with_files(graph: &PackageDependencyGraph) -> Vec<PackageCycleWithFiles> {
        let cycles = Self::detect(graph);
        cycles
            .iter()
            .map(|cycle| PackageCycleWithFiles::from_cycle(cycle, graph))
            .collect()
    }

    /// Find all 2-cycles (bidirectional edges: A -> B and B -> A).
    fn find_two_cycles(graph: &PackageDependencyGraph) -> Vec<PackageCycle> {
        let mut cycles = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();

        // Sort nodes for deterministic output
        let mut nodes: Vec<_> = graph.nodes().iter().cloned().collect();
        nodes.sort();

        for node in &nodes {
            Self::find_two_cycles_for_node(graph, node, &mut seen, &mut cycles);
        }

        cycles
    }

    /// Find 2-cycles originating from a specific node.
    fn find_two_cycles_for_node(
        graph: &PackageDependencyGraph,
        node: &str,
        seen: &mut HashSet<(String, String)>,
        cycles: &mut Vec<PackageCycle>,
    ) {
        for successor in graph.successors(node) {
            // Check if successor also points back to node
            if !graph.has_edge(successor, node) {
                continue;
            }

            let pair = Self::normalize_pair(node, successor);
            if seen.contains(&pair) {
                continue;
            }
            let _ = seen.insert(pair.clone());

            // Cycle representation: A -> B -> A
            cycles.push(PackageCycle::new(vec![
                pair.0.clone(),
                pair.1.clone(),
                pair.0,
            ]));
        }
    }

    /// Normalize a node pair to have the smaller one first.
    fn normalize_pair(node_a: &str, node_b: &str) -> (String, String) {
        if node_a < node_b {
            (node_a.to_owned(), node_b.to_owned())
        } else {
            (node_b.to_owned(), node_a.to_owned())
        }
    }

    /// Find cycles longer than 2 nodes using Johnson's algorithm.
    fn find_longer_cycles(graph: &PackageDependencyGraph) -> Vec<PackageCycle> {
        let sccs = Self::tarjan(graph);
        let mut cycles = Vec::new();

        for scc in sccs {
            Self::find_longer_cycles_in_scc(graph, &scc, &mut cycles);
        }

        cycles
    }

    /// Find longer cycles within a single SCC.
    fn find_longer_cycles_in_scc(
        graph: &PackageDependencyGraph,
        scc: &[String],
        cycles: &mut Vec<PackageCycle>,
    ) {
        // Skip small SCCs (2-cycles already handled)
        if scc.len() <= 2 {
            return;
        }

        // Find all cycles in this SCC using Johnson's algorithm
        let scc_cycles = Self::johnson_all_cycles(graph, scc);
        for cycle in scc_cycles {
            // Skip 2-cycles (already found) - len > 3 means at least 3 distinct nodes
            if cycle.len() <= 3 {
                continue;
            }
            cycles.push(Self::normalize(cycle));
        }
    }

    /// Tarjan's strongly connected components algorithm for packages.
    fn tarjan(graph: &PackageDependencyGraph) -> Vec<Vec<String>> {
        let mut state = PackageTarjanState::new();

        // Sort nodes for deterministic output
        let mut nodes: Vec<_> = graph.nodes().iter().cloned().collect();
        nodes.sort();

        for node in &nodes {
            if !state.index.contains_key(node) {
                Self::strongconnect(graph, node, &mut state);
            }
        }

        state.sccs
    }

    /// Helper function for Tarjan's algorithm.
    fn strongconnect(graph: &PackageDependencyGraph, node: &str, state: &mut PackageTarjanState) {
        // Set the depth index for this node
        let _ = state.index.insert(node.to_owned(), state.index_counter);
        let _ = state.lowlink.insert(node.to_owned(), state.index_counter);
        state.index_counter = state.index_counter.saturating_add(1);
        state.stack.push(node.to_owned());
        let _ = state.on_stack.insert(node.to_owned());

        // Consider successors
        let mut successors: Vec<_> = graph.successors(node).cloned().collect();
        successors.sort();

        for successor in successors {
            Self::process_successor(graph, node, &successor, state);
        }

        // If node is a root node, pop the stack and generate an SCC
        let node_index = state.index.get(node).copied().unwrap_or(0);
        let node_lowlink = state.lowlink.get(node).copied().unwrap_or(0);

        if node_lowlink == node_index {
            let scc = Self::pop_scc_from_stack(node, state);
            state.sccs.push(scc);
        }
    }

    /// Process a single successor in the Tarjan algorithm.
    fn process_successor(
        graph: &PackageDependencyGraph,
        node: &str,
        successor: &str,
        state: &mut PackageTarjanState,
    ) {
        if !state.index.contains_key(successor) {
            // Successor has not been visited; recurse
            Self::strongconnect(graph, successor, state);
            // Update lowlink
            let node_lowlink = state.lowlink.get(node).copied().unwrap_or(usize::MAX);
            let succ_lowlink = state.lowlink.get(successor).copied().unwrap_or(usize::MAX);
            let _ = state
                .lowlink
                .insert(node.to_owned(), node_lowlink.min(succ_lowlink));
        } else if state.on_stack.contains(successor) {
            // Successor is on the stack, hence in the current SCC
            let node_lowlink = state.lowlink.get(node).copied().unwrap_or(usize::MAX);
            let succ_index = state.index.get(successor).copied().unwrap_or(usize::MAX);
            let _ = state
                .lowlink
                .insert(node.to_owned(), node_lowlink.min(succ_index));
        } else {
            // Successor already processed, not part of current SCC path
        }
    }

    /// Pop a strongly connected component from the stack.
    fn pop_scc_from_stack(node: &str, state: &mut PackageTarjanState) -> Vec<String> {
        let mut scc = Vec::new();
        while let Some(w) = state.stack.pop() {
            let _ = state.on_stack.remove(&w);
            let is_root = w == node;
            scc.push(w);
            if is_root {
                break;
            }
        }
        scc
    }

    /// Johnson's algorithm for finding all elementary cycles in an SCC.
    ///
    /// This finds all simple cycles (cycles without repeated nodes except
    /// the start/end node).
    fn johnson_all_cycles(graph: &PackageDependencyGraph, scc: &[String]) -> Vec<Vec<String>> {
        let mut cycles = Vec::new();
        let scc_set: HashSet<_> = scc.iter().cloned().collect();

        // Sort SCC nodes for deterministic iteration
        let mut sorted_scc = scc.to_vec();
        sorted_scc.sort();

        // For each node as a starting point
        for (idx, start) in sorted_scc.iter().enumerate() {
            // Only consider nodes from current position onwards
            // (nodes before have already been processed as start nodes)
            let subgraph: HashSet<_> = sorted_scc.iter().skip(idx).cloned().collect();

            let mut blocked: HashSet<String> = HashSet::new();
            let mut blocked_map: HashMap<String, HashSet<String>> = HashMap::new();
            let mut path = vec![start.clone()];

            let _ = Self::johnson_circuit(
                graph,
                start,
                start,
                &scc_set,
                &subgraph,
                &mut blocked,
                &mut blocked_map,
                &mut path,
                &mut cycles,
            );
        }

        cycles
    }

    /// Circuit-finding subroutine of Johnson's algorithm.
    #[expect(clippy::too_many_arguments, reason = "Johnson's algorithm state")]
    fn johnson_circuit(
        graph: &PackageDependencyGraph,
        start: &str,
        current: &str,
        scc_set: &HashSet<String>,
        subgraph: &HashSet<String>,
        blocked: &mut HashSet<String>,
        blocked_map: &mut HashMap<String, HashSet<String>>,
        path: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) -> bool {
        let mut found_cycle = false;
        let _ = blocked.insert(current.to_owned());

        // Get successors in the subgraph
        let mut successors: Vec<_> = graph
            .successors(current)
            .filter(|s| scc_set.contains(*s) && subgraph.contains(*s))
            .cloned()
            .collect();
        successors.sort();

        for successor in successors {
            let cycle_found = Self::process_johnson_successor(
                graph,
                start,
                &successor,
                scc_set,
                subgraph,
                blocked,
                blocked_map,
                path,
                cycles,
            );
            if cycle_found {
                found_cycle = true;
            }
        }

        if found_cycle {
            Self::unblock(current, blocked, blocked_map);
        } else {
            // Add current to blocked_map for all successors
            for successor in graph
                .successors(current)
                .filter(|s| scc_set.contains(*s) && subgraph.contains(*s))
            {
                let _ = blocked_map
                    .entry(successor.clone())
                    .or_default()
                    .insert(current.to_owned());
            }
        }

        found_cycle
    }

    /// Process a single successor in Johnson's circuit algorithm.
    #[expect(clippy::too_many_arguments, reason = "Johnson's algorithm state")]
    fn process_johnson_successor(
        graph: &PackageDependencyGraph,
        start: &str,
        successor: &str,
        scc_set: &HashSet<String>,
        subgraph: &HashSet<String>,
        blocked: &mut HashSet<String>,
        blocked_map: &mut HashMap<String, HashSet<String>>,
        path: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) -> bool {
        if successor == start {
            // Found a cycle
            let mut cycle = path.clone();
            cycle.push(start.to_owned());
            cycles.push(cycle);
            return true;
        }

        if blocked.contains(successor) {
            return false;
        }

        path.push(successor.to_owned());
        let found = Self::johnson_circuit(
            graph,
            start,
            successor,
            scc_set,
            subgraph,
            blocked,
            blocked_map,
            path,
            cycles,
        );
        let _ = path.pop();
        found
    }

    /// Unblock a node in Johnson's algorithm.
    fn unblock(
        node: &str,
        blocked: &mut HashSet<String>,
        blocked_map: &mut HashMap<String, HashSet<String>>,
    ) {
        let _ = blocked.remove(node);
        let Some(to_unblock) = blocked_map.remove(node) else {
            return;
        };
        // Sort for deterministic order
        let mut sorted: Vec<_> = to_unblock.into_iter().collect();
        sorted.sort();
        for other in sorted {
            if blocked.contains(&other) {
                Self::unblock(&other, blocked, blocked_map);
            }
        }
    }

    /// Normalize a cycle to start from the lexicographically smallest node.
    fn normalize(mut cycle: Vec<String>) -> PackageCycle {
        if cycle.len() <= 1 {
            return PackageCycle::new(cycle);
        }

        // Remove the last element (duplicate of first)
        let last = cycle.pop();

        // Find the index of the smallest element
        let min_idx = cycle
            .iter()
            .enumerate()
            .min_by_key(|(_, name)| *name)
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // Rotate to start from the smallest
        cycle.rotate_left(min_idx);

        // Add the first element back at the end
        if let Some(first) = cycle.first().cloned() {
            cycle.push(first);
        } else if let Some(l) = last {
            cycle.push(l);
        } else {
            // Both are None, leave cycle as is
        }

        PackageCycle::new(cycle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("a.ts"));

        let cycles = CycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "expected exactly one cycle");
        assert_eq!(
            cycles.first().map(Cycle::len),
            Some(2),
            "cycle should have 2 nodes"
        );
    }

    #[test]
    fn test_no_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("c.ts"));

        let cycles = CycleDetector::detect(&graph);
        assert!(cycles.is_empty(), "expected no cycles");
    }

    #[test]
    fn test_self_loop() {
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("a.ts"));

        let cycles = CycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "expected one self-loop cycle");
    }

    #[test]
    fn test_cycle_normalization() {
        let cycle = vec![
            PathBuf::from("c.ts"),
            PathBuf::from("a.ts"),
            PathBuf::from("b.ts"),
            PathBuf::from("c.ts"),
        ];
        let normalized = CycleDetector::normalize(cycle);
        assert_eq!(
            normalized
                .path
                .first()
                .map(|p| p.to_string_lossy().to_string()),
            Some("a.ts".to_owned()),
            "cycle should start from lexicographically smallest"
        );
    }

    #[test]
    fn test_package_simple_cycle() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@myorg/a".to_owned(), "@myorg/b".to_owned());
        graph.add_edge("@myorg/b".to_owned(), "@myorg/a".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "expected exactly one package cycle");
        assert_eq!(
            cycles.first().map(PackageCycle::len),
            Some(2),
            "package cycle should have 2 packages"
        );
    }

    #[test]
    fn test_package_no_cycle() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@myorg/a".to_owned(), "@myorg/b".to_owned());
        graph.add_edge("@myorg/b".to_owned(), "@myorg/c".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert!(cycles.is_empty(), "expected no package cycles");
    }

    #[test]
    fn test_package_three_way_cycle() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@myorg/a".to_owned(), "@myorg/b".to_owned());
        graph.add_edge("@myorg/b".to_owned(), "@myorg/c".to_owned());
        graph.add_edge("@myorg/c".to_owned(), "@myorg/a".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "expected exactly one package cycle");
        assert_eq!(
            cycles.first().map(PackageCycle::len),
            Some(3),
            "package cycle should have 3 packages"
        );
    }

    // =========================================================================
    // Additional cycle detection tests for comprehensive coverage
    // =========================================================================

    #[test]
    fn test_cycle_len_and_is_empty() {
        let empty_cycle = Cycle::new(vec![]);
        assert_eq!(empty_cycle.len(), 0, "empty cycle has length 0");
        assert!(empty_cycle.is_empty(), "empty cycle is empty");

        let single_node = Cycle::new(vec![PathBuf::from("a.ts")]);
        assert_eq!(single_node.len(), 0, "single node cycle has length 0");
        assert!(!single_node.is_empty(), "single node cycle is not empty");

        let two_node_cycle = Cycle::new(vec![
            PathBuf::from("a.ts"),
            PathBuf::from("b.ts"),
            PathBuf::from("a.ts"),
        ]);
        assert_eq!(two_node_cycle.len(), 2, "two node cycle has length 2");
        assert!(!two_node_cycle.is_empty(), "two node cycle is not empty");
    }

    #[test]
    fn test_package_cycle_len_and_is_empty() {
        let empty_cycle = PackageCycle::new(vec![]);
        assert_eq!(empty_cycle.len(), 0, "empty package cycle has length 0");
        assert!(empty_cycle.is_empty(), "empty package cycle is empty");

        let single_pkg = PackageCycle::new(vec!["@myorg/a".to_owned()]);
        assert_eq!(
            single_pkg.len(),
            0,
            "single package cycle has length 0 (minus repeat)"
        );
        assert!(!single_pkg.is_empty(), "single package cycle is not empty");

        let two_pkg_cycle = PackageCycle::new(vec![
            "@myorg/a".to_owned(),
            "@myorg/b".to_owned(),
            "@myorg/a".to_owned(),
        ]);
        assert_eq!(two_pkg_cycle.len(), 2, "two package cycle has length 2");
    }

    #[test]
    fn test_complex_cycle_with_branches() {
        // Graph: A -> B -> C -> D -> B (cycle in B-C-D)
        //        A -> E (no cycle)
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("c.ts"));
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("d.ts"));
        graph.add_edge(PathBuf::from("d.ts"), PathBuf::from("b.ts")); // creates cycle
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("e.ts")); // no cycle

        let cycles = CycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "should detect exactly one cycle");
        assert_eq!(
            cycles.first().map(Cycle::len),
            Some(3),
            "cycle B-C-D should have 3 nodes"
        );
    }

    #[test]
    fn test_multiple_independent_cycles() {
        // Two independent cycles: A -> B -> A and C -> D -> C
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("a.ts"));
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("d.ts"));
        graph.add_edge(PathBuf::from("d.ts"), PathBuf::from("c.ts"));

        let cycles = CycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 2, "should detect two independent cycles");
    }

    #[test]
    fn test_nested_cycles() {
        // Nested cycles: A -> B -> C -> A, B -> D -> B
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("c.ts"));
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("a.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("d.ts"));
        graph.add_edge(PathBuf::from("d.ts"), PathBuf::from("b.ts"));

        let cycles = CycleDetector::detect(&graph);
        // The SCC includes all nodes A, B, C, D as they are all reachable in cycles
        assert!(
            !cycles.is_empty(),
            "should detect cycles in nested structure"
        );
    }

    #[test]
    fn test_long_chain_no_cycle() {
        // A -> B -> C -> D -> E -> F (no cycle)
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("c.ts"));
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("d.ts"));
        graph.add_edge(PathBuf::from("d.ts"), PathBuf::from("e.ts"));
        graph.add_edge(PathBuf::from("e.ts"), PathBuf::from("f.ts"));

        let cycles = CycleDetector::detect(&graph);
        assert!(
            cycles.is_empty(),
            "long chain without cycle should produce no cycles"
        );
    }

    #[test]
    fn test_long_chain_with_cycle_at_end() {
        // A -> B -> C -> D -> E -> F -> D (cycle D-E-F)
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("c.ts"));
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("d.ts"));
        graph.add_edge(PathBuf::from("d.ts"), PathBuf::from("e.ts"));
        graph.add_edge(PathBuf::from("e.ts"), PathBuf::from("f.ts"));
        graph.add_edge(PathBuf::from("f.ts"), PathBuf::from("d.ts")); // cycle

        let cycles = CycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "should detect one cycle at end of chain");
        assert_eq!(
            cycles.first().map(Cycle::len),
            Some(3),
            "cycle D-E-F should have 3 nodes"
        );
    }

    #[test]
    fn test_diamond_dependency_no_cycle() {
        // Diamond: A -> B, A -> C, B -> D, C -> D (not a cycle)
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("c.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("d.ts"));
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("d.ts"));

        let cycles = CycleDetector::detect(&graph);
        assert!(
            cycles.is_empty(),
            "diamond dependency pattern is not a cycle"
        );
    }

    #[test]
    fn test_figure_eight_cycles() {
        // Figure-8: A -> B -> A, B -> C -> B
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("a.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("c.ts"));
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("b.ts"));

        let cycles = CycleDetector::detect(&graph);
        // All three are in one SCC
        assert!(!cycles.is_empty(), "figure-8 pattern should detect cycles");
    }

    #[test]
    fn test_cycle_normalization_lexical_order() {
        // The cycle should be normalized to start from lexicographically smallest node
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("z.ts"), PathBuf::from("m.ts"));
        graph.add_edge(PathBuf::from("m.ts"), PathBuf::from("a.ts"));
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("z.ts"));

        let cycles = CycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "should detect one cycle");

        let cycle = cycles.first().expect("test should have cycle");
        assert_eq!(
            cycle.path.first().map(|p| p.to_string_lossy().to_string()),
            Some("a.ts".to_owned()),
            "cycle should start with lexicographically smallest node"
        );
    }

    #[test]
    fn test_package_cycle_five_way() {
        // A -> B -> C -> D -> E -> A (5-way cycle)
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/e".to_owned());
        graph.add_edge("@pkg/e".to_owned(), "@pkg/a".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "should detect one 5-way cycle");
        assert_eq!(
            cycles.first().map(PackageCycle::len),
            Some(5),
            "cycle should have 5 packages"
        );
    }

    #[test]
    fn test_package_cycle_normalization() {
        // Z -> A -> M -> Z should normalize to A -> M -> Z -> A
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/z".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/a".to_owned(), "@pkg/m".to_owned());
        graph.add_edge("@pkg/m".to_owned(), "@pkg/z".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "should detect one cycle");

        let cycle = cycles.first().expect("test should have cycle");
        assert_eq!(
            cycle.packages.first(),
            Some(&"@pkg/a".to_owned()),
            "cycle should start with lexicographically smallest package"
        );
    }

    #[test]
    fn test_package_multiple_cycles() {
        // Independent cycles: A <-> B, C <-> D
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/c".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 2, "should detect two independent cycles");
    }

    #[test]
    fn test_package_complex_interconnected() {
        // Complex: A -> B -> C -> A, A -> D -> E -> A
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/a".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/e".to_owned());
        graph.add_edge("@pkg/e".to_owned(), "@pkg/a".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        // Should find BOTH cycles, not just one
        assert_eq!(
            cycles.len(),
            2,
            "should detect both cycles in interconnected graph"
        );
    }

    #[test]
    fn test_package_graph_with_leaf_nodes() {
        // A -> B -> C -> A (cycle), A -> L1, B -> L2, C -> L3 (leaves)
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/a".to_owned(), "@pkg/leaf1".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/leaf2".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/leaf3".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(
            cycles.len(),
            1,
            "should detect one cycle (leaves don't affect)"
        );
        assert_eq!(
            cycles.first().map(PackageCycle::len),
            Some(3),
            "cycle A-B-C should have 3 packages"
        );
    }

    #[test]
    fn test_empty_graph_no_cycles() {
        let graph = DependencyGraph::new();
        let cycles = CycleDetector::detect(&graph);
        assert!(cycles.is_empty(), "empty graph should have no cycles");

        let pkg_graph = PackageDependencyGraph::new();
        let pkg_cycles = PackageCycleDetector::detect(&pkg_graph);
        assert!(
            pkg_cycles.is_empty(),
            "empty package graph should have no cycles"
        );
    }

    #[test]
    fn test_single_node_no_self_loop() {
        let mut graph = DependencyGraph::new();
        graph.add_node(PathBuf::from("lonely.ts"));

        let cycles = CycleDetector::detect(&graph);
        assert!(cycles.is_empty(), "single isolated node is not a cycle");
    }

    #[test]
    fn test_cycle_determinism() {
        // Run detection multiple times to ensure deterministic output
        let mut graph = DependencyGraph::new();
        graph.add_edge(PathBuf::from("c.ts"), PathBuf::from("a.ts"));
        graph.add_edge(PathBuf::from("a.ts"), PathBuf::from("b.ts"));
        graph.add_edge(PathBuf::from("b.ts"), PathBuf::from("c.ts"));

        let cycles1 = CycleDetector::detect(&graph);
        let cycles2 = CycleDetector::detect(&graph);
        let cycles3 = CycleDetector::detect(&graph);

        assert_eq!(cycles1, cycles2, "cycle detection should be deterministic");
        assert_eq!(cycles2, cycles3, "cycle detection should be deterministic");
    }

    #[test]
    fn test_package_cycle_determinism() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/c".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());

        let cycles1 = PackageCycleDetector::detect(&graph);
        let cycles2 = PackageCycleDetector::detect(&graph);

        assert_eq!(
            cycles1, cycles2,
            "package cycle detection should be deterministic"
        );
    }

    #[test]
    fn test_cycle_clone_and_equality() {
        let cycle1 = Cycle::new(vec![
            PathBuf::from("a.ts"),
            PathBuf::from("b.ts"),
            PathBuf::from("a.ts"),
        ]);
        let cycle2 = cycle1.clone();

        assert_eq!(cycle1, cycle2, "cloned cycle should be equal");
        assert_eq!(cycle1.path, cycle2.path, "paths should be equal");
    }

    #[test]
    fn test_package_cycle_clone_and_equality() {
        let cycle1 = PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]);
        let cycle2 = cycle1.clone();

        assert_eq!(cycle1, cycle2, "cloned package cycle should be equal");
        assert_eq!(cycle1.packages, cycle2.packages, "packages should be equal");
    }

    // =========================================================================
    // Tests for finding ALL elementary cycles (Johnson's algorithm)
    // =========================================================================

    #[test]
    fn test_package_finds_all_two_cycles() {
        // Multiple 2-cycles: A <-> B, C <-> D, E <-> F
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/e".to_owned(), "@pkg/f".to_owned());
        graph.add_edge("@pkg/f".to_owned(), "@pkg/e".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 3, "should find all three 2-cycles");

        // Verify each cycle has length 2 (3 nodes including repeated start)
        for cycle in &cycles {
            assert_eq!(cycle.len(), 2, "each cycle should have 2 distinct packages");
        }
    }

    #[test]
    fn test_package_finds_both_cycles_in_shared_scc() {
        // Two distinct cycles sharing node A: A->B->C->A and A->D->E->A
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/a".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/e".to_owned());
        graph.add_edge("@pkg/e".to_owned(), "@pkg/a".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 2, "should find both 3-cycles in same SCC");

        // Verify both cycles have 3 distinct packages
        for cycle in &cycles {
            assert_eq!(cycle.len(), 3, "each cycle should have 3 distinct packages");
        }
    }

    #[test]
    fn test_package_finds_mixed_cycle_sizes() {
        // 2-cycle: A <-> B, 3-cycle: C->D->E->C
        let mut graph = PackageDependencyGraph::new();
        // 2-cycle
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/a".to_owned());
        // 3-cycle
        graph.add_edge("@pkg/c".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/e".to_owned());
        graph.add_edge("@pkg/e".to_owned(), "@pkg/c".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(
            cycles.len(),
            2,
            "should find both cycles of different sizes"
        );

        let two_cycle_count = cycles.iter().filter(|c| c.len() == 2).count();
        let three_cycle_count = cycles.iter().filter(|c| c.len() == 3).count();

        assert_eq!(two_cycle_count, 1, "should have one 2-cycle");
        assert_eq!(three_cycle_count, 1, "should have one 3-cycle");
    }

    #[test]
    fn test_package_finds_overlapping_cycles() {
        // Overlapping cycles: A->B->C->A and A->B->D->A (share A->B edge)
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/a".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 2, "should find both overlapping cycles");
    }

    #[test]
    fn test_package_finds_two_cycle_with_longer_cycles() {
        // 2-cycle A<->B plus 3-cycle involving A: A->C->D->A
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/a".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/a".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 2, "should find 2-cycle and 3-cycle");

        let has_two_cycle = cycles.iter().any(|c| c.len() == 2);
        let has_three_cycle = cycles.iter().any(|c| c.len() == 3);

        assert!(has_two_cycle, "should have a 2-cycle (A<->B)");
        assert!(has_three_cycle, "should have a 3-cycle (A->C->D->A)");
    }

    #[test]
    fn test_package_cycles_are_normalized() {
        // Cycle C->A->B->C should be normalized to start from A
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/c".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);
        assert_eq!(cycles.len(), 1, "should find one cycle");

        let cycle = cycles.first().expect("test should have one cycle");
        assert_eq!(
            cycle.packages.first(),
            Some(&"@pkg/a".to_owned()),
            "cycle should start from lexicographically smallest package"
        );
    }

    #[test]
    fn test_package_no_duplicate_cycles() {
        // Complex graph that could produce duplicates if not handled properly
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/a".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/b".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);

        // Check for duplicates
        let mut seen = HashSet::new();
        for cycle in &cycles {
            let key = cycle.packages.join("->");
            assert!(seen.insert(key.clone()), "duplicate cycle found: {key}");
        }
    }

    #[test]
    fn test_package_large_scc_finds_all_two_cycles() {
        // Create a large interconnected SCC with multiple 2-cycles
        // A <-> B, B <-> C, C <-> D, all in one SCC via A->C->B connection
        let mut graph = PackageDependencyGraph::new();
        // Main chain making it one SCC
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/a".to_owned());
        // Add back-edges creating 2-cycles
        graph.add_edge("@pkg/b".to_owned(), "@pkg/a".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/c".to_owned());

        let cycles = PackageCycleDetector::detect(&graph);

        // Should find: A<->B, B<->C, C<->D (3 two-cycles) plus longer cycles
        let two_cycle_count = cycles.iter().filter(|c| c.len() == 2).count();
        assert_eq!(
            two_cycle_count, 3,
            "should find all three 2-cycles in large SCC"
        );
    }

    /// Test that mimics a real monorepo structure like the ender workspace.
    ///
    /// Structure:
    /// - shared-utils-test is imported by many packages
    /// - shared-utils-test imports shared-utils-query-client
    /// - shared-utils-query-client imports shared-hooks
    /// - shared-hooks imports shared-utils-test-wrappers
    /// - shared-utils-test-wrappers imports back to shared-ds (creating cycles)
    /// - shared-ds imports shared-utils-test-wrappers (2-cycle)
    /// - Multiple packages form a large interconnected SCC
    #[test]
    #[expect(clippy::too_many_lines, reason = "complex test setup required")]
    fn test_package_realistic_monorepo_structure() {
        let mut graph = PackageDependencyGraph::new();

        // Core utilities chain (mimics @ender/shared-utils-*)
        graph.add_edge(
            "@pkg/utils-test".to_owned(),
            "@pkg/utils-query-client".to_owned(),
        );
        graph.add_edge(
            "@pkg/utils-query-client".to_owned(),
            "@pkg/utils-error".to_owned(),
        );
        graph.add_edge(
            "@pkg/utils-error".to_owned(),
            "@pkg/utils-notifications".to_owned(),
        );
        graph.add_edge("@pkg/utils-notifications".to_owned(), "@pkg/ds".to_owned());

        // Design system (shared-ds) has bidirectional deps with test-wrappers
        graph.add_edge("@pkg/ds".to_owned(), "@pkg/utils-test-wrappers".to_owned());
        graph.add_edge("@pkg/utils-test-wrappers".to_owned(), "@pkg/ds".to_owned());

        // Test wrappers connects back to query-client (creating longer cycle)
        graph.add_edge(
            "@pkg/utils-test-wrappers".to_owned(),
            "@pkg/utils-query-client".to_owned(),
        );

        // Hooks package in the middle
        graph.add_edge(
            "@pkg/utils-query-client".to_owned(),
            "@pkg/hooks".to_owned(),
        );
        graph.add_edge(
            "@pkg/hooks".to_owned(),
            "@pkg/utils-test-wrappers".to_owned(),
        );

        // Feature packages that depend on shared code
        graph.add_edge("@pkg/feature-auth".to_owned(), "@pkg/ds".to_owned());
        graph.add_edge(
            "@pkg/feature-auth".to_owned(),
            "@pkg/utils-error".to_owned(),
        );
        graph.add_edge("@pkg/ds".to_owned(), "@pkg/feature-auth".to_owned()); // back-edge

        // Generated API packages
        graph.add_edge(
            "@pkg/generated-api".to_owned(),
            "@pkg/utils-rest".to_owned(),
        );
        graph.add_edge("@pkg/utils-rest".to_owned(), "@pkg/utils-error".to_owned());
        graph.add_edge(
            "@pkg/feature-auth".to_owned(),
            "@pkg/generated-api".to_owned(),
        );

        // Context packages
        graph.add_edge("@pkg/contexts-user".to_owned(), "@pkg/ds".to_owned());
        graph.add_edge("@pkg/contexts-user".to_owned(), "@pkg/hooks".to_owned());
        graph.add_edge(
            "@pkg/utils-test-wrappers".to_owned(),
            "@pkg/contexts-user".to_owned(),
        );

        let cycles = PackageCycleDetector::detect(&graph);

        // Should find multiple cycles including:
        // 1. @pkg/ds <-> @pkg/utils-test-wrappers (2-cycle)
        // 2. @pkg/ds <-> @pkg/feature-auth (2-cycle)
        // 3. Longer cycles through the utility chain

        let two_cycle_count = cycles.iter().filter(|c| c.len() == 2).count();
        assert!(
            two_cycle_count >= 2,
            "should find at least 2 two-cycles, found {two_cycle_count}"
        );

        let longer_cycle_count = cycles.iter().filter(|c| c.len() > 2).count();
        assert!(
            longer_cycle_count >= 1,
            "should find at least 1 longer cycle, found {longer_cycle_count}"
        );

        // Total should be more than just finding one cycle
        assert!(
            cycles.len() >= 3,
            "should find multiple cycles in interconnected monorepo, found {}",
            cycles.len()
        );
    }

    /// Test with a graph structure that has many overlapping cycles through shared nodes.
    /// This mimics how utility packages often create many cycle paths.
    #[test]
    fn test_package_hub_spoke_with_cross_connections() {
        let mut graph = PackageDependencyGraph::new();

        // Hub package that many things depend on
        let hub = "@pkg/shared-core";

        // Spoke packages
        let spokes = ["@pkg/a", "@pkg/b", "@pkg/c", "@pkg/d", "@pkg/e"];

        // Each spoke depends on hub
        for spoke in &spokes {
            graph.add_edge((*spoke).to_owned(), hub.to_owned());
        }

        // Hub depends back on some spokes (creating 2-cycles)
        graph.add_edge(hub.to_owned(), "@pkg/a".to_owned());
        graph.add_edge(hub.to_owned(), "@pkg/c".to_owned());

        // Cross-connections between spokes
        graph.add_edge("@pkg/a".to_owned(), "@pkg/b".to_owned());
        graph.add_edge("@pkg/b".to_owned(), "@pkg/c".to_owned());
        graph.add_edge("@pkg/c".to_owned(), "@pkg/d".to_owned());
        graph.add_edge("@pkg/d".to_owned(), "@pkg/e".to_owned());
        graph.add_edge("@pkg/e".to_owned(), "@pkg/a".to_owned()); // completes outer ring

        let cycles = PackageCycleDetector::detect(&graph);

        // Should find:
        // - 2 two-cycles: hub<->a, hub<->c
        // - Multiple longer cycles through the ring and hub
        let two_cycle_count = cycles.iter().filter(|c| c.len() == 2).count();
        assert_eq!(two_cycle_count, 2, "should find 2 two-cycles with hub");

        // The ring a->b->c->d->e->a plus paths through hub create many cycles
        assert!(
            cycles.len() > 2,
            "should find more than just 2-cycles, found {}",
            cycles.len()
        );
    }

    /// Test performance and correctness with a moderately large interconnected graph.
    /// 20 packages with dense connections - smaller than ender but still complex.
    #[test]
    fn test_package_medium_scale_interconnected() {
        let mut graph = PackageDependencyGraph::new();

        // Create 20 packages
        let packages: Vec<String> = (0..20).map(|i| format!("@pkg/p{i:02}")).collect();

        // Create a base ring: p00 -> p01 -> p02 -> ... -> p19 -> p00
        // Use iter().zip() to avoid indexing
        for (from, to) in packages.iter().zip(packages.iter().skip(1)) {
            graph.add_edge(from.clone(), to.clone());
        }
        // Close the ring: p19 -> p00
        if let (Some(last), Some(first)) = (packages.last(), packages.first()) {
            graph.add_edge(last.clone(), first.clone());
        }

        // Add some back-edges to create 2-cycles
        graph.add_edge(packages[1].clone(), packages[0].clone()); // p00 <-> p01
        graph.add_edge(packages[5].clone(), packages[4].clone()); // p04 <-> p05
        graph.add_edge(packages[10].clone(), packages[9].clone()); // p09 <-> p10
        graph.add_edge(packages[15].clone(), packages[14].clone()); // p14 <-> p15

        // Add some cross-connections to create additional cycles
        graph.add_edge(packages[0].clone(), packages[10].clone());
        graph.add_edge(packages[10].clone(), packages[0].clone());
        graph.add_edge(packages[5].clone(), packages[15].clone());

        let cycles = PackageCycleDetector::detect(&graph);

        // Should find at least the 2-cycles we explicitly created
        let two_cycle_count = cycles.iter().filter(|c| c.len() == 2).count();
        assert!(
            two_cycle_count >= 5,
            "should find at least 5 two-cycles, found {two_cycle_count}"
        );

        // Should complete in reasonable time (this test itself is the check)
        // If Johnson's algorithm has a bug, this could hang or take very long
    }

    /// Test that verifies we find ALL 2-cycles even when they're part of larger SCCs.
    /// This was the original bug - 2-cycles were missed when in large SCCs.
    #[test]
    fn test_package_two_cycles_not_missed_in_large_scc() {
        let mut graph = PackageDependencyGraph::new();

        // Create a chain that forms one large SCC
        // p0 -> p1 -> p2 -> p3 -> p4 -> p5 -> p0
        graph.add_edge("@pkg/p0".to_owned(), "@pkg/p1".to_owned());
        graph.add_edge("@pkg/p1".to_owned(), "@pkg/p2".to_owned());
        graph.add_edge("@pkg/p2".to_owned(), "@pkg/p3".to_owned());
        graph.add_edge("@pkg/p3".to_owned(), "@pkg/p4".to_owned());
        graph.add_edge("@pkg/p4".to_owned(), "@pkg/p5".to_owned());
        graph.add_edge("@pkg/p5".to_owned(), "@pkg/p0".to_owned()); // close the ring

        // Add 2-cycles within the SCC
        graph.add_edge("@pkg/p1".to_owned(), "@pkg/p0".to_owned()); // p0 <-> p1
        graph.add_edge("@pkg/p3".to_owned(), "@pkg/p2".to_owned()); // p2 <-> p3
        graph.add_edge("@pkg/p5".to_owned(), "@pkg/p4".to_owned()); // p4 <-> p5

        let cycles = PackageCycleDetector::detect(&graph);

        // Must find all 3 two-cycles
        let two_cycles: Vec<_> = cycles.iter().filter(|c| c.len() == 2).collect();
        assert_eq!(
            two_cycles.len(),
            3,
            "must find all 3 two-cycles even in large SCC, found: {:?}",
            two_cycles.iter().map(|c| &c.packages).collect::<Vec<_>>()
        );

        // Verify the specific pairs
        let has_p0_p1 = two_cycles.iter().any(|c| {
            c.packages.contains(&"@pkg/p0".to_owned()) && c.packages.contains(&"@pkg/p1".to_owned())
        });
        let has_p2_p3 = two_cycles.iter().any(|c| {
            c.packages.contains(&"@pkg/p2".to_owned()) && c.packages.contains(&"@pkg/p3".to_owned())
        });
        let has_p4_p5 = two_cycles.iter().any(|c| {
            c.packages.contains(&"@pkg/p4".to_owned()) && c.packages.contains(&"@pkg/p5".to_owned())
        });

        assert!(has_p0_p1, "should find p0 <-> p1 cycle");
        assert!(has_p2_p3, "should find p2 <-> p3 cycle");
        assert!(has_p4_p5, "should find p4 <-> p5 cycle");
    }

    // =========================================================================
    // Tests for PackageCycleWithFiles and file-level detail
    // =========================================================================

    #[test]
    fn test_package_cycle_edge_new() {
        let edge = PackageCycleEdge::new("@pkg/a".to_owned(), "@pkg/b".to_owned());
        assert_eq!(edge.from_package, "@pkg/a", "from_package should match");
        assert_eq!(edge.to_package, "@pkg/b", "to_package should match");
        assert!(edge.files.is_empty(), "files should be empty");
    }

    #[test]
    fn test_package_cycle_edge_with_evidence() {
        let mut evidence = EdgeEvidence::new();
        evidence.add_file(PathBuf::from("src/a.ts"), None);
        evidence.add_file(PathBuf::from("src/b.ts"), Some("utils".to_owned()));

        let edge =
            PackageCycleEdge::with_evidence("@pkg/a".to_owned(), "@pkg/b".to_owned(), &evidence);

        assert_eq!(edge.from_package, "@pkg/a", "from_package should match");
        assert_eq!(edge.to_package, "@pkg/b", "to_package should match");
        assert_eq!(edge.files.len(), 2, "should have 2 files");
    }

    #[test]
    fn test_package_cycle_with_files_len_empty() {
        let cycle = PackageCycleWithFiles {
            packages: Vec::new(),
            edges: Vec::new(),
        };
        assert_eq!(cycle.len(), 0, "empty cycle should have len 0");
        assert!(cycle.is_empty(), "empty cycle should be empty");
    }

    #[test]
    fn test_package_cycle_with_files_len() {
        let cycle = PackageCycleWithFiles {
            packages: vec![
                "@pkg/a".to_owned(),
                "@pkg/b".to_owned(),
                "@pkg/a".to_owned(),
            ],
            edges: vec![
                PackageCycleEdge::new("@pkg/a".to_owned(), "@pkg/b".to_owned()),
                PackageCycleEdge::new("@pkg/b".to_owned(), "@pkg/a".to_owned()),
            ],
        };
        assert_eq!(cycle.len(), 2, "cycle with 2 packages should have len 2");
        assert!(!cycle.is_empty(), "cycle should not be empty");
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "test requires multiple assertions")]
    fn test_package_cycle_with_files_from_cycle() {
        // Build a graph with edge evidence
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            PathBuf::from("a/index.ts"),
            None,
        );
        graph.add_edge_with_evidence(
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
            PathBuf::from("b/index.ts"),
            Some("utils".to_owned()),
        );

        // Create a basic cycle
        let basic_cycle = PackageCycle::new(vec![
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
        ]);

        // Convert to cycle with files
        let cycle_with_files = PackageCycleWithFiles::from_cycle(&basic_cycle, &graph);

        assert_eq!(cycle_with_files.packages.len(), 3, "should have 3 entries");
        assert_eq!(cycle_with_files.edges.len(), 2, "should have 2 edges");

        // Check first edge: a -> b
        let first_edge = cycle_with_files.edges.first();
        assert_eq!(
            first_edge.map(|e| e.from_package.as_str()),
            Some("@pkg/a"),
            "first edge from should be @pkg/a"
        );
        assert_eq!(
            first_edge.map(|e| e.to_package.as_str()),
            Some("@pkg/b"),
            "first edge to should be @pkg/b"
        );
        assert_eq!(
            first_edge.map(|e| e.files.len()),
            Some(1),
            "first edge should have 1 file"
        );

        // Check second edge: b -> a
        let second_edge = cycle_with_files.edges.get(1);
        assert_eq!(
            second_edge.map(|e| e.from_package.as_str()),
            Some("@pkg/b"),
            "second edge from should be @pkg/b"
        );
        assert_eq!(
            second_edge.map(|e| e.to_package.as_str()),
            Some("@pkg/a"),
            "second edge to should be @pkg/a"
        );
        assert_eq!(
            second_edge.map(|e| e.files.len()),
            Some(1),
            "second edge should have 1 file"
        );

        // Check file details
        let empty_files: Vec<(PathBuf, Option<String>)> = vec![];
        let second_files = second_edge.map(|e| &e.files).unwrap_or(&empty_files);
        assert_eq!(
            second_files
                .first()
                .and_then(|(_, s)| s.as_ref())
                .map(String::as_str),
            Some("utils"),
            "second edge file should have utils subpath"
        );
    }

    #[test]
    fn test_detect_with_files_simple() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            PathBuf::from("a/index.ts"),
            None,
        );
        graph.add_edge_with_evidence(
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
            PathBuf::from("b/index.ts"),
            None,
        );

        let cycles = PackageCycleDetector::detect_with_files(&graph);

        assert_eq!(cycles.len(), 1, "should detect 1 cycle");

        let cycle = cycles.first().unwrap();
        assert_eq!(cycle.len(), 2, "cycle should have 2 packages");
        assert_eq!(cycle.edges.len(), 2, "cycle should have 2 edges");

        // Verify edges have file evidence
        for edge in &cycle.edges {
            assert!(
                !edge.files.is_empty(),
                "edge {} -> {} should have files",
                edge.from_package,
                edge.to_package
            );
        }
    }

    #[test]
    fn test_detect_with_files_three_way() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            PathBuf::from("a/uses-b.ts"),
            None,
        );
        graph.add_edge_with_evidence(
            "@pkg/b".to_owned(),
            "@pkg/c".to_owned(),
            PathBuf::from("b/uses-c.ts"),
            Some("lib".to_owned()),
        );
        graph.add_edge_with_evidence(
            "@pkg/c".to_owned(),
            "@pkg/a".to_owned(),
            PathBuf::from("c/uses-a.ts"),
            None,
        );

        let cycles = PackageCycleDetector::detect_with_files(&graph);

        assert_eq!(cycles.len(), 1, "should detect 1 cycle");

        let cycle = cycles.first().unwrap();
        assert_eq!(cycle.len(), 3, "cycle should have 3 packages");
        assert_eq!(cycle.edges.len(), 3, "cycle should have 3 edges");

        // Find the b -> c edge and check its subpath
        let b_to_c_edge = cycle
            .edges
            .iter()
            .find(|e| e.from_package == "@pkg/b" && e.to_package == "@pkg/c");

        assert!(b_to_c_edge.is_some(), "should find b -> c edge");
        let b_to_c = b_to_c_edge.unwrap();
        assert_eq!(b_to_c.files.len(), 1, "b -> c should have 1 file");
        assert_eq!(
            b_to_c.files.first().and_then(|(_, s)| s.as_ref()),
            Some(&"lib".to_owned()),
            "b -> c file should have lib subpath"
        );
    }

    #[test]
    fn test_detect_with_files_multiple_files_per_edge() {
        let mut graph = PackageDependencyGraph::new();

        // Multiple files create the same edge
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
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            PathBuf::from("a/types.ts"),
            Some("types".to_owned()),
        );
        graph.add_edge_with_evidence(
            "@pkg/b".to_owned(),
            "@pkg/a".to_owned(),
            PathBuf::from("b/index.ts"),
            None,
        );

        let cycles = PackageCycleDetector::detect_with_files(&graph);

        assert_eq!(cycles.len(), 1, "should detect 1 cycle");

        let cycle = cycles.first().unwrap();
        let a_to_b_edge = cycle
            .edges
            .iter()
            .find(|e| e.from_package == "@pkg/a" && e.to_package == "@pkg/b");

        assert!(a_to_b_edge.is_some(), "should find a -> b edge");
        assert_eq!(
            a_to_b_edge.map(|e| e.files.len()),
            Some(3),
            "a -> b edge should have 3 files"
        );
    }

    #[test]
    fn test_detect_with_files_no_cycles() {
        let mut graph = PackageDependencyGraph::new();
        graph.add_edge_with_evidence(
            "@pkg/a".to_owned(),
            "@pkg/b".to_owned(),
            PathBuf::from("a/index.ts"),
            None,
        );
        graph.add_edge_with_evidence(
            "@pkg/b".to_owned(),
            "@pkg/c".to_owned(),
            PathBuf::from("b/index.ts"),
            None,
        );

        let cycles = PackageCycleDetector::detect_with_files(&graph);

        assert!(cycles.is_empty(), "should detect no cycles");
    }

    #[test]
    fn test_package_cycle_with_files_clone() {
        let cycle = PackageCycleWithFiles {
            packages: vec![
                "@pkg/a".to_owned(),
                "@pkg/b".to_owned(),
                "@pkg/a".to_owned(),
            ],
            edges: vec![PackageCycleEdge::new(
                "@pkg/a".to_owned(),
                "@pkg/b".to_owned(),
            )],
        };

        let cloned = cycle.clone();
        assert_eq!(cloned.packages, cycle.packages, "packages should match");
        assert_eq!(cloned.edges.len(), cycle.edges.len(), "edges should match");
    }
}
