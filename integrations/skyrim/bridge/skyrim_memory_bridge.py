#!/usr/bin/env python3
"""Skyrim → Memory Layer bridge.

Watches two data channels and converts them into Memory Layer memories via
the Python client (clients/python):

1. **Save files** (works with a completely unmodded game): every new
   ``Saves/*.ess`` header carries the character name, level, location
   display name, and in-game date — each new save becomes a checkpoint
   memory, deduped by save number.

2. **The Chronicle mod's Papyrus user log**
   (``Logs/Script/User/MemoryLayer.0.log``): live events traced by
   MLChronicleQuest — session starts, level-ups, first visits to
   locations, large gold swings. Combat events are parsed but not stored
   individually (too noisy); they are counted into the session state.

Location identity from the mod comes as Papyrus debug strings like
``[Location <WhiterunBanneredMareLocation (00016A02)>]`` — LCTN/WRLD
records keep their editor IDs at runtime, so no SKSE is required; the
bridge prettifies the CamelCase editor ID into "Whiterun Bannered Mare".

Run once (cron/test): ``skyrim_memory_bridge.py --once``
Run as a daemon:      ``skyrim_memory_bridge.py``
"""

from __future__ import annotations

import argparse
import json
import os
import re
import struct
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# Allow running straight from the repo without installing the client.
_REPO_CLIENT = Path(__file__).resolve().parents[3] / "clients" / "python" / "src"
if _REPO_CLIENT.is_dir():
    sys.path.insert(0, str(_REPO_CLIENT))

from memory_layer import MemoryLayerClient  # noqa: E402

DEFAULT_GAME_DOCS = Path(
    "~/.steam/steam/steamapps/compatdata/489830/pfx/drive_c/users/steamuser"
    "/Documents/My Games/Skyrim Special Edition"
).expanduser()
DEFAULT_STATE = Path("~/.local/state/memory-layer/skyrim-bridge.json").expanduser()

SAVE_MAGIC = b"TESV_SAVEGAME"
LOG_LINE = re.compile(r"^\[[^\]]+\]\s*ML1\|(.*)$")
DEBUG_FORM = re.compile(r"\[\w+ <(\w*)\s*\(([0-9A-Fa-f]{8})\)>\]")


# --------------------------------------------------------------------------
# Parsing


@dataclass
class SaveHeader:
    save_number: int
    player_name: str
    player_level: int
    player_location: str
    game_date: str
    player_race: str


def parse_save_header(path: Path) -> SaveHeader:
    """Parse the fixed header of a Skyrim SE .ess save file."""
    with path.open("rb") as fh:
        blob = fh.read(4096)
    if not blob.startswith(SAVE_MAGIC):
        raise ValueError(f"{path.name}: not a TESV save")
    off = len(SAVE_MAGIC) + 4  # skip headerSize
    _version, save_number = struct.unpack_from("<II", blob, off)
    off += 8

    def wstring() -> str:
        nonlocal off
        (length,) = struct.unpack_from("<H", blob, off)
        off += 2
        value = blob[off : off + length].decode("cp1252", "replace")
        off += length
        return value

    player_name = wstring()
    (player_level,) = struct.unpack_from("<I", blob, off)
    off += 4
    player_location = wstring()
    game_date = wstring()
    player_race = wstring()
    return SaveHeader(
        save_number=save_number,
        player_name=player_name,
        player_level=player_level,
        player_location=player_location,
        game_date=game_date,
        player_race=player_race,
    )


def parse_debug_form(text: str) -> tuple[str, str] | None:
    """('WhiterunBanneredMareLocation', '00016A02') from a Papyrus debug string."""
    match = DEBUG_FORM.search(text)
    if not match:
        return None
    return match.group(1), match.group(2).upper()


def prettify_editor_id(editor_id: str) -> str:
    """WhiterunBanneredMareLocation -> 'Whiterun Bannered Mare'."""
    name = re.sub(r"(Location|Zone|Marker)$", "", editor_id)
    name = re.sub(r"^(DLC\d|BYOH|CC|MS|MQ)", "", name)
    words = re.findall(r"[A-Z]+(?=[A-Z][a-z])|[A-Z][a-z]+|[A-Z]+|\d+", name)
    return " ".join(words) if words else editor_id


def describe_place(raw: str) -> str:
    parsed = parse_debug_form(raw)
    if parsed is None or not parsed[0]:
        if parsed is not None:
            return f"an unnamed area ({parsed[1]})"
        return "the wilderness"
    return prettify_editor_id(parsed[0])


def parse_log_line(line: str) -> dict[str, str] | None:
    """One 'ML1|event|k=v|...' trace line -> {'event': ..., k: v, ...}."""
    match = LOG_LINE.match(line.strip())
    if not match:
        return None
    parts = match.group(1).split("|")
    record: dict[str, str] = {"event": parts[0]}
    for part in parts[1:]:
        key, _, value = part.partition("=")
        if key:
            record[key] = value
    return record


def game_day(record: dict[str, str]) -> str:
    """Human-ish rendering of Utility.GetCurrentGameTime() float days."""
    try:
        return f"game day {float(record.get('day', '0')):.1f}"
    except ValueError:
        return "game day ?"


# --------------------------------------------------------------------------
# Bridge


@dataclass
class BridgeState:
    save_numbers: list[int] = field(default_factory=list)
    seen_locations: list[str] = field(default_factory=list)
    log_offset: int = 0
    log_signature: str = ""

    @classmethod
    def load(cls, path: Path) -> "BridgeState":
        if path.is_file():
            data = json.loads(path.read_text())
            return cls(**{k: v for k, v in data.items() if k in cls.__dataclass_fields__})
        return cls()

    def save(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(self.__dict__, indent=2))


class SkyrimBridge:
    def __init__(
        self,
        client: MemoryLayerClient,
        project: str,
        game_docs: Path,
        state: BridgeState,
        verbose: bool = True,
    ):
        self.client = client
        self.project = project
        self.saves_dir = game_docs / "Saves"
        self.log_path = game_docs / "Logs" / "Script" / "User" / "MemoryLayer.0.log"
        self.state = state
        self.verbose = verbose

    def remember(self, title: str, summary: str, notes: list[str], tags: list[str], importance: int = 2) -> None:
        if self.verbose:
            print(f"  + {title}")
        self.client.remember(
            self.project,
            title=title,
            summary=summary,
            notes=notes,
            tags=["skyrim", "chronicle", *tags],
            importance=importance,
        )

    # -- channel 1: save files ------------------------------------------

    def scan_saves(self) -> int:
        if not self.saves_dir.is_dir():
            return 0
        stored = 0
        for path in sorted(self.saves_dir.glob("*.ess"), key=lambda p: p.stat().st_mtime):
            try:
                header = parse_save_header(path)
            except (ValueError, struct.error) as error:
                if self.verbose:
                    print(f"  ! skipping {path.name}: {error}", file=sys.stderr)
                continue
            if header.save_number in self.state.save_numbers:
                continue
            self.remember(
                title=f"Skyrim checkpoint: {header.player_name} at {header.player_location}",
                summary=(
                    f"In Skyrim, {header.player_name} (level {header.player_level} "
                    f"{header.player_race.removesuffix('Race')}) saved at "
                    f"{header.player_location} on in-game date {header.game_date} "
                    f"(save #{header.save_number})."
                ),
                notes=[
                    f"Character {header.player_name} was last at {header.player_location} "
                    f"at level {header.player_level}.",
                ],
                tags=["save", header.player_name.lower()],
                importance=3,
            )
            self.state.save_numbers.append(header.save_number)
            stored += 1
        return stored

    # -- channel 2: the Chronicle mod's user log -------------------------

    def read_new_log_lines(self) -> list[str]:
        if not self.log_path.is_file():
            return []
        stat = self.log_path.stat()
        signature = f"{stat.st_ino}:{stat.st_ctime_ns}"
        if signature != self.state.log_signature or stat.st_size < self.state.log_offset:
            # Rotated or truncated: Papyrus starts a fresh .0 log per session.
            self.state.log_offset = 0
            self.state.log_signature = signature
        with self.log_path.open("r", encoding="cp1252", errors="replace") as fh:
            fh.seek(self.state.log_offset)
            chunk = fh.read()
            self.state.log_offset = fh.tell()
        return chunk.splitlines()

    def handle_event(self, record: dict[str, str]) -> None:
        event = record.get("event")
        when = game_day(record)
        place = describe_place(record.get("loc", record.get("to", "")))

        if event == "session":
            self.remember(
                title=f"Skyrim session started at {place}",
                summary=(
                    f"Started a Skyrim play session at {place}, level "
                    f"{record.get('level', '?')}, {when}."
                ),
                notes=[f"The playthrough resumed from {place} at level {record.get('level', '?')}."],
                tags=["session"],
            )
        elif event == "level":
            self.remember(
                title=f"Skyrim: reached level {record.get('level', '?')}",
                summary=f"Leveled up to {record.get('level', '?')} at {place}, {when}.",
                notes=[f"Character level {record.get('level', '?')} was reached at {place}."],
                tags=["milestone"],
                importance=3,
            )
        elif event == "location":
            parsed = parse_debug_form(record.get("to", ""))
            key = parsed[1] if parsed else None
            if key and key not in self.state.seen_locations:
                self.state.seen_locations.append(key)
                self.remember(
                    title=f"Skyrim: first visit to {place}",
                    summary=f"Discovered {place} for the first time, {when} (level {record.get('level', '?')}).",
                    notes=[f"{place} was first visited around {when}."],
                    tags=["exploration"],
                )
        elif event == "gold":
            self.remember(
                title=f"Skyrim: gold swing of {record.get('delta', '?')}",
                summary=(
                    f"Gold changed by {record.get('delta', '?')} to "
                    f"{record.get('gold', '?')} at {place}, {when}."
                ),
                notes=[f"Treasury stood at {record.get('gold', '?')} gold after {place}."],
                tags=["economy"],
                importance=1,
            )
        # 'combat' events are deliberately not stored one-by-one.

    def run_once(self) -> int:
        handled = self.scan_saves()
        for line in self.read_new_log_lines():
            record = parse_log_line(line)
            if record:
                self.handle_event(record)
                handled += 1
        return handled


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--project", default="skyrim")
    parser.add_argument("--base-url", default="http://127.0.0.1:4040")
    parser.add_argument("--game-docs", type=Path, default=DEFAULT_GAME_DOCS,
                        help="My Games/Skyrim Special Edition directory")
    parser.add_argument("--state-file", type=Path, default=DEFAULT_STATE)
    parser.add_argument("--interval", type=float, default=15.0)
    parser.add_argument("--once", action="store_true", help="single scan, then exit")
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()

    # The client reads MEMORY_API_TOKEN; fall back to the token the installer
    # provisions into ~/.config/memory-layer/memory-layer.env.
    token = os.environ.get("MEMORY_API_TOKEN")
    if not token:
        env_file = Path("~/.config/memory-layer/memory-layer.env").expanduser()
        if env_file.is_file():
            for line in env_file.read_text().splitlines():
                if line.startswith("MEMORY_LAYER__SERVICE__API_TOKEN="):
                    token = line.split("=", 1)[1].strip()
    client = MemoryLayerClient(base_url=args.base_url, token=token, writer_id="skyrim-bridge")
    state = BridgeState.load(args.state_file)
    bridge = SkyrimBridge(client, args.project, args.game_docs, state, verbose=not args.quiet)

    if not args.quiet:
        print(f"Watching {args.game_docs} -> project '{args.project}' at {args.base_url}")
    try:
        while True:
            count = bridge.run_once()
            state.save(args.state_file)
            if args.once:
                if not args.quiet:
                    print(f"Stored {count} new event(s).")
                return
            time.sleep(args.interval)
    except KeyboardInterrupt:
        state.save(args.state_file)


if __name__ == "__main__":
    main()
