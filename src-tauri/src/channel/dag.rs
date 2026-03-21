use std::collections::HashMap;

use anyhow::{anyhow, Result};
use petgraph::algo::toposort;
use petgraph::graph::DiGraph;

use crate::channel::types::Subtask;

/// Build execution layers from a list of subtasks with dependency information.
///
/// Returns a Vec of layers, where each layer is a Vec of Subtask that can be
/// executed in parallel. Layers are ordered such that all dependencies of tasks
/// in layer N are contained in layers 0..N-1.
///
/// Returns an error if a cycle is detected in the dependency graph.
pub fn build_execution_layers(subtasks: &[Subtask]) -> Result<Vec<Vec<Subtask>>> {
    if subtasks.is_empty() {
        return Ok(vec![]);
    }

    // Map pup name → index in subtasks slice
    let mut pup_to_idx: HashMap<&str, usize> = HashMap::new();
    for (i, st) in subtasks.iter().enumerate() {
        pup_to_idx.insert(st.pup.as_str(), i);
    }

    // Build a directed graph: edge from dep → dependent
    let mut graph: DiGraph<usize, ()> = DiGraph::new();
    let node_indices: Vec<_> = (0..subtasks.len()).map(|i| graph.add_node(i)).collect();

    for (i, st) in subtasks.iter().enumerate() {
        for dep in &st.depends_on {
            if let Some(&dep_idx) = pup_to_idx.get(dep.as_str()) {
                // dep_idx must finish before i
                graph.add_edge(node_indices[dep_idx], node_indices[i], ());
            }
            // Unknown deps are silently ignored (pup may not be in the plan)
        }
    }

    // Topological sort — detects cycles
    let topo = toposort(&graph, None)
        .map_err(|_| anyhow!("Cycle detected in subtask dependency graph"))?;

    // Assign depth to each node: depth = max(dep.depth) + 1, roots get depth 0
    let mut depth: Vec<usize> = vec![0; subtasks.len()];
    for node in &topo {
        let idx = *graph.node_weight(*node).unwrap();
        // Look at all predecessors of this node
        let max_dep_depth = graph
            .neighbors_directed(*node, petgraph::Direction::Incoming)
            .map(|pred| {
                let pred_idx = *graph.node_weight(pred).unwrap();
                depth[pred_idx]
            })
            .max();
        depth[idx] = match max_dep_depth {
            Some(d) => d + 1,
            None => 0,
        };
    }

    // Group subtasks by depth into layers
    let max_depth = depth.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<Subtask>> = vec![vec![]; max_depth + 1];
    for (i, st) in subtasks.iter().enumerate() {
        layers[depth[i]].push(st.clone());
    }

    // Remove empty layers (shouldn't happen but be safe)
    layers.retain(|l| !l.is_empty());

    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(pup: &str, deps: &[&str]) -> Subtask {
        Subtask {
            pup: pup.to_string(),
            description: format!("task for {}", pup),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn test_no_deps_single_layer() {
        let subtasks = vec![st("dev", &[]), st("writer", &[])];
        let layers = build_execution_layers(&subtasks).unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].len(), 2);
    }

    #[test]
    fn test_sequential_deps() {
        let subtasks = vec![st("research", &[]), st("writer", &["research"])];
        let layers = build_execution_layers(&subtasks).unwrap();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0][0].pup, "research");
        assert_eq!(layers[1][0].pup, "writer");
    }

    #[test]
    fn test_cycle_returns_error() {
        let subtasks = vec![st("a", &["b"]), st("b", &["a"])];
        assert!(build_execution_layers(&subtasks).is_err());
    }
}
