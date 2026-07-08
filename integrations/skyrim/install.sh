#!/usr/bin/env bash
# Install (or remove) the Memory Layer Chronicle mod into a Steam/Proton
# Skyrim Special Edition. Everything is additive and reversible:
#   Data/MemoryLayerChronicle.esp        the quest plugin
#   Data/Scripts/MLChronicleQuest.pex    the compiled Papyrus script
#   Data/SEQ/MemoryLayerChronicle.seq    start-game-enabled quest registration
#   <My Games>/SkyrimCustom.ini          [Papyrus] logging switches (merged)
#   <AppData>/Plugins.txt                plugin activation (merged)
#
# Usage: ./install.sh [--uninstall]
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST="$HERE/mod/dist"
GAME_DIR="${SKYRIM_DIR:-$HOME/.steam/steam/steamapps/common/Skyrim Special Edition}"
PFX="${SKYRIM_PFX:-$HOME/.steam/steam/steamapps/compatdata/489830/pfx}"
MY_GAMES="$PFX/drive_c/users/steamuser/Documents/My Games/Skyrim Special Edition"
APPDATA_DIR="$PFX/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition"
PLUGIN="MemoryLayerChronicle.esp"

[ -d "$GAME_DIR/Data" ] || { echo "Skyrim SE not found at '$GAME_DIR' (set SKYRIM_DIR)" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$GAME_DIR/Data/$PLUGIN" \
          "$GAME_DIR/Data/Scripts/MLChronicleQuest.pex" \
          "$GAME_DIR/Data/SEQ/MemoryLayerChronicle.seq"
    [ -f "$APPDATA_DIR/Plugins.txt" ] && sed -i "/^\*\?$PLUGIN$/d" "$APPDATA_DIR/Plugins.txt"
    echo "Removed mod files and plugin activation. SkyrimCustom.ini left in place."
    exit 0
fi

[ -f "$DIST/$PLUGIN" ] || { echo "dist/ missing — run mod/build.sh first" >&2; exit 1; }

echo "Installing mod files into $GAME_DIR/Data"
install -m 644 "$DIST/$PLUGIN" "$GAME_DIR/Data/$PLUGIN"
install -D -m 644 "$DIST/Scripts/MLChronicleQuest.pex" "$GAME_DIR/Data/Scripts/MLChronicleQuest.pex"
install -D -m 644 "$DIST/SEQ/MemoryLayerChronicle.seq" "$GAME_DIR/Data/SEQ/MemoryLayerChronicle.seq"

echo "Enabling Papyrus logging in SkyrimCustom.ini"
mkdir -p "$MY_GAMES"
INI="$MY_GAMES/SkyrimCustom.ini"
if ! grep -qs "bEnableLogging=1" "$INI"; then
    printf '[Papyrus]\nbEnableLogging=1\nbEnableTrace=1\nbLoadDebugInformation=1\n' >> "$INI"
fi

echo "Activating plugin in Plugins.txt"
mkdir -p "$APPDATA_DIR"
PLUGINS="$APPDATA_DIR/Plugins.txt"
if ! grep -qs "^\*\?$PLUGIN$" "$PLUGINS"; then
    printf '*%s\n' "$PLUGIN" >> "$PLUGINS"
fi

echo
echo "Done. Launch Skyrim SE via Steam, load or start a game, and the"
echo "Chronicle quest starts silently. Verify with:"
echo "  tail -f '$MY_GAMES/Logs/Script/User/MemoryLayer.0.log'"
echo "Then run the bridge:"
echo "  python3 $HERE/bridge/skyrim_memory_bridge.py"
