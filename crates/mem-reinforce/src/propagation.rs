//! Spreading activation over pre-fetched memory-relation edges (Collins &
//! Loftus). Pure graph walk: the edge fetch itself lives in `repository`.
//!
//! Edges are treated as undirected at the canonical-memory level.
//! `Supersedes` relations are excluded at fetch time (they express version
//! lineage, not semantic association).

use std::collections::{HashMap, VecDeque};

use uuid::Uuid;

use crate::scoring::ScoreParams;

/// An undirected edge between two canonical memories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalEdge {
    pub a: Uuid,
    pub b: Uuid,
}

/// A propagated activation increment for a linked memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropagatedIncrement {
    pub canonical_id: Uuid,
    pub increment: f64,
    pub hop_distance: u8,
}

/// Computes spreading-activation increments for memories reachable from
/// `seed` within `params.max_hops` hops.
///
/// `increment(h) = seed_boost * hop_decay^h / fan(previous node)` (fan
/// division only when `fan_normalization` is set — the ACT-R fan effect,
/// which stops hub nodes from inflating all their neighbours). When a node
/// is reachable via several paths the largest increment wins. Increments
/// below `min_propagated_increment` are dropped, which zeroes out distant
/// nodes.
pub fn propagation_increments(
    seed: Uuid,
    seed_boost: f64,
    edges: &[CanonicalEdge],
    params: &ScoreParams,
) -> Vec<PropagatedIncrement> {
    if params.max_hops == 0 || seed_boost <= 0.0 {
        return Vec::new();
    }

    let mut adjacency: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for edge in edges {
        if edge.a == edge.b {
            continue;
        }
        adjacency.entry(edge.a).or_default().push(edge.b);
        adjacency.entry(edge.b).or_default().push(edge.a);
    }

    let fan = |node: Uuid| -> f64 {
        if !params.fan_normalization {
            return 1.0;
        }
        adjacency.get(&node).map_or(1.0, |n| n.len().max(1) as f64)
    };

    // best[node] = (increment, hop) — largest increment reachable so far.
    let mut best: HashMap<Uuid, (f64, u8)> = HashMap::new();
    let mut queue: VecDeque<(Uuid, f64, u8)> = VecDeque::new();
    queue.push_back((seed, seed_boost, 0));

    while let Some((node, incoming, hop)) = queue.pop_front() {
        if hop >= params.max_hops {
            continue;
        }
        let Some(neighbors) = adjacency.get(&node) else {
            continue;
        };
        let spread = incoming * params.hop_decay / fan(node);
        if spread < params.min_propagated_increment {
            continue;
        }
        for &next in neighbors {
            if next == seed {
                continue;
            }
            let improved = match best.get(&next) {
                Some(&(existing, _)) => spread > existing,
                None => true,
            };
            if improved {
                best.insert(next, (spread, hop + 1));
                queue.push_back((next, spread, hop + 1));
            }
        }
    }

    let mut out: Vec<PropagatedIncrement> = best
        .into_iter()
        .map(
            |(canonical_id, (increment, hop_distance))| PropagatedIncrement {
                canonical_id,
                increment,
                hop_distance,
            },
        )
        .collect();
    out.sort_by(|l, r| {
        r.increment
            .partial_cmp(&l.increment)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(l.canonical_id.cmp(&r.canonical_id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn params() -> ScoreParams {
        ScoreParams {
            fan_normalization: false,
            ..ScoreParams::default()
        }
    }

    #[test]
    fn one_hop_gets_hop_decay_fraction() {
        let edges = [CanonicalEdge { a: id(1), b: id(2) }];
        let out = propagation_increments(id(1), 1.0, &edges, &params());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].canonical_id, id(2));
        assert!((out[0].increment - 0.5).abs() < 1e-12);
        assert_eq!(out[0].hop_distance, 1);
    }

    #[test]
    fn two_hops_decay_twice_and_beyond_max_hops_is_dropped() {
        // 1 - 2 - 3 - 4 (max_hops = 2 → node 4 unreachable)
        let edges = [
            CanonicalEdge { a: id(1), b: id(2) },
            CanonicalEdge { a: id(2), b: id(3) },
            CanonicalEdge { a: id(3), b: id(4) },
        ];
        let out = propagation_increments(id(1), 1.0, &edges, &params());
        let by_id: HashMap<Uuid, PropagatedIncrement> =
            out.into_iter().map(|p| (p.canonical_id, p)).collect();
        assert!((by_id[&id(2)].increment - 0.5).abs() < 1e-12);
        assert!((by_id[&id(3)].increment - 0.25).abs() < 1e-12);
        assert_eq!(by_id[&id(3)].hop_distance, 2);
        assert!(!by_id.contains_key(&id(4)));
    }

    #[test]
    fn fan_normalization_divides_by_out_degree() {
        // Seed 1 connects to 2, 3, 4 → fan(1) = 3.
        let edges = [
            CanonicalEdge { a: id(1), b: id(2) },
            CanonicalEdge { a: id(1), b: id(3) },
            CanonicalEdge { a: id(1), b: id(4) },
        ];
        let p = ScoreParams::default(); // fan_normalization on, min cutoff 0.05
        let out = propagation_increments(id(1), 1.5, &edges, &p);
        assert_eq!(out.len(), 3);
        for inc in &out {
            assert!((inc.increment - 1.5 * 0.5 / 3.0).abs() < 1e-12);
        }
    }

    #[test]
    fn increments_below_cutoff_are_dropped() {
        let edges = [CanonicalEdge { a: id(1), b: id(2) }];
        let p = ScoreParams {
            min_propagated_increment: 0.6,
            ..params()
        };
        assert!(propagation_increments(id(1), 1.0, &edges, &p).is_empty());
    }

    #[test]
    fn multi_path_keeps_largest_increment() {
        // 1-2 direct, and 1-3-2 longer path; direct 0.5 beats 0.25.
        let edges = [
            CanonicalEdge { a: id(1), b: id(2) },
            CanonicalEdge { a: id(1), b: id(3) },
            CanonicalEdge { a: id(3), b: id(2) },
        ];
        let out = propagation_increments(id(1), 1.0, &edges, &params());
        let two = out.iter().find(|p| p.canonical_id == id(2)).unwrap();
        assert!((two.increment - 0.5).abs() < 1e-12);
        assert_eq!(two.hop_distance, 1);
    }

    #[test]
    fn seed_never_receives_an_increment_and_self_loops_ignored() {
        let edges = [
            CanonicalEdge { a: id(1), b: id(1) },
            CanonicalEdge { a: id(1), b: id(2) },
            CanonicalEdge { a: id(2), b: id(1) },
        ];
        let out = propagation_increments(id(1), 1.0, &edges, &params());
        assert!(out.iter().all(|p| p.canonical_id != id(1)));
    }

    #[test]
    fn zero_boost_or_zero_hops_yield_nothing() {
        let edges = [CanonicalEdge { a: id(1), b: id(2) }];
        assert!(propagation_increments(id(1), 0.0, &edges, &params()).is_empty());
        let p = ScoreParams {
            max_hops: 0,
            ..params()
        };
        assert!(propagation_increments(id(1), 1.0, &edges, &p).is_empty());
    }
}
