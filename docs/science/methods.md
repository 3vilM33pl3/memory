# Memory Layer: Methods

A cognitively grounded, evidence-backed memory system for coding agents —
architecture, parameters, and evaluation methodology. This document is the
technical companion to the [science pages on the website]
(https://www.memory-layer.dev/docs/how-it-works/reinforcement) and is written
to be citable; see [`CITATION.cff`](../../CITATION.cff).

## 1. System overview

Memory Layer converts completed work into durable, citable memories and
serves them back with either a grounded answer or an explicit refusal. The
pipeline is capture → curation → storage (PostgreSQL + pgvector) → hybrid
retrieval → answer synthesis, with three background systems maintaining the
store: usage-driven reinforcement, evidence-backed validation, and memory
consolidation. Every durable mutation beyond initial curation is
human-gated: replacements, corrections, merges, and consolidated insights all
land in a review queue rather than being applied silently.

Design constraints that shape everything below:

- **Local-first**: all storage and deterministic paths run on one machine;
  LLM and embedding providers are optional enhancements.
- **Determinism where it matters**: retrieval ranking, activation scoring,
  consolidation clustering, and the fallback answer synthesizer are
  deterministic — reproducible runs are a design requirement, not an
  afterthought.
- **Evidence over confidence**: memories carry provenance (files, commits,
  notes) that is re-verified against the filesystem; answers carry citations;
  weak evidence yields refusal (`insufficient_evidence`), not fabrication.

## 2. Memory model

A memory is an immutable versioned record: a summary, a self-contained
canonical text, one of 17 types (decision, convention, incident, insight, …),
confidence ∈ [0,1], importance ∈ [1,5], tags, and sources. Supersession
creates a new version rather than editing; tombstones rather than deletes.
Typed relations (supports, supersedes, duplicates, depends_on, summarizes,
related_to) link memories into a graph used by both retrieval and
consolidation.

Curation applies write-time quality controls: noise rejection, exact
re-observation folding (a repeated fact raises confidence instead of
duplicating), lexical and semantic near-duplicate detection, and a
replacement decision engine governed by a per-repository policy
(conservative / balanced / aggressive) that either supersedes in place or
queues a human-reviewed proposal.

## 3. Retrieval and answer synthesis

Queries fan out over three channels — lexical full-text, embedding cosine
similarity (optional), and a code-graph channel connecting memories to the
files/symbols a question touches — merged by a deterministic ranker whose
components (term overlap, exact phrases, tag/path matches, relation and graph
boosts, recency, importance, confidence, activation) are individually
reported per result (`score_explanation`, `debug`).

The deterministic answer synthesizer enforces an anti-overconfidence
contract, tuned against measured per-item signals rather than intuition:

- **Weak-match refusal**: refuse when the top result has term overlap < 0.55
  AND semantic similarity < 0.60 with no exact-phrase anchor — a high
  aggregate score from confidence/importance boosts cannot turn an off-topic
  memory into an answer.
- **Confidence floor**: memories below 0.55 confidence are never stated as
  the answer nor cited as support.
- **Same-topic runner-up filter**: a runner-up whose summary restates the
  top result's topic (token containment ≥ 0.55) is a duplicate or a
  contradiction — the superseded-sibling "stale echo" — and is neither echoed
  nor cited.

## 4. Reinforcement: ACT-R activation

Each memory carries an activation score following the ACT-R rational
analysis of memory (Anderson & Schooler, 1991): retrieval boosts activation,
activation spreads to related memories, and it decays exponentially with a
configurable half-life. Defaults: direct access boost 1.0; citation boost 1.5
(an answer *citing* a memory is stronger evidence of usefulness than mere
retrieval); half-life 30 days; activation capped at 20. Decay is computed
inside each update from `last_decay_at`, so concurrent writers never race a
read-modify-write cycle, and the ranker reads decayed activation with the
same half-life math it ranks by (weight 0.3, capped rank contribution).
There is deliberately no stochastic noise term: identical states yield
identical rankings.

Hot memories (activation ≥ 8.0 by default) are validated: evidence is
gathered from the repository and an LLM verdict confirms, improves, or flags
the memory — corrections are always human-gated. Provenance verification
independently decays memories whose file sources disappear.

The same utility-learning family maintains the automation loops: each loop
learns a per-project procedural utility from proposal decisions via the
delta rule U_n = U_{n-1} + α(R_n − U_{n-1}) (Fu & Anderson, 2006), advisory
only.

## 5. Consolidation

Following complementary-learning-systems reasoning (McClelland et al., 1995)
and schema-consolidation evidence (Tse et al., 2007), clusters of related
memories are periodically summarized into **insight** memories. Cluster
discovery fuses three signals — relation edges, embedding similarity, and
co-access (memories retrieved by the same queries) — into one graph and runs
deterministic weighted label propagation (Raghavan et al., 2007). A
worthiness gate driven by use/non-use decides which clusters merit synthesis;
a two-step LLM synthesis with a citable-evidence guard produces one atomic,
human-gated proposal per cluster. Members keep living (non-destructive gist
layer, per CLS); insights link to members via `summarizes` relations and
warm them through spreading activation when used.

## 6. Evaluation methodology

The eval harness (`mem-eval`) runs paired ablations — a no-memory baseline
versus full memory — over reviewed suites with typed items (retrieval QA,
grounded answers, resume quality, adversarial staleness), reporting
Recall@K/MRR/nDCG for retrieval, assertion recall for grounding, McNemar
tests over paired successes, and release gates with absolute per-group
floors. Runs are immutable artifacts tied to a suite checksum, git commit,
and configuration.

Reproducibility is first-class: `memory eval reproduce --suite <dir>` seeds
the fixture, runs both conditions under the keyless offline profile
(deterministic retrieval and synthesis; no API keys), compares, and applies
the gate; `memory eval archive` packages the suite plus run artifacts with a
manifest (git commit, CLI version, suite checksum, per-file SHA-256) into a
citeable, tamper-evident archive.

### Headline results (memory-quality-v1, 2026-07-07, keyless offline profile)

| Condition | Success | Notes |
|---|---|---|
| No-memory baseline | 4/26 (15.4%) | Refusal items pass by definition. |
| Full memory | 25/26 (96.2%) | +80.8 pp, McNemar p < 0.0001. |

Per group (full memory): retrieval 10/10; grounded answers 8/9; adversarial
staleness 7/7 — the adversarial group (superseded facts must not be echoed;
unanswerable questions must be refused) was 0/7 before the synthesis
guardrails of §3 landed and is the release gate's zero-tolerance floor.
Results are byte-stable across repeated runs.

The suite's fixture is deliberately synthetic (fictional "ledger-bridge"
facts reachable only through memory), making it privacy-clean and
publishable; the fixture seeds with note-kind sources so provenance decay
cannot rot the baseline between runs.

## 7. Limitations

- The canary suite is small (26 items) and synthetic; it is a regression
  gate, not a benchmark of general capability. Larger suites (research-v1,
  memory-improvement-v1) require LLM providers and a driving agent.
- Keyless mode restricts retrieval to the lexical and graph channels and
  synthesis to the deterministic template path.
- Activation parameters follow ACT-R conventions but have not been fitted to
  human data; they are engineering defaults validated by the eval gates.
- Single-writer identity per machine; multi-user deployments are future work.

## References

- Anderson, J. R., & Schooler, L. J. (1991). Reflections of the environment
  in memory. *Psychological Science*, 2(6). doi:10.1111/j.1467-9280.1991.tb00174.x
- Fu, W.-T., & Anderson, J. R. (2006). From recurrent choice to skill
  learning: A reinforcement-learning model. *JEP: General*, 135(2).
  doi:10.1037/0096-3445.135.2.184
- McClelland, J. L., McNaughton, B. L., & O'Reilly, R. C. (1995). Why there
  are complementary learning systems in the hippocampus and neocortex.
  *Psychological Review*, 102(3). doi:10.1037/0033-295X.102.3.419
- Raghavan, U. N., Albert, R., & Kumara, S. (2007). Near linear time
  algorithm to detect community structures in large-scale networks.
  *Physical Review E*, 76(3). doi:10.1103/PhysRevE.76.036106
- Tse, D., et al. (2007). Schemas and memory consolidation. *Science*,
  316(5821). doi:10.1126/science.1135935
