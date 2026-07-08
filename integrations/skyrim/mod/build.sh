#!/usr/bin/env bash
# Build the Memory Layer Chronicle mod on Linux, using the game's own
# Papyrus toolchain: the compiler front-end is a .NET assembly (runs under
# mono), the assembler is a native Win32 exe (runs under wine).
#
# Prereqs: mono, wine, unzip, python3, and the Skyrim SE Creation Kit
# installed via Steam (it provides "Papyrus Compiler" and Data/Scripts.zip).
set -euo pipefail

MOD_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GAME_DIR="${SKYRIM_DIR:-$HOME/.steam/steam/steamapps/common/Skyrim Special Edition}"
COMPILER_DIR="$GAME_DIR/Papyrus Compiler"
BUILD_DIR="$MOD_DIR/build"
DIST_DIR="$MOD_DIR/dist"

[ -x "$(command -v mono)" ] || { echo "mono is required" >&2; exit 1; }
[ -x "$(command -v wine)" ] || { echo "wine is required" >&2; exit 1; }
[ -f "$COMPILER_DIR/PapyrusCompiler.exe" ] || { echo "PapyrusCompiler.exe not found under '$COMPILER_DIR' — install the Creation Kit" >&2; exit 1; }

mkdir -p "$BUILD_DIR" "$DIST_DIR"

# Vanilla script sources ship zipped with the Creation Kit.
if [ ! -f "$BUILD_DIR/base/Source/Scripts/TESV_Papyrus_Flags.flg" ]; then
    echo "Extracting vanilla Papyrus sources..."
    unzip -oq "$GAME_DIR/Data/Scripts.zip" "Source/Scripts/*" -d "$BUILD_DIR/base"
fi

echo "Compiling MLChronicleQuest.psc..."
# The front-end succeeds but then fails trying to exec the native
# assembler; the .pas it leaves behind is what we want. Detect real
# compile errors by the absence of the .pas.
rm -f "$BUILD_DIR/MLChronicleQuest.pas" "$BUILD_DIR/MLChronicleQuest.pex"
(cd "$COMPILER_DIR" && mono PapyrusCompiler.exe \
    "$MOD_DIR/Source/MLChronicleQuest.psc" \
    -f="$BUILD_DIR/base/Source/Scripts/TESV_Papyrus_Flags.flg" \
    -i="$MOD_DIR/Source;$BUILD_DIR/base/Source/Scripts" \
    -o="$BUILD_DIR" 2>&1 | grep -v '^run-detectors:' || true)
[ -f "$BUILD_DIR/MLChronicleQuest.pas" ] || { echo "Papyrus compilation failed" >&2; exit 1; }

echo "Assembling to .pex..."
export WINEPREFIX="${WINEPREFIX:-$BUILD_DIR/winepfx}" WINEDEBUG=-all
(cd "$BUILD_DIR" && wine "$COMPILER_DIR/PapyrusAssembler.exe" MLChronicleQuest >/dev/null)
[ -f "$BUILD_DIR/MLChronicleQuest.pex" ] || { echo "Papyrus assembly failed" >&2; exit 1; }

echo "Generating plugin + SEQ..."
python3 "$MOD_DIR/build_esp.py" "$DIST_DIR"

mkdir -p "$DIST_DIR/Scripts"
cp "$BUILD_DIR/MLChronicleQuest.pex" "$DIST_DIR/Scripts/"
echo "Done. dist/ contains:"
find "$DIST_DIR" -type f | sed "s|$DIST_DIR/|  |"
