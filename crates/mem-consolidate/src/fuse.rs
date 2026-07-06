use uuid::Uuid;

use crate::graph::WeightedEdge;

/// Weights and thresholds for combining the three edge channels into one
/// fused graph. Exposed as config knobs on the service side.
#[derive(Debug, Clone, Copy)]
pub struct FuseWeights {
    /// Contribution of a memory_relations edge (uniform per edge).
    pub relation: f64,
    /// Contribution of a semantic-similarity edge, scaled by normalized cosine.
    pub similarity: f64,
    /// Contribution of a co-access edge, scaled by a saturating co-occurrence count.
    pub coaccess: f64,
    /// Cosine floor: similarities below this are excluded upstream; used here to
    /// rescale `[sim_floor, 1]` onto `[0, 1]` so a barely-passing pair
    /// contributes little and a near-identical pair contributes fully.
    pub sim_floor: f64,
    /// Co-access count that saturates the co-access contribution to its full weight.
    pub coaccess_norm: f64,
}

impl Default for FuseWeights {
    fn default() -> Self {
        Self {
            relation: 1.0,
            similarity: 1.0,
            coaccess: 1.0,
            sim_floor: 0.82,
            coaccess_norm: 4.0,
        }
    }
}

/// Fuses the three channels into weighted edges over canonical ids.
///
/// - `relations`: memory-to-memory relation pairs (already excluding version
///   lineage), each contributing `relation`.
/// - `similarities`: `(a, b, cosine)` with `cosine >= sim_floor`, contributing
///   `similarity * norm(cosine)`.
/// - `coaccess`: `(a, b, count)` co-occurrence counts, contributing
///   `coaccess * min(count / coaccess_norm, 1)`.
///
/// Parallel edges across channels are summed by [`FusedGraph::from_edges`].
pub fn fuse_edges(
    relations: &[(Uuid, Uuid)],
    similarities: &[(Uuid, Uuid, f64)],
    coaccess: &[(Uuid, Uuid, u32)],
    w: &FuseWeights,
) -> Vec<WeightedEdge> {
    let mut edges = Vec::with_capacity(relations.len() + similarities.len() + coaccess.len());

    for &(a, b) in relations {
        edges.push(WeightedEdge {
            a,
            b,
            weight: w.relation,
        });
    }

    let span = (1.0 - w.sim_floor).max(f64::EPSILON);
    for &(a, b, cosine) in similarities {
        let norm = ((cosine - w.sim_floor) / span).clamp(0.0, 1.0);
        let weight = w.similarity * norm;
        if weight > 0.0 {
            edges.push(WeightedEdge { a, b, weight });
        }
    }

    for &(a, b, count) in coaccess {
        let saturated = (count as f64 / w.coaccess_norm).min(1.0);
        let weight = w.coaccess * saturated;
        if weight > 0.0 {
            edges.push(WeightedEdge { a, b, weight });
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> Uuid {
        Uuid::from_bytes([byte; 16])
    }

    #[test]
    fn fuses_all_three_channels() {
        let w = FuseWeights::default();
        let edges = fuse_edges(
            &[(id(1), id(2))],
            &[(id(1), id(2), 1.0)],
            &[(id(1), id(2), 8)],
            &w,
        );
        assert_eq!(edges.len(), 3);
        // relation 1.0 + similarity (cos 1.0 -> full) 1.0 + coaccess (saturated) 1.0
        let total: f64 = edges.iter().map(|e| e.weight).sum();
        assert!((total - 3.0).abs() < 1e-9);
    }

    #[test]
    fn similarity_at_floor_contributes_nothing() {
        let w = FuseWeights::default();
        let edges = fuse_edges(&[], &[(id(1), id(2), w.sim_floor)], &[], &w);
        assert!(edges.is_empty());
    }
}
