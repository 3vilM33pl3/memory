# Memory Layer Demo Storyboard

Target audience: OpenAI/Codex-style agent infrastructure engineers.
Target length: 90-120 seconds.
Format: scripted terminal output with narration and captions.

## Structure

| Time | Scene | Visual | Purpose |
| ---: | --- | --- | --- |
| 0:00 | Context evaporates | Fresh agent session prompt and missing project facts | State the infrastructure problem. |
| 0:12 | Capture then curate | Raw task capture becomes durable memory with provenance | Explain the Memory Layer model. |
| 0:26 | Query memory | `memory query` answer with citations and source lines | Demonstrate agent-facing retrieval. |
| 0:44 | Provenance | Cited memories, file sources, and task evidence | Show auditability. |
| 0:58 | Beyond vector search | Graph status and query diagnostics | Distinguish graph-backed evidence from plain similarity. |
| 1:14 | Evaluation loop | Paired eval commands and metrics to watch | Frame the future measurement plan. |
| 1:31 | Close | Summary architecture line | Leave engineers with the system claim. |

## Privacy Model

The video pipeline must not capture the local Memory database or send raw
project memory to a third-party service. The terminal is rendered from
sanitized, checked-in scene text. ElevenLabs receives only the narration text
from `demo/script.md`.

## Generated Artifacts

All generated media and intermediate files are written under `demo/output/`.
That directory is ignored except for its `.gitignore` placeholder.
