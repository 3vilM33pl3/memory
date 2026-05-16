import json
import os
import sys
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "overlays"))

from memory_layer_adapter import MemoryLayerAdapter


class FakeTokenizer:
    def encode(self, text, disallowed_special=()):
        return text.split()


class FakeWrapper:
    def __init__(self, save_dir):
        self.agent_save_to_folder = save_dir
        self.tokenizer = FakeTokenizer()


class FakeMemoryHandler(BaseHTTPRequestHandler):
    requests = []

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        payload = json.loads(self.rfile.read(length).decode("utf-8"))
        self.__class__.requests.append((self.path, payload, dict(self.headers)))
        if self.path == "/v1/capture/task":
            self._json({"raw_capture_id": "00000000-0000-0000-0000-000000000001"})
        elif self.path == "/v1/curate":
            self._json({"ok": True})
        elif self.path == "/v1/query":
            self._json(
                {
                    "answer": "Ada Lovelace",
                    "results": [
                        {
                            "memory_id": "00000000-0000-0000-0000-000000000002",
                            "summary": "Inventor",
                            "snippet": "Ada wrote notes.",
                            "score": 1.0,
                        }
                    ],
                    "answer_generation": {
                        "token_usage": {
                            "input_tokens": 12,
                            "output_tokens": 2,
                            "total_tokens": 14,
                        }
                    },
                }
            )
        else:
            self.send_error(404)

    def log_message(self, fmt, *args):
        return

    def _json(self, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class MemoryLayerAdapterTest(unittest.TestCase):
    def setUp(self):
        FakeMemoryHandler.requests = []
        self.server = HTTPServer(("127.0.0.1", 0), FakeMemoryHandler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.service_url = f"http://127.0.0.1:{self.server.server_port}"

    def tearDown(self):
        self.server.shutdown()
        self.thread.join(timeout=2)
        self.server.server_close()
        for name in (
            "MEMORY_AGENT_BENCH_MEMORY_URL",
            "MEMORY_AGENT_BENCH_MEMORY_API_TOKEN",
            "MEMORY_AGENT_BENCH_PROJECT_PREFIX",
            "MEMORY_AGENT_BENCH_ORIGIN",
        ):
            os.environ.pop(name, None)

    def test_memorize_captures_and_curates_structured_reference_memory(self):
        with TemporaryDirectory() as tmp:
            adapter = MemoryLayerAdapter(
                FakeWrapper(str(Path(tmp) / "exp_0")),
                {
                    "agent_name": "memory_layer",
                    "retrieve_num": 3,
                    "memory_service_url": self.service_url,
                    "memory_api_token": "secret",
                    "memory_project_prefix": "pilot",
                },
                {
                    "dataset": "Accurate_Retrieval",
                    "sub_dataset": "longmemeval_s",
                    "context_max_length": 1000,
                },
            )
            adapter.send_message("Ada wrote the first algorithm.", memorizing=True)

        self.assertEqual([path for path, _, _ in FakeMemoryHandler.requests], ["/v1/capture/task", "/v1/curate"])
        capture = FakeMemoryHandler.requests[0][1]
        self.assertEqual(capture["project"], "pilot-accurate_retrieval-longmemeval_s-exp_0")
        self.assertEqual(capture["structured_candidates"][0]["memory_type"], "reference")
        self.assertIn("Ada wrote the first algorithm.", capture["structured_candidates"][0]["canonical_text"])
        headers = {key.lower(): value for key, value in FakeMemoryHandler.requests[0][2].items()}
        self.assertEqual(headers["authorization"], "Bearer secret")

    def test_query_returns_memory_agent_bench_output_shape(self):
        with TemporaryDirectory() as tmp:
            adapter = MemoryLayerAdapter(
                FakeWrapper(str(Path(tmp) / "exp_1")),
                {
                    "agent_name": "memory_layer",
                    "retrieve_num": 3,
                    "memory_service_url": self.service_url,
                    "memory_api_token": "secret",
                },
                {
                    "dataset": "Accurate_Retrieval",
                    "sub_dataset": "longmemeval_s",
                    "context_max_length": 1000,
                },
            )
            output = adapter.send_message("Who wrote the notes?", memorizing=False, query_id=7, context_id=1)

        self.assertEqual(output["output"], "Ada Lovelace")
        self.assertEqual(output["input_len"], 12)
        self.assertEqual(output["output_len"], 2)
        self.assertIn("retrieval_context", output)
        self.assertEqual(FakeMemoryHandler.requests[-1][0], "/v1/query")
        self.assertEqual(FakeMemoryHandler.requests[-1][1]["answer_mode"], "llm")
        self.assertEqual(FakeMemoryHandler.requests[-1][1]["retrieval_mode"], "full-memory")

    def test_environment_overrides_config_and_can_send_local_origin(self):
        os.environ["MEMORY_AGENT_BENCH_MEMORY_URL"] = self.service_url
        os.environ["MEMORY_AGENT_BENCH_MEMORY_API_TOKEN"] = "env-secret"
        os.environ["MEMORY_AGENT_BENCH_PROJECT_PREFIX"] = "env-pilot"
        os.environ["MEMORY_AGENT_BENCH_ORIGIN"] = "http://127.0.0.1:4250"
        with TemporaryDirectory() as tmp:
            adapter = MemoryLayerAdapter(
                FakeWrapper(str(Path(tmp) / "exp_2")),
                {
                    "agent_name": "memory_layer",
                    "retrieve_num": 3,
                    "memory_service_url": "http://127.0.0.1:9",
                    "memory_api_token": "config-secret",
                    "memory_project_prefix": "config-pilot",
                },
                {
                    "dataset": "Conflict_Resolution",
                    "sub_dataset": "factconsolidation_sh_6k",
                    "context_max_length": 1000,
                },
            )
            adapter.send_message("goaltender is associated with ice hockey.", memorizing=True)

        capture = FakeMemoryHandler.requests[0][1]
        headers = {key.lower(): value for key, value in FakeMemoryHandler.requests[0][2].items()}
        self.assertEqual(capture["project"], "env-pilot-conflict_resolution-factconsolidation_sh_6k-exp_2")
        self.assertEqual(headers["authorization"], "Bearer env-secret")
        self.assertEqual(headers["origin"], "http://127.0.0.1:4250")


if __name__ == "__main__":
    unittest.main()
