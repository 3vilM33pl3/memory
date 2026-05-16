"""Memory Layer adapter for a pinned external MemoryAgentBench checkout.

The adapter intentionally depends only on Python's standard library so it can be
copied into MemoryAgentBench without expanding that project's dependency set.
"""

from __future__ import annotations

import json
import os
import re
import hashlib
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any


def _env_first(*names: str) -> str | None:
    for name in names:
        value = os.environ.get(name)
        if value:
            return value
    return None


def _slug(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9_.-]+", "-", value.strip()).strip("-").lower()
    return slug or "unknown"


def _json_request(method: str, url: str, payload: dict[str, Any], token: str | None) -> dict[str, Any]:
    body = json.dumps(payload).encode("utf-8")
    headers = {
        "content-type": "application/json",
        "accept": "application/json",
    }
    if token:
        headers["authorization"] = f"Bearer {token}"
        headers["x-api-token"] = token
    request = urllib.request.Request(url, data=body, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            data = response.read().decode("utf-8")
    except urllib.error.HTTPError as error:
        details = error.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"Memory API request failed: {method} {url}: {error.code} {details}") from error
    except urllib.error.URLError as error:
        raise RuntimeError(f"Memory API request failed: {method} {url}: {error}") from error
    return json.loads(data) if data else {}


@dataclass
class _Timer:
    started_at: float

    @classmethod
    def start(cls) -> "_Timer":
        return cls(time.time())

    def elapsed(self) -> float:
        return time.time() - self.started_at


class MemoryLayerAdapter:
    """Implements the MemoryAgentBench memorizing/query contract."""

    def __init__(self, wrapper: Any, agent_config: dict[str, Any], dataset_config: dict[str, Any]):
        self.wrapper = wrapper
        self.agent_config = agent_config
        self.dataset_config = dataset_config
        self.service_url = (
            agent_config.get("memory_service_url")
            or os.environ.get("MEMORY_AGENT_BENCH_MEMORY_URL")
            or "http://127.0.0.1:4040"
        ).rstrip("/")
        self.api_token = (
            agent_config.get("memory_api_token")
            or _env_first(
                "MEMORY_AGENT_BENCH_MEMORY_API_TOKEN",
                "MEMORY_LAYER_API_TOKEN",
                "MEMORY_SERVICE_API_TOKEN",
            )
        )
        self.retrieve_num = int(agent_config.get("retrieve_num", 8))
        self.chunk_index = 0
        self.memory_construction_time = 0.0
        self.project = self._project_slug()

    def _project_slug(self) -> str:
        prefix = self.agent_config.get("memory_project_prefix") or os.environ.get(
            "MEMORY_AGENT_BENCH_PROJECT_PREFIX", "mab"
        )
        save_path = Path(getattr(self.wrapper, "agent_save_to_folder", "memory-layer"))
        context_id = save_path.name if save_path.name else "context"
        parts = [
            str(prefix),
            str(self.dataset_config.get("dataset", "dataset")),
            str(self.dataset_config.get("sub_dataset", "subset")),
            str(context_id),
        ]
        return "-".join(_slug(part) for part in parts)

    def send_message(self, message: str, memorizing: bool = False, query_id: int | None = None, context_id: int | None = None) -> Any:
        if memorizing:
            self._memorize(message, context_id)
            return ""
        return self._query(message, query_id, context_id)

    def save_agent(self) -> None:
        Path(self.wrapper.agent_save_to_folder).mkdir(parents=True, exist_ok=True)
        marker = {
            "project": self.project,
            "service_url": self.service_url,
            "chunks_seen": self.chunk_index,
        }
        Path(self.wrapper.agent_save_to_folder, "memory-layer-agent.json").write_text(
            json.dumps(marker, indent=2),
            encoding="utf-8",
        )

    def load_agent(self) -> None:
        marker_path = Path(self.wrapper.agent_save_to_folder, "memory-layer-agent.json")
        if marker_path.exists():
            marker = json.loads(marker_path.read_text(encoding="utf-8"))
            self.project = marker.get("project", self.project)

    def _memorize(self, message: str, context_id: int | None) -> None:
        timer = _Timer.start()
        chunk_id = self.chunk_index
        self.chunk_index += 1
        context_label = "unknown" if context_id is None else str(context_id)
        canonical_text = (
            f"MemoryAgentBench context chunk\n"
            f"dataset: {self.dataset_config.get('dataset')}\n"
            f"sub_dataset: {self.dataset_config.get('sub_dataset')}\n"
            f"context_id: {context_label}\n"
            f"chunk_id: {chunk_id}\n\n"
            f"{message}"
        )
        task_title = f"MemoryAgentBench ingest {self.dataset_config.get('sub_dataset')} context {context_label}"
        capture = {
            "project": self.project,
            "task_title": task_title,
            "user_prompt": "MemoryAgentBench memorization phase",
            "writer_id": "memory-agent-bench",
            "writer_name": "MemoryAgentBench",
            "agent_summary": f"Stored benchmark context chunk {chunk_id}.",
            "structured_candidates": [
                {
                    "canonical_text": canonical_text,
                    "summary": f"MemoryAgentBench {self.dataset_config.get('sub_dataset')} chunk {chunk_id}",
                    "memory_type": "reference",
                    "confidence": 0.95,
                    "importance": 3,
                    "tags": [
                        "memory-agent-bench",
                        _slug(str(self.dataset_config.get("dataset", "dataset"))),
                        _slug(str(self.dataset_config.get("sub_dataset", "subset"))),
                        f"context-{_slug(context_label)}",
                        f"chunk-{chunk_id}",
                    ],
                    "sources": [
                        {
                            "source_kind": "note",
                            "excerpt": f"MemoryAgentBench context {context_label}, chunk {chunk_id}",
                        }
                    ],
                }
            ],
            "idempotency_key": f"{self.project}:chunk:{chunk_id}:{hashlib.sha256(message.encode('utf-8')).hexdigest()}",
        }
        capture_response = _json_request(
            "POST", f"{self.service_url}/v1/capture/task", capture, self.api_token
        )
        raw_capture_id = capture_response.get("raw_capture_id")
        curate = {
            "project": self.project,
            "raw_capture_id": raw_capture_id,
            "replacement_policy": "conservative",
        }
        _json_request("POST", f"{self.service_url}/v1/curate", curate, self.api_token)
        self.memory_construction_time += timer.elapsed()

    def _query(self, message: str, query_id: int | None, context_id: int | None) -> dict[str, Any]:
        timer = _Timer.start()
        query = {
            "project": self.project,
            "query": message,
            "top_k": self.retrieve_num,
            "retrieval_mode": "full-memory",
            "answer_mode": "llm",
        }
        response = _json_request("POST", f"{self.service_url}/v1/query", query, self.api_token)
        answer = response.get("answer", "")
        token_usage = response.get("answer_generation", {}).get("token_usage") or {}
        input_len = int(token_usage.get("input_tokens") or self._token_count(message))
        output_len = int(token_usage.get("output_tokens") or self._token_count(answer))
        retrieval_context = [
            {
                "memory_id": result.get("memory_id"),
                "summary": result.get("summary"),
                "snippet": result.get("snippet"),
                "score": result.get("score"),
            }
            for result in response.get("results", [])
        ]
        memory_time = self.memory_construction_time
        self.memory_construction_time = 0.0
        return {
            "output": answer,
            "input_len": input_len,
            "output_len": output_len,
            "memory_construction_time": memory_time,
            "query_time_len": timer.elapsed(),
            "retrieval_context": retrieval_context,
        }

    def _token_count(self, text: str) -> int:
        tokenizer = getattr(self.wrapper, "tokenizer", None)
        if tokenizer is not None:
            try:
                return len(tokenizer.encode(text, disallowed_special=()))
            except TypeError:
                return len(tokenizer.encode(text))
        return max(1, len(text.split()))
