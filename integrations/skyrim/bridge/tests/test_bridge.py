"""Offline unit tests for the Skyrim bridge — no game, no network."""

import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from skyrim_memory_bridge import (  # noqa: E402
    BridgeState,
    SkyrimBridge,
    describe_place,
    parse_debug_form,
    parse_log_line,
    parse_save_header,
    prettify_editor_id,
)


def synth_save(save_number: int, name: str, level: int, location: str,
               game_date: str = "220.14.32", race: str = "NordRace") -> bytes:
    def w(text: str) -> bytes:
        raw = text.encode("cp1252")
        return struct.pack("<H", len(raw)) + raw

    header = struct.pack("<II", 12, save_number) + w(name) + struct.pack("<I", level)
    header += w(location) + w(game_date) + w(race)
    header += struct.pack("<H", 0) + struct.pack("<ff", 10.0, 100.0)
    return b"TESV_SAVEGAME" + struct.pack("<I", len(header)) + header + b"\0" * 64


class StubClient:
    def __init__(self):
        self.calls = []

    def remember(self, project, **kwargs):
        self.calls.append((project, kwargs))
        return {"output_count": 1}


class ParseTests(unittest.TestCase):
    def test_save_header_roundtrip(self):
        blob = synth_save(7, "Lydia", 23, "Whiterun")
        with tempfile.NamedTemporaryFile(suffix=".ess", delete=False) as fh:
            fh.write(blob)
            path = Path(fh.name)
        header = parse_save_header(path)
        self.assertEqual(header.save_number, 7)
        self.assertEqual(header.player_name, "Lydia")
        self.assertEqual(header.player_level, 23)
        self.assertEqual(header.player_location, "Whiterun")
        self.assertEqual(header.player_race, "NordRace")
        path.unlink()

    def test_non_save_rejected(self):
        with tempfile.NamedTemporaryFile(suffix=".ess", delete=False) as fh:
            fh.write(b"definitely not a save file")
            path = Path(fh.name)
        with self.assertRaises(ValueError):
            parse_save_header(path)
        path.unlink()

    def test_log_line(self):
        record = parse_log_line(
            "[07/08/2026 - 09:15:12PM] ML1|location|from=[Location <WhiterunLocation (00018A56)>]"
            "|to=[Location <WhiterunBanneredMareLocation (00016A02)>]|world=None|day=220.604167|level=23"
        )
        assert record is not None
        self.assertEqual(record["event"], "location")
        self.assertEqual(record["level"], "23")
        self.assertIn("BanneredMare", record["to"])

    def test_log_line_rejects_other_output(self):
        self.assertIsNone(parse_log_line("[07/08/2026 - 09:15:12PM] warning: unrelated"))
        self.assertIsNone(parse_log_line(""))

    def test_debug_form_and_prettify(self):
        parsed = parse_debug_form("[Location <WhiterunBanneredMareLocation (00016A02)>]")
        self.assertEqual(parsed, ("WhiterunBanneredMareLocation", "00016A02"))
        self.assertEqual(prettify_editor_id("WhiterunBanneredMareLocation"), "Whiterun Bannered Mare")
        self.assertEqual(prettify_editor_id("DLC2SolstheimLocation"), "Solstheim")
        self.assertEqual(describe_place("None"), "the wilderness")
        self.assertEqual(describe_place("[Location < (000FF001)>]"), "an unnamed area (000FF001)")


class BridgeFlowTests(unittest.TestCase):
    def make_bridge(self, tmp: Path) -> tuple[SkyrimBridge, StubClient]:
        client = StubClient()
        bridge = SkyrimBridge(client, "skyrim-test", tmp, BridgeState(), verbose=False)
        return bridge, client

    def test_saves_dedupe_by_number(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "Saves").mkdir()
            (tmp / "Saves" / "Save7.ess").write_bytes(synth_save(7, "Aela", 12, "Riverwood"))
            bridge, client = self.make_bridge(tmp)
            self.assertEqual(bridge.run_once(), 1)
            self.assertEqual(bridge.run_once(), 0)  # already remembered
            self.assertEqual(len(client.calls), 1)
            project, kwargs = client.calls[0]
            self.assertEqual(project, "skyrim-test")
            self.assertIn("Riverwood", kwargs["summary"])
            self.assertIn("skyrim", kwargs["tags"])

    def test_log_events_first_visit_only(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            log_dir = tmp / "Logs" / "Script" / "User"
            log_dir.mkdir(parents=True)
            loc = "[Location <RiftenLocation (00016BB4)>]"
            lines = [
                f"[07/08/2026 - 09:00:00PM] ML1|session|day=220.5|level=10|loc={loc}|world=None|gold=50",
                f"[07/08/2026 - 09:05:00PM] ML1|location|from=None|to={loc}|world=None|day=220.6|level=10",
                f"[07/08/2026 - 09:10:00PM] ML1|location|from=None|to={loc}|world=None|day=220.7|level=10",
                f"[07/08/2026 - 09:15:00PM] ML1|level|level=11|loc={loc}|day=220.8",
                f"[07/08/2026 - 09:16:00PM] ML1|combat|loc={loc}|day=220.8|level=11",
            ]
            (log_dir / "MemoryLayer.0.log").write_text("\n".join(lines) + "\n", encoding="cp1252")
            bridge, client = self.make_bridge(tmp)
            bridge.run_once()
            titles = [kwargs["title"] for _, kwargs in client.calls]
            self.assertEqual(len([t for t in titles if "first visit" in t]), 1)
            self.assertTrue(any("session started" in t for t in titles))
            self.assertTrue(any("reached level 11" in t for t in titles))
            self.assertFalse(any("combat" in t.lower() for t in titles))
            # incremental tail: nothing new on the second pass
            self.assertEqual(bridge.run_once(), 0)


if __name__ == "__main__":
    unittest.main()
