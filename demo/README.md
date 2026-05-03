# Demo Video Pipeline

This directory contains a reproducible, privacy-safe pipeline for generating a
90-120 second Memory Layer demo video.

## Prerequisites

- Python 3.10 or newer
- `ffmpeg` with `libx264`, `libass`, and `mov_text` subtitle support
- `ffprobe`
- Optional: `ELEVENLABS_API_KEY` for narration

No terminal recording tool is required. Terminal output is scripted and rendered
from `demo/script.md`.

## Build

Dry-run mode builds a silent captioned MP4 and skips ElevenLabs:

```bash
./demo/make-demo.sh --dry-run
```

With narration:

```bash
export ELEVENLABS_API_KEY=...
./demo/make-demo.sh
```

Generated files are written under `demo/output/`, including:

- `memory-layer-demo.mp4`
- `captions.srt`
- `narration.txt`
- intermediate video/audio files

## ElevenLabs Configuration

The pipeline sends only narration text from `demo/script.md` to ElevenLabs. It
does not send raw captured memory data, local database contents, generated
terminal output, source files, or secrets.

Optional environment variables:

- `ELEVENLABS_VOICE_ID`: defaults to `21m00Tcm4TlvDq8ikWAM`
- `ELEVENLABS_MODEL_ID`: defaults to `eleven_multilingual_v2`
- `DEMO_OUTPUT`: output directory, defaults to `demo/output`

If `ELEVENLABS_API_KEY` is unset, the script automatically falls back to
dry-run mode.
