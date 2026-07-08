# Skyrim → Memory Layer integration

Chronicles a Skyrim Special Edition playthrough into a Memory Layer project
so `memory query --project skyrim --question "Where did I leave off?"`
answers with citations into your own play history. User-facing walkthrough:
[docs-site recipe](../../docs-site/content/docs/recipes/skyrim.mdx).

## Layout

```
mod/Source/MLChronicleQuest.psc   vanilla-Papyrus quest script (no SKSE)
mod/build_esp.py                  hand-authors the .esp + SEQ, self-verifying
mod/build.sh                      full build: psc -> pex (mono+wine), esp, dist/
mod/dist/                         committed build artifacts, ready to install
bridge/skyrim_memory_bridge.py    saves + Papyrus log -> memories (Python client)
bridge/tests/                     offline unit tests (stdlib unittest)
install.sh                        copy into game + Proton prefix, idempotent
```

## How the pieces fit

- The **quest script** polls the player every 10 real seconds and traces
  `ML1|event|k=v|...` lines via `Debug.TraceUser` to
  `Logs/Script/User/MemoryLayer.0.log`. Vanilla Papyrus only: player via
  `Game.GetPlayer()` (no CK property fills), gold via `Game.GetForm(0xF)`,
  new-session detection via `Utility.GetCurrentRealTime()` going backwards.
  Location names come from debug strings — LCTN/WRLD records keep editor IDs
  at runtime, so SKSE's `GetName()` is not needed.
- The **plugin** is a minimal form-44 esp: TES4 header + one QUST record
  (`EDID VMAD FULL DNAM NEXT ANAM`), mirroring vanilla `DialogueGeneric` —
  which proves a QUST VMAD may end right after the scripts array when there
  are no fragments. DNAM flags 0x0011 = Start Game Enabled | Run Once. The
  SEQ file is required for start-game-enabled quests in SE.
- The **bridge** also parses `.ess` save headers (name, level, location
  display string, in-game date) so checkpoints work on a completely unmodded
  game. State (seen saves, first-visited locations, log offset) lives in
  `~/.local/state/memory-layer/skyrim-bridge.json`.

## Build

Requires: Steam Skyrim SE + Creation Kit (for `Papyrus Compiler/` and
`Data/Scripts.zip`), `mono`, `wine`, `unzip`, `python3`.

```bash
./mod/build.sh          # SKYRIM_DIR overrides the default Steam path
```

The Papyrus front-end is a .NET assembly and runs under mono; it then fails
to exec the native Win32 assembler, so build.sh runs `PapyrusAssembler.exe`
under wine on the emitted `.pas`. Compile success is detected by the `.pas`
appearing, not the front-end's exit status.

## Test

```bash
cd bridge && python3 -m unittest discover -s tests   # offline, 7 tests
```

End-to-end against a dev service: point `--game-docs` at a directory with
synthetic `Saves/*.ess` + `Logs/Script/User/MemoryLayer.0.log` fixtures and
run with `--once --base-url http://127.0.0.1:4250` (dev-stack token via
`MEMORY_API_TOKEN`, see the per-project `memory-layer.env`).

## Install / run

```bash
./install.sh            # or --uninstall; SKYRIM_PFX overrides the prefix
python3 bridge/skyrim_memory_bridge.py --project skyrim
```
