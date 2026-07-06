use std::collections::BTreeMap;

use uuid::Uuid;

/// An undirected weighted edge between two memories (by canonical id).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeightedEdge {
    pub a: Uuid,
    pub b: Uuid,
    pub weight: f64,
}

/// Symmetric weighted adjacency over canonical memory ids.
///
/// Backed by a `BTreeMap` so node and neighbor iteration order is a stable
/// function of the ids alone — the basis for deterministic clustering.
#[derive(Debug, Default, Clone)]
pub struct FusedGraph {
    adj: BTreeMap<Uuid, BTreeMap<Uuid, f64>>,
}

impl FusedGraph {
    /// Builds a graph from weighted edges. Parallel edges (including the
    /// mirrored direction) sum; self-loops and non-positive weights are
    /// dropped. Every edge is stored in both directions.
    pub fn from_edges(edges: impl IntoIterator<Item = WeightedEdge>) -> Self {
        let mut adj: BTreeMap<Uuid, BTreeMap<Uuid, f64>> = BTreeMap::new();
        for edge in edges {
            if edge.a == edge.b || edge.weight <= 0.0 || edge.weight.is_nan() {
                continue;
            }
            *adj.entry(edge.a).or_default().entry(edge.b).or_default() += edge.weight;
            *adj.entry(edge.b).or_default().entry(edge.a).or_default() += edge.weight;
        }
        Self { adj }
    }

    /// Iterates node ids in ascending order.
    pub fn nodes(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.adj.keys().copied()
    }

    pub fn node_count(&self) -> usize {
        self.adj.len()
    }

    /// Neighbors of `node` with edge weights, ascending by neighbor id.
    pub fn neighbors(&self, node: Uuid) -> impl Iterator<Item = (Uuid, f64)> + '_ {
        self.adj
            .get(&node)
            .into_iter()
            .flat_map(|edges| edges.iter().map(|(id, w)| (*id, *w)))
    }

    /// Weight of the edge between `a` and `b`, or 0 if none.
    pub fn edge_weight(&self, a: Uuid, b: Uuid) -> f64 {
        self.adj
            .get(&a)
            .and_then(|edges| edges.get(&b))
            .copied()
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> Uuid {
        Uuid::from_bytes([byte; 16])
    }

    #[test]
    fn parallel_edges_sum_and_mirror() {
        let g = FusedGraph::from_edges([
            WeightedEdge { a: id(1), b: id(2), weight: 0.5 },
            WeightedEdge { a: id(2), b: id(1), weight: 0.25 },
        ]);
        assert_eq!(g.edge_weight(id(1), id(2)), 0.75);
        assert_eq!(g.edge_weight(id(2), id(1)), 0.75);
    }

    #[test]
    fn drops_self_loops_and_nonpositive() {
        let g = FusedGraph::from_edges([
            WeightedEdge { a: id(1), b: id(1), weight: 1.0 },
            WeightedEdge { a: id(1), b: id(2), weight: 0.0 },
            WeightedEdge { a: id(1), b: id(3), weight: -1.0 },
        ]);
        assert_eq!(g.node_count(), 0);
    }
}
