"""Unit tests with a stub session — no network, stdlib unittest only."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from memory_layer import MemoryLayerClient, MemoryLayerError, QueryAnswer  # noqa: E402


class StubResponse:
    def __init__(self, status_code, payload):
        self.status_code = status_code
        self._payload = payload
        self.text = str(payload)

    def json(self):
        return self._payload


class StubSession:
    def __init__(self, responses):
        self.responses = list(responses)
        self.calls = []

    def request(self, method, url, json=None, headers=None, timeout=None):
        self.calls.append({"method": method, "url": url, "json": json})
        return self.responses.pop(0)


class ClientTests(unittest.TestCase):
    def test_query_parses_answer_and_ignores_unknown_fields(self):
        session = StubSession(
            [
                StubResponse(
                    200,
                    {
                        "answer": "Port 7420.",
                        "confidence": 0.9,
                        "insufficient_evidence": False,
                        "results": [
                            {
                                "memory_id": "m1",
                                "summary": "Gateway on 7420",
                                "memory_type": "reference",
                                "score": 12.5,
                                "snippet": "…",
                                "tags": ["net"],
                                "future_field": "ignored",
                            }
                        ],
                        "answer_citations": [{"result_number": 1, "memory_id": "m1"}],
                        "brand_new_top_level_field": True,
                    },
                )
            ]
        )
        client = MemoryLayerClient(session=session, token="t")
        answer = client.query("demo", "which port?", deterministic=True)
        self.assertIsInstance(answer, QueryAnswer)
        self.assertEqual(answer.answer, "Port 7420.")
        self.assertFalse(answer.insufficient_evidence)
        self.assertEqual(answer.results[0].memory_id, "m1")
        self.assertEqual(answer.results[0].raw["future_field"], "ignored")
        self.assertEqual(session.calls[0]["json"]["answer_mode"], "deterministic")

    def test_remember_captures_then_curates_bounded(self):
        session = StubSession(
            [
                StubResponse(200, {"raw_capture_id": "cap-1"}),
                StubResponse(200, {"output_count": 1}),
            ]
        )
        client = MemoryLayerClient(session=session, token="t")
        result = client.remember("demo", title="t", summary="s", notes=["n"])
        self.assertEqual(result["output_count"], 1)
        self.assertEqual(session.calls[1]["json"]["raw_capture_id"], "cap-1")

    def test_errors_raise_with_status(self):
        session = StubSession([StubResponse(401, {"error": "invalid api token"})])
        client = MemoryLayerClient(session=session, token="wrong")
        with self.assertRaises(MemoryLayerError) as ctx:
            client.stats()
        self.assertEqual(ctx.exception.status, 401)


if __name__ == "__main__":
    unittest.main()
