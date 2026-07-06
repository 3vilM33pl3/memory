use uuid::Uuid;

use crate::graph::FusedGraph;

/// Per-member signals used by the value gate.
#[derive(Debug, Clone, Copy)]
pub struct MemberStat {
    pub canonical_id: Uuid,
    /// Decay-adjusted activation (usage warmth) of the memory.
    pub activation: f64,
    /// Total co-access mass this member contributed within the cluster.
    pub coaccess_mass: f64,
}

/// Thresholds for deciding whether a cluster is worth consolidating.
#[derive(Debug, Clone, Copy)]
pub struct ValueGateConfig {
    pub min_size: usize,
    pub max_size: usize,
    /// Minimum fraction of possible intra-cluster edges that must be present.
    pub min_cohesion: f64,
    /// Salience floor (co-access mass OR activation mass) for the "use" trigger.
    pub min_salience: f64,
    /// Activation-mass ceiling below which a dense cluster counts as cold
    /// (the "non-use" trigger).
    pub cold_activation_max: f64,
}

impl Default for ValueGateConfig {
    fn default() -> Self {
        Self {
            min_size: 3,
            max_size: 25,
            min_cohesion: 0.35,
            min_salience: 2.0,
            cold_activation_max: 1.0,
        }
    }
}

/// Why a cluster passed the gate: driven by usage, or by dense-but-unused
/// structure. Mirrors the biological use/non-use consolidation triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerReason {
    /// Frequently co-accessed or highly activated ("use").
    Salient,
    /// Densely related but individually cold ("non-use").
    ColdDense,
}

/// Outcome of evaluating a cluster.
#[derive(Debug, Clone, PartialEq)]
pub enum GateOutcome {
    Accept(TriggerReason),
    Reject(&'static str),
}

/// Structural and salience metrics computed for a cluster.
#[derive(Debug, Clone, Copy)]
pub struct ClusterMetrics {
    pub size: usize,
    /// Present intra-cluster edges / possible pairs, in `[0, 1]`.
    pub intra_density: f64,
    pub coaccess_mass: f64,
    pub activation_mass: f64,
}

/// Scores a cluster and decides whether it is worth consolidating.
///
/// Accepts when it is neither too small nor too large, its members are
/// sufficiently interconnected, and it is *either* salient (usage) *or*
/// dense-but-cold (non-use). Novelty (already covered by an existing insight)
/// is enforced separately at the repository layer, since it needs the database.
pub fn evaluate_cluster(
    members: &[MemberStat],
    graph: &FusedGraph,
    cfg: &ValueGateConfig,
) -> (ClusterMetrics, GateOutcome) {
    let size = members.len();
    let possible_pairs = size.saturating_sub(1) * size / 2;
    let mut present = 0usize;
    for (i, left) in members.iter().enumerate() {
        for right in &members[i + 1..] {
            if graph.edge_weight(left.canonical_id, right.canonical_id) > 0.0 {
                present += 1;
            }
        }
    }
    let intra_density = if possible_pairs == 0 {
        0.0
    } else {
        present as f64 / possible_pairs as f64
    };
    let coaccess_mass: f64 = members.iter().map(|m| m.coaccess_mass).sum();
    let activation_mass: f64 = members.iter().map(|m| m.activation).sum();
    let metrics = ClusterMetrics {
        size,
        intra_density,
        coaccess_mass,
        activation_mass,
    };

    let outcome = if size < cfg.min_size {
        GateOutcome::Reject("below min_size")
    } else if size > cfg.max_size {
        GateOutcome::Reject("above max_size")
    } else if intra_density < cfg.min_cohesion {
        GateOutcome::Reject("below min_cohesion")
    } else if coaccess_mass >= cfg.min_salience || activation_mass >= cfg.min_salience {
        GateOutcome::Accept(TriggerReason::Salient)
    } else if activation_mass <= cfg.cold_activation_max {
        // Dense (passed cohesion) and unused: consolidate to reduce
        // fragmentation and let the members cool under one schema.
        GateOutcome::Accept(TriggerReason::ColdDense)
    } else {
        GateOutcome::Reject("insufficient salience")
    };

    (metrics, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::WeightedEdge;

    fn id(byte: u8) -> Uuid {
        Uuid::from_bytes([byte; 16])
    }

    fn clique(ids: &[u8]) -> FusedGraph {
        let mut edges = Vec::new();
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                edges.push(WeightedEdge {
                    a: id(a),
                    b: id(b),
                    weight: 1.0,
                });
            }
        }
        FusedGraph::from_edges(edges)
    }

    fn member(byte: u8, activation: f64, coaccess: f64) -> MemberStat {
        MemberStat {
            canonical_id: id(byte),
            activation,
            coaccess_mass: coaccess,
        }
    }

    #[test]
    fn rejects_too_small() {
        let g = clique(&[1, 2]);
        let (_, outcome) = evaluate_cluster(
            &[member(1, 5.0, 5.0), member(2, 5.0, 5.0)],
            &g,
            &ValueGateConfig::default(),
        );
        assert_eq!(outcome, GateOutcome::Reject("below min_size"));
    }

    #[test]
    fn accepts_salient_dense_cluster_via_use() {
        let g = clique(&[1, 2, 3]);
        let (metrics, outcome) = evaluate_cluster(
            &[
                member(1, 3.0, 0.0),
                member(2, 3.0, 0.0),
                member(3, 3.0, 0.0),
            ],
            &g,
            &ValueGateConfig::default(),
        );
        assert_eq!(metrics.intra_density, 1.0);
        assert_eq!(outcome, GateOutcome::Accept(TriggerReason::Salient));
    }

    #[test]
    fn accepts_dense_cold_cluster_via_non_use() {
        let g = clique(&[1, 2, 3]);
        let (_, outcome) = evaluate_cluster(
            &[
                member(1, 0.1, 0.0),
                member(2, 0.1, 0.0),
                member(3, 0.1, 0.0),
            ],
            &g,
            &ValueGateConfig::default(),
        );
        assert_eq!(outcome, GateOutcome::Accept(TriggerReason::ColdDense));
    }

    #[test]
    fn rejects_sparse_cluster() {
        // Three nodes, only one edge -> density 1/3 < 0.35.
        let g = FusedGraph::from_edges([WeightedEdge {
            a: id(1),
            b: id(2),
            weight: 1.0,
        }]);
        let (_, outcome) = evaluate_cluster(
            &[
                member(1, 5.0, 5.0),
                member(2, 5.0, 5.0),
                member(3, 5.0, 5.0),
            ],
            &g,
            &ValueGateConfig::default(),
        );
        assert_eq!(outcome, GateOutcome::Reject("below min_cohesion"));
    }

    #[test]
    fn rejects_warm_but_not_salient_mid_activation() {
        let g = clique(&[1, 2, 3]);
        // activation_mass = 1.5 (below min_salience 2.0) but above cold ceiling 1.0.
        let (_, outcome) = evaluate_cluster(
            &[
                member(1, 0.5, 0.0),
                member(2, 0.5, 0.0),
                member(3, 0.5, 0.0),
            ],
            &g,
            &ValueGateConfig::default(),
        );
        assert_eq!(outcome, GateOutcome::Reject("insufficient salience"));
    }
}
