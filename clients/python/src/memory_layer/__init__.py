"""Typed Python client for the Memory Layer HTTP API v1.

Covers the frozen ``x-stability: core`` surface documented by the OpenAPI
specification (served by any running service at ``GET /v1/openapi.yaml``).
Core operations are additive-only: unknown response fields are preserved in
each dataclass's ``raw`` attribute and must be ignored, never rejected.

Quickstart::

    from memory_layer import MemoryLayerClient

    client = MemoryLayerClient()  # http://127.0.0.1:4040, token from env
    client.remember("demo", title="Tried the Python client",
                    summary="It works", notes=["The client speaks core v1."])
    answer = client.query("demo", "What did I just record?")
    print(answer.answer, answer.confidence, answer.insufficient_evidence)
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from typing import Any

import requests

__all__ = [
    "MemoryLayerClient",
    "MemoryLayerError",
    "QueryAnswer",
    "QueryResult",
]

DEFAULT_BASE_URL = "http://127.0.0.1:4040"


class MemoryLayerError(RuntimeError):
    """An API call failed; carries the HTTP status and server error body."""

    def __init__(self, status: int, body: str):
        super().__init__(f"HTTP {status}: {body}")
        self.status = status
        self.body = body


@dataclass
class QueryResult:
    memory_id: str
    summary: str
    memory_type: str
    score: float
    snippet: str
    tags: list[str] = field(default_factory=list)
    raw: dict[str, Any] = field(default_factory=dict, repr=False)

    @classmethod
    def from_json(cls, data: dict[str, Any]) -> "QueryResult":
        return cls(
            memory_id=data["memory_id"],
            summary=data["summary"],
            memory_type=data["memory_type"],
            score=data["score"],
            snippet=data.get("snippet", ""),
            tags=data.get("tags", []),
            raw=data,
        )


@dataclass
class QueryAnswer:
    answer: str
    confidence: float
    insufficient_evidence: bool
    results: list[QueryResult]
    citations: list[dict[str, Any]]
    raw: dict[str, Any] = field(default_factory=dict, repr=False)

    @classmethod
    def from_json(cls, data: dict[str, Any]) -> "QueryAnswer":
        return cls(
            answer=data["answer"],
            confidence=data["confidence"],
            insufficient_evidence=data["insufficient_evidence"],
            results=[QueryResult.from_json(item) for item in data.get("results", [])],
            citations=data.get("answer_citations", []),
            raw=data,
        )


class MemoryLayerClient:
    """Thin client over the core v1 endpoints.

    ``token`` falls back to the ``MEMORY_API_TOKEN`` environment variable.
    On loopback deployments the service may accept trusted local writes
    without a token.
    """

    def __init__(
        self,
        base_url: str = DEFAULT_BASE_URL,
        token: str | None = None,
        session: requests.Session | None = None,
        timeout: float = 30.0,
        writer_id: str = "python-client",
    ):
        self.base_url = base_url.rstrip("/")
        self.token = token if token is not None else os.environ.get("MEMORY_API_TOKEN", "")
        self.session = session or requests.Session()
        self.timeout = timeout
        self.writer_id = writer_id

    # -- transport -----------------------------------------------------

    def _request(self, method: str, path: str, json: dict[str, Any] | None = None) -> dict[str, Any]:
        response = self.session.request(
            method,
            f"{self.base_url}{path}",
            json=json,
            headers={"x-api-token": self.token},
            timeout=self.timeout,
        )
        if response.status_code >= 400:
            raise MemoryLayerError(response.status_code, response.text)
        return response.json()

    # -- core reads ----------------------------------------------------

    def health(self) -> dict[str, Any]:
        return self._request("GET", "/healthz")

    def stats(self) -> dict[str, Any]:
        return self._request("GET", "/v1/stats")

    def query(
        self,
        project: str,
        question: str,
        *,
        top_k: int = 8,
        tags: list[str] | None = None,
        deterministic: bool = False,
    ) -> QueryAnswer:
        payload: dict[str, Any] = {
            "project": project,
            "query": question,
            "top_k": top_k,
            "include_stale": False,
            "history": False,
        }
        if tags:
            payload["filters"] = {"tags": tags}
        if deterministic:
            payload["answer_mode"] = "deterministic"
        return QueryAnswer.from_json(self._request("POST", "/v1/query", payload))

    def query_global(self, question: str, *, top_k: int = 8) -> QueryAnswer:
        return QueryAnswer.from_json(
            self._request("POST", "/v1/query/global", {"query": question, "top_k": top_k})
        )

    def memory(self, memory_id: str) -> dict[str, Any]:
        return self._request("GET", f"/v1/memory/{memory_id}")

    def memory_history(self, memory_id: str) -> dict[str, Any]:
        return self._request("GET", f"/v1/memory/{memory_id}/history")

    def project_memories(self, project: str) -> dict[str, Any]:
        return self._request("GET", f"/v1/projects/{project}/memories")

    def memory_graph(self, project: str, *, limit: int = 250) -> dict[str, Any]:
        return self._request("GET", f"/v1/projects/{project}/memory-graph?limit={limit}")

    def overview(self, project: str) -> dict[str, Any]:
        return self._request("GET", f"/v1/projects/{project}/overview")

    def resume(self, project: str) -> dict[str, Any]:
        return self._request("POST", f"/v1/projects/{project}/resume", {"project": project})

    # -- core writes ---------------------------------------------------

    def capture_task(self, request: dict[str, Any]) -> dict[str, Any]:
        """Low-level capture; see the OpenAPI CaptureTaskRequest schema."""
        return self._request("POST", "/v1/capture/task", request)

    def curate(self, project: str, *, raw_capture_id: str | None = None) -> dict[str, Any]:
        payload: dict[str, Any] = {"project": project}
        if raw_capture_id:
            # Bounded curation of one capture; whole-project curation can be
            # slow when a backlog is pending.
            payload["raw_capture_id"] = raw_capture_id
        return self._request("POST", "/v1/curate", payload)

    def remember(
        self,
        project: str,
        *,
        title: str,
        summary: str,
        notes: list[str] | None = None,
        memory_type: str = "project",
        tags: list[str] | None = None,
        confidence: float = 0.85,
        importance: int = 3,
    ) -> dict[str, Any]:
        """Capture one durable fact and curate it, in one call."""
        capture = self.capture_task(
            {
                "project": project,
                "task_title": title,
                "user_prompt": title,
                "writer_id": self.writer_id,
                "agent_summary": summary,
                "structured_candidates": [
                    {
                        "canonical_text": summary if not notes else f"{summary} " + " ".join(notes),
                        "summary": summary,
                        "memory_type": memory_type,
                        "confidence": confidence,
                        "importance": importance,
                        "tags": tags or [],
                        "sources": [{"source_kind": "note", "excerpt": title}],
                    }
                ],
            }
        )
        return self.curate(project, raw_capture_id=capture["raw_capture_id"])
