-- Relation edges move to canonical endpoints: an edge between memories must
-- survive new versions of either end. Edges also gain provenance - origin
-- distinguishes relations asserted by consolidation or humans from the
-- heuristics curate re-derives on every run.
ALTER TABLE memory_relations
    ADD COLUMN src_canonical_id UUID,
    ADD COLUMN dst_canonical_id UUID,
    ADD COLUMN origin TEXT NOT NULL DEFAULT 'derived'
        CHECK (origin IN ('asserted', 'derived')),
    ADD COLUMN confidence REAL,
    ADD COLUMN actor TEXT,
    ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now();

UPDATE memory_relations r SET src_canonical_id = m.canonical_id
    FROM memory_entries m WHERE m.id = r.src_memory_id;
UPDATE memory_relations r SET dst_canonical_id = m.canonical_id
    FROM memory_entries m WHERE m.id = r.dst_memory_id;

-- Consolidation insight->member links are asserted facts, not heuristics.
UPDATE memory_relations SET origin = 'asserted' WHERE relation_type = 'summarizes';

-- Version churn created duplicate edges between the same canonicals.
DELETE FROM memory_relations a USING memory_relations b
 WHERE a.ctid < b.ctid
   AND a.src_canonical_id = b.src_canonical_id
   AND a.dst_canonical_id = b.dst_canonical_id
   AND a.relation_type = b.relation_type;

ALTER TABLE memory_relations
    ALTER COLUMN src_canonical_id SET NOT NULL,
    ALTER COLUMN dst_canonical_id SET NOT NULL;
ALTER TABLE memory_relations DROP COLUMN src_memory_id;
ALTER TABLE memory_relations DROP COLUMN dst_memory_id;

CREATE UNIQUE INDEX idx_memory_relations_canonical_edge
    ON memory_relations(src_canonical_id, relation_type, dst_canonical_id);
CREATE INDEX idx_memory_relations_dst ON memory_relations(dst_canonical_id);

-- Latest non-tombstone version of a canonical, for joining edges back to
-- displayable memory rows.
CREATE FUNCTION memory_latest_version_id(cid UUID) RETURNS UUID
LANGUAGE sql STABLE AS
$$
    SELECT id FROM memory_entries
    WHERE canonical_id = cid AND NOT is_tombstone
    ORDER BY version_no DESC
    LIMIT 1
$$;
