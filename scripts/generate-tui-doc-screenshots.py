#!/usr/bin/env python3
from __future__ import annotations

import os
import math
import subprocess
import sys
import time
import uuid
from dataclasses import dataclass, replace
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parent.parent
TMUX = os.environ.get("TMUX_BIN", "/home/olivier/bin/tmux")
PROJECT = os.environ.get("MEMORY_LAYER_SCREENSHOT_PROJECT", "memory")
TUI_BIN = os.environ.get("MEMORY_LAYER_TUI_BIN", str(ROOT / "target" / "debug" / "mem-cli"))
WATCH_BIN = os.environ.get(
    "MEMORY_LAYER_WATCH_BIN", str(ROOT / "target" / "debug" / "memory-watch")
)
WIDTH = 184
HEIGHT = 48
OUTPUT_DIR = ROOT / "docs" / "img" / "tui"
DEFAULT_FG = (230, 236, 245)
DEFAULT_BG = (22, 31, 46)
FONT_NAME = os.environ.get("MEMORY_LAYER_SCREENSHOT_FONT", "DejaVuSansMono.ttf")
FONT_SIZE = 14
PADDING_X = 20
PADDING_Y = 18
QUERY_TEXT = "What is the main driver for coding agent interaction with Memory Layer?"


@dataclass(frozen=True)
class CellStyle:
    fg: tuple[int, int, int] = DEFAULT_FG
    bg: tuple[int, int, int] = DEFAULT_BG
    bold: bool = False


def run(*args: str, capture: bool = True, check: bool = True) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(args, cwd=ROOT, capture_output=capture, check=check)


def tmux(*args: str, capture: bool = True, check: bool = True) -> subprocess.CompletedProcess[bytes]:
    return run(TMUX, *args, capture=capture, check=check)


def send_keys(session: str, *keys: str, literal: bool = False) -> None:
    args = [TMUX, "send-keys", "-t", session]
    if literal:
        args.append("-l")
    args.extend(keys)
    subprocess.run(args, cwd=ROOT, check=True)


def capture_pane(session: str) -> bytes:
    return tmux("capture-pane", "-e", "-p", "-t", session).stdout


def sleep_for(seconds: float) -> None:
    time.sleep(seconds)


def start_session(name: str, command: str) -> None:
    shell_command = (
        f"cd {ROOT}; "
        f"LANG=C.UTF-8 LC_ALL=C.UTF-8 TERM=xterm-256color {command}"
    )
    tmux("new-session", "-d", "-x", str(WIDTH), "-y", str(HEIGHT), "-s", name, shell_command, capture=False)


def kill_session(name: str) -> None:
    subprocess.run([TMUX, "kill-session", "-t", name], cwd=ROOT, check=False, capture_output=True)


def parse_sgr(style: CellStyle, params: list[int]) -> CellStyle:
    if not params:
        params = [0]
    i = 0
    next_style = style
    while i < len(params):
        code = params[i]
        if code == 0:
            next_style = CellStyle()
        elif code == 1:
            next_style = replace(next_style, bold=True)
        elif code == 22:
            next_style = replace(next_style, bold=False)
        elif code == 39:
            next_style = replace(next_style, fg=DEFAULT_FG)
        elif code == 49:
            next_style = replace(next_style, bg=DEFAULT_BG)
        elif code == 38 and i + 4 < len(params) and params[i + 1] == 2:
            next_style = replace(
                next_style,
                fg=(params[i + 2], params[i + 3], params[i + 4]),
            )
            i += 4
        elif code == 48 and i + 4 < len(params) and params[i + 1] == 2:
            next_style = replace(
                next_style,
                bg=(params[i + 2], params[i + 3], params[i + 4]),
            )
            i += 4
        i += 1
    return next_style


def parse_ansi_screen(payload: bytes) -> list[list[tuple[str, CellStyle]]]:
    text = payload.decode("utf-8", errors="replace")
    lines: list[list[tuple[str, CellStyle]]] = []
    current: list[tuple[str, CellStyle]] = []
    style = CellStyle()
    i = 0
    while i < len(text):
        ch = text[i]
        if ch == "\x1b" and i + 1 < len(text) and text[i + 1] == "[":
            end = i + 2
            while end < len(text) and not text[end].isalpha():
                end += 1
            if end < len(text) and text[end] == "m":
                raw = text[i + 2 : end]
                params = [int(part) for part in raw.split(";") if part] if raw else [0]
                style = parse_sgr(style, params)
                i = end + 1
                continue
        if ch == "\n":
            lines.append(current)
            current = []
        elif ch != "\r":
            current.append((ch, style))
        i += 1
    if current:
        lines.append(current)
    return lines


def render_screen(payload: bytes, output_path: Path) -> None:
    lines = parse_ansi_screen(payload)
    if not lines:
        raise RuntimeError("captured screen is empty")

    font = ImageFont.truetype(FONT_NAME, FONT_SIZE)
    left, top, right, bottom = font.getbbox("M")
    cell_width = max(8, math.ceil(font.getlength("M")))
    cell_height = max(16, bottom - top + 4)
    cols = max(len(line) for line in lines)
    rows = len(lines)

    image = Image.new(
        "RGB",
        (cols * cell_width + PADDING_X * 2, rows * cell_height + PADDING_Y * 2),
        DEFAULT_BG,
    )
    draw = ImageDraw.Draw(image)

    for row, line in enumerate(lines):
        if not line:
            continue
        x = PADDING_X
        y = PADDING_Y + row * cell_height
        run_text = ""
        run_style = line[0][1]
        run_start = x

        def flush() -> None:
            nonlocal run_text, run_style, run_start
            if not run_text:
                return
            width = len(run_text) * cell_width
            draw.rectangle(
                [run_start, y, run_start + width, y + cell_height],
                fill=run_style.bg,
            )
            draw.text(
                (run_start, y - 1),
                run_text,
                font=font,
                fill=run_style.fg,
            )
            run_text = ""

        for col, (char, style) in enumerate(line):
            char_x = PADDING_X + col * cell_width
            if style != run_style:
                flush()
                run_style = style
                run_start = char_x
            if not run_text:
                run_start = char_x
            run_text += char
        flush()

    output_path.parent.mkdir(parents=True, exist_ok=True)
    image.save(output_path)


def main() -> int:
    run("cargo", "build", "--bin", "mem-cli", "--bin", "memory-watch", capture=False)

    tui_session = f"memory-docs-{uuid.uuid4().hex[:8]}"
    watcher_session = f"memory-watch-docs-{uuid.uuid4().hex[:8]}"

    screenshots: dict[str, bytes] = {}

    try:
        start_session(tui_session, f"{TUI_BIN} tui --project {PROJECT}")
        sleep_for(4.0)

        screenshots["resume-tab.png"] = capture_pane(tui_session)

        send_keys(tui_session, "Tab")
        sleep_for(1.0)
        memories = capture_pane(tui_session)
        screenshots["overview.png"] = memories
        screenshots["memories-tab.png"] = memories

        send_keys(tui_session, "?")
        sleep_for(0.3)
        send_keys(tui_session, QUERY_TEXT, literal=True)
        send_keys(tui_session, "Enter")
        sleep_for(1.2)
        screenshots["query-tab.png"] = capture_pane(tui_session)

        send_keys(tui_session, "Tab")
        sleep_for(0.6)
        screenshots["activity-tab.png"] = capture_pane(tui_session)

        send_keys(tui_session, "Tab")
        sleep_for(0.6)
        screenshots["project-tab.png"] = capture_pane(tui_session)

        kill_session(tui_session)

        start_session(watcher_session, f"{WATCH_BIN} run --project {PROJECT}")
        sleep_for(3.0)
        start_session(tui_session, f"{TUI_BIN} tui --project {PROJECT}")
        sleep_for(4.0)
        send_keys(tui_session, "Tab")
        send_keys(tui_session, "Tab")
        send_keys(tui_session, "Tab")
        send_keys(tui_session, "Tab")
        send_keys(tui_session, "Tab")
        sleep_for(1.0)
        screenshots["watchers-tab.png"] = capture_pane(tui_session)

    finally:
        kill_session(tui_session)
        kill_session(watcher_session)

    for name, payload in screenshots.items():
        render_screen(payload, OUTPUT_DIR / name)
        print(f"wrote {OUTPUT_DIR / name}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
