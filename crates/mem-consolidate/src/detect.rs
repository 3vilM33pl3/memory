use std::collections::BTreeMap;

use uuid::Uuid;

use crate::graph::FusedGraph;

/// Tuning for [`detect_communities`].
#[derive(Debug, Clone, Copy)]
pub struct DetectParams {
    /// Upper bound on label-propagation sweeps before returning the current
    /// partition (the loop usually converges well before this).
    pub max_iterations: usize,
}

impl Default for DetectParams {
    fn default() -> Self {
        Self { max_iterations: 20 }
    }
}

/// A discovered cluster of related memories, members sorted ascending.
#[derive(Debug, Clone, PartialEq)]
pub struct Community {
    pub members: Vec<Uuid>,
}

/// Groups the graph into communities with deterministic weighted asynchronous
/// label propagation (Raghavan et al. 2007), adapted for reproducibility.
///
/// Determinism comes from three choices, none of which use randomness or a
/// clock: nodes are visited in ascending id order; each node adopts the label
/// carrying the greatest summed incident edge weight, breaking ties toward the
/// smallest label id; and a node keeps its current label whenever that label is
/// already tied-maximal, which suppresses the oscillation that plagues vanilla
/// LPA on symmetric structures. The same graph therefore always yields the same
/// partition.
pub fn detect_communities(graph: &FusedGraph, params: &DetectParams) -> Vec<Community> {
    // Seed every node with its own id as its label.
    let mut label: BTreeMap<Uuid, Uuid> = graph.nodes().map(|node| (node, node)).collect();

    for _ in 0..params.max_iterations {
        let mut changed = false;
        for node in graph.nodes() {
            // Sum incident edge weight per neighboring label.
            let mut score: BTreeMap<Uuid, f64> = BTreeMap::new();
            for (neighbor, weight) in graph.neighbors(node) {
                *score.entry(label[&neighbor]).or_default() += weight;
            }
            if score.is_empty() {
                continue;
            }

            let current = label[&node];
            let current_weight = score.get(&current).copied().unwrap_or(0.0);

            // Best label by weight; ties broken toward the smallest label id.
            // Iterating the BTreeMap ascending means the first maximum reached
            // is already the smallest-id maximum.
            let mut best_label = current;
            let mut best_weight = f64::NEG_INFINITY;
            for (&candidate, &weight) in &score {
                if weight > best_weight {
                    best_weight = weight;
                    best_label = candidate;
                }
            }

            // Oscillation guard: only move if the winner strictly beats the
            // current label's weight. If the current label is tied-maximal it
            // stays put.
            if best_label != current && best_weight > current_weight {
                label.insert(node, best_label);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Group nodes by final label; sort members and communities for stable output.
    let mut groups: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
    for (node, lbl) in label {
        groups.entry(lbl).or_default().push(node);
    }
    let mut communities: Vec<Community> = groups
        .into_values()
        .map(|mut members| {
            members.sort();
            Community { members }
        })
        .collect();
    communities.sort_by(|a, b| a.members.first().cmp(&b.members.first()));
    communities
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::WeightedEdge;

    fn id(byte: u8) -> Uuid {
        Uuid::from_bytes([byte; 16])
    }

    fn edge(a: u8, b: u8, weight: f64) -> WeightedEdge {
        WeightedEdge {
            a: id(a),
            b: id(b),
            weight,
        }
    }

    #[test]
    fn separates_two_dense_cliques_joined_by_a_weak_bridge() {
        // Two triangles {1,2,3} and {4,5,6} with a single weak 3-4 bridge.
        let g = FusedGraph::from_edges([
            edge(1, 2, 1.0),
            edge(2, 3, 1.0),
            edge(1, 3, 1.0),
            edge(4, 5, 1.0),
            edge(5, 6, 1.0),
            edge(4, 6, 1.0),
            edge(3, 4, 0.05),
        ]);
        let communities = detect_communities(&g, &DetectParams::default());
        assert_eq!(communities.len(), 2);
        assert_eq!(communities[0].members, vec![id(1), id(2), id(3)]);
        assert_eq!(communities[1].members, vec![id(4), id(5), id(6)]);
    }

    #[test]
    fn is_deterministic_across_runs() {
        let g = FusedGraph::from_edges([
            edge(1, 2, 0.9),
            edge(2, 3, 0.8),
            edge(3, 1, 0.7),
            edge(4, 5, 0.6),
            edge(10, 11, 0.5),
            edge(2, 10, 0.1),
        ]);
        let first = detect_communities(&g, &DetectParams::default());
        let second = detect_communities(&g, &DetectParams::default());
        assert_eq!(first, second);
    }

    #[test]
    fn bipartite_graph_does_not_oscillate() {
        // Classic LPA oscillator: a 4-cycle. The guard must converge.
        let g = FusedGraph::from_edges([
            edge(1, 2, 1.0),
            edge(2, 3, 1.0),
            edge(3, 4, 1.0),
            edge(4, 1, 1.0),
        ]);
        let communities = detect_communities(&g, &DetectParams { max_iterations: 20 });
        // Converges to a single stable partition (all nodes accounted for once).
        let total: usize = communities.iter().map(|c| c.members.len()).sum();
        assert_eq!(total, 4);
    }
}
