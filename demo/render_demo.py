#!/usr/bin/env python3
"""Render the scripted Memory Layer demo video."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import textwrap
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path


DEFAULT_VOICE_ID = "21m00Tcm4TlvDq8ikWAM"
DEFAULT_MODEL_ID = "eleven_multilingual_v2"
VIDEO_SIZE = "1920x1080"
FPS = "30"


@dataclass
class Scene:
    title: str
    duration: float
    caption: str
    narration: str
    terminal: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--script", default="demo/script.md", help="script markdown path")
    parser.add_argument(
        "--output-dir",
        default=os.environ.get("DEMO_OUTPUT", "demo/output"),
        help="directory for generated artifacts",
    )
    parser.add_argument("--dry-run", action="store_true", help="skip ElevenLabs audio")
    parser.add_argument("--check", action="store_true", help="validate only; do not render")
    return parser.parse_args()


def parse_script(path: Path) -> list[Scene]:
    text = path.read_text(encoding="utf-8")
    chunks = re.split(r"^## Scene \d+:\s+", text, flags=re.MULTILINE)[1:]
    scenes: list[Scene] = []
    for chunk in chunks:
        title, _, body = chunk.partition("\n")
        duration = extract_single(body, r"^Duration:\s*([0-9]+(?:\.[0-9]+)?)$", title)
        caption = extract_single(body, r"^Caption:\s*(.+)$", title)
        narration = extract_block(body, "Narration:", "Terminal:", title)
        terminal_match = re.search(r"Terminal:\s*```text\n(.*?)\n```", body, re.DOTALL)
        if not terminal_match:
            raise ValueError(f"{title}: missing Terminal fenced text block")
        scenes.append(
            Scene(
                title=title.strip(),
                duration=float(duration),
                caption=caption.strip(),
                narration=clean_block(narration),
                terminal=terminal_match.group(1).strip("\n"),
            )
        )
    if not scenes:
        raise ValueError(f"no scenes found in {path}")
    return scenes


def extract_single(body: str, pattern: str, title: str) -> str:
    match = re.search(pattern, body, re.MULTILINE)
    if not match:
        raise ValueError(f"{title}: missing field matching {pattern}")
    return match.group(1)


def extract_block(body: str, start: str, end: str, title: str) -> str:
    pattern = re.escape(start) + r"\n(.*?)\n" + re.escape(end)
    match = re.search(pattern, body, re.DOTALL)
    if not match:
        raise ValueError(f"{title}: missing {start} block")
    return match.group(1)


def clean_block(value: str) -> str:
    return "\n".join(line.strip() for line in value.strip().splitlines())


def validate_scenes(scenes: list[Scene]) -> float:
    total = sum(scene.duration for scene in scenes)
    if total < 90 or total > 120:
        raise ValueError(f"planned duration must be 90-120 seconds, got {total:.1f}")
    for scene in scenes:
        if not scene.narration:
            raise ValueError(f"{scene.title}: narration is empty")
        if not scene.caption:
            raise ValueError(f"{scene.title}: caption is empty")
    return total


def require_tool(name: str) -> None:
    if shutil.which(name) is None:
        raise RuntimeError(f"required tool not found on PATH: {name}")


def run(cmd: list[str]) -> None:
    printable = " ".join(cmd)
    print(f"+ {printable}", file=sys.stderr)
    subprocess.run(cmd, check=True)


def probe_duration(path: Path) -> float:
    result = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            str(path),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return float(result.stdout.strip())


def srt_timestamp(seconds: float) -> str:
    millis = int(round(seconds * 1000))
    hours, rem = divmod(millis, 3_600_000)
    minutes, rem = divmod(rem, 60_000)
    secs, ms = divmod(rem, 1000)
    return f"{hours:02}:{minutes:02}:{secs:02},{ms:03}"


def ass_timestamp(seconds: float) -> str:
    centis = int(round(seconds * 100))
    hours, rem = divmod(centis, 360_000)
    minutes, rem = divmod(rem, 6_000)
    secs, cs = divmod(rem, 100)
    return f"{hours}:{minutes:02}:{secs:02}.{cs:02}"


def write_transcript(path: Path, scenes: list[Scene]) -> None:
    parts = []
    for index, scene in enumerate(scenes, start=1):
        parts.append(f"Scene {index}: {scene.title}\n{scene.narration}")
    path.write_text("\n\n".join(parts) + "\n", encoding="utf-8")


def write_srt(path: Path, scenes: list[Scene], durations: list[float]) -> None:
    cursor = 0.0
    entries = []
    for index, (scene, duration) in enumerate(zip(scenes, durations), start=1):
        start = cursor
        end = cursor + duration
        entries.append(
            f"{index}\n{srt_timestamp(start)} --> {srt_timestamp(end)}\n"
            f"{wrap_caption(scene.caption)}\n"
        )
        cursor = end
    path.write_text("\n".join(entries), encoding="utf-8")


def wrap_caption(text: str) -> str:
    return "\n".join(textwrap.wrap(text, width=58))


def ass_escape(text: str) -> str:
    return (
        text.replace("\\", "\\\\")
        .replace("{", "(")
        .replace("}", ")")
        .replace("\n", r"\N")
    )


def write_ass(path: Path, scenes: list[Scene], durations: list[float]) -> None:
    header = """[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Title,DejaVu Sans,54,&H00F4F1E8,&H000000FF,&H00262B2F,&H00000000,-1,0,0,0,100,100,0,0,1,2,0,7,92,92,76,1
Style: Terminal,DejaVu Sans Mono,31,&H00D7E2D0,&H000000FF,&H00141618,&H00000000,0,0,0,0,100,100,0,0,1,1,0,7,116,116,178,1
Style: Caption,DejaVu Sans,38,&H00FFFFFF,&H000000FF,&H00101010,&HAA000000,-1,0,0,0,100,100,0,0,1,3,0,2,130,130,62,1
Style: Meta,DejaVu Sans,27,&H009CB8B1,&H000000FF,&H00141618,&H00000000,0,0,0,0,100,100,0,0,1,1,0,9,92,92,78,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""
    cursor = 0.0
    events = []
    for index, (scene, duration) in enumerate(zip(scenes, durations), start=1):
        start = ass_timestamp(cursor)
        end = ass_timestamp(cursor + duration)
        title = ass_escape(f"{index}. {scene.title}")
        terminal = ass_escape(scene.terminal)
        caption = ass_escape(wrap_caption(scene.caption))
        events.append(f"Dialogue: 1,{start},{end},Title,,0,0,0,,{title}")
        events.append(f"Dialogue: 1,{start},{end},Terminal,,0,0,0,,{terminal}")
        events.append(f"Dialogue: 1,{start},{end},Caption,,0,0,0,,{caption}")
        events.append(f"Dialogue: 1,{start},{end},Meta,,0,0,0,,Memory Layer demo")
        cursor += duration
    path.write_text(header + "\n".join(events) + "\n", encoding="utf-8")


def synthesize_audio(
    scenes: list[Scene],
    output_dir: Path,
    dry_run: bool,
) -> tuple[Path | None, list[float]]:
    if dry_run or not os.environ.get("ELEVENLABS_API_KEY"):
        if not dry_run:
            print("ELEVENLABS_API_KEY is unset; falling back to dry-run mode.", file=sys.stderr)
        return None, [scene.duration for scene in scenes]

    api_key = os.environ["ELEVENLABS_API_KEY"]
    voice_id = os.environ.get("ELEVENLABS_VOICE_ID", DEFAULT_VOICE_ID)
    model_id = os.environ.get("ELEVENLABS_MODEL_ID", DEFAULT_MODEL_ID)
    audio_files: list[Path] = []
    durations: list[float] = []

    for index, scene in enumerate(scenes, start=1):
        audio_path = output_dir / f"scene-{index:02}.mp3"
        payload = json.dumps(
            {
                "text": scene.narration,
                "model_id": model_id,
                "voice_settings": {"stability": 0.5, "similarity_boost": 0.75},
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            f"https://api.elevenlabs.io/v1/text-to-speech/{voice_id}",
            data=payload,
            headers={
                "Content-Type": "application/json",
                "Accept": "audio/mpeg",
                "xi-api-key": api_key,
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                audio_path.write_bytes(response.read())
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as error:
            print(f"ElevenLabs failed for scene {index}: {error}", file=sys.stderr)
            print("Falling back to dry-run mode without audio.", file=sys.stderr)
            for existing in audio_files:
                existing.unlink(missing_ok=True)
            return None, [scene.duration for scene in scenes]
        audio_files.append(audio_path)
        durations.append(max(scene.duration, probe_duration(audio_path) + 0.2))

    concat_file = output_dir / "audio-concat.txt"
    concat_file.write_text(
        "".join(f"file '{path.resolve()}'\n" for path in audio_files),
        encoding="utf-8",
    )
    audio_out = output_dir / "narration.mp3"
    run(["ffmpeg", "-y", "-f", "concat", "-safe", "0", "-i", str(concat_file), "-c", "copy", str(audio_out)])
    return audio_out, durations


def render_video(output_dir: Path, ass_path: Path, total_duration: float) -> Path:
    video_path = output_dir / "visual.mp4"
    vf = f"subtitles={ass_path}:fontsdir=/usr/share/fonts"
    run(
        [
            "ffmpeg",
            "-y",
            "-f",
            "lavfi",
            "-i",
            f"color=c=0x101418:s={VIDEO_SIZE}:r={FPS}:d={total_duration:.3f}",
            "-vf",
            vf,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            str(video_path),
        ]
    )
    return video_path


def mux_final(video_path: Path, audio_path: Path | None, srt_path: Path, final_path: Path) -> None:
    if audio_path:
        run(
            [
                "ffmpeg",
                "-y",
                "-i",
                str(video_path),
                "-i",
                str(audio_path),
                "-i",
                str(srt_path),
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-map",
                "2:0",
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-c:s",
                "mov_text",
                "-metadata:s:s:0",
                "language=eng",
                "-shortest",
                str(final_path),
            ]
        )
    else:
        run(
            [
                "ffmpeg",
                "-y",
                "-i",
                str(video_path),
                "-i",
                str(srt_path),
                "-map",
                "0:v:0",
                "-map",
                "1:0",
                "-c:v",
                "copy",
                "-c:s",
                "mov_text",
                "-metadata:s:s:0",
                "language=eng",
                str(final_path),
            ]
        )


def main() -> int:
    args = parse_args()
    script_path = Path(args.script)
    output_dir = Path(args.output_dir)
    scenes = parse_script(script_path)
    planned_duration = validate_scenes(scenes)
    print(f"Parsed {len(scenes)} scenes, planned duration {planned_duration:.1f}s")

    if args.check:
        return 0

    require_tool("ffmpeg")
    require_tool("ffprobe")
    output_dir.mkdir(parents=True, exist_ok=True)

    transcript_path = output_dir / "narration.txt"
    srt_path = output_dir / "captions.srt"
    ass_path = output_dir / "visual.ass"
    final_path = output_dir / "memory-layer-demo.mp4"

    write_transcript(transcript_path, scenes)
    audio_path, durations = synthesize_audio(scenes, output_dir, args.dry_run)
    total_duration = sum(durations)
    if total_duration < 90 or total_duration > 120:
        raise RuntimeError(f"rendered duration must be 90-120 seconds, got {total_duration:.1f}")

    write_srt(srt_path, scenes, durations)
    write_ass(ass_path, scenes, durations)
    video_path = render_video(output_dir, ass_path, total_duration)
    mux_final(video_path, audio_path, srt_path, final_path)

    final_duration = probe_duration(final_path)
    if final_duration < 90 or final_duration > 120:
        raise RuntimeError(f"final MP4 duration must be 90-120 seconds, got {final_duration:.1f}")
    print(f"Wrote {final_path} ({final_duration:.1f}s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
