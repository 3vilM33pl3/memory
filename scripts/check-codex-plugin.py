#!/usr/bin/env python3
"""Check the repository-owned Memory Layer Codex plugin contract."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
PLUGIN_ROOT = ROOT / "plugins" / "memory-layer"


def load_json(path: Path) -> dict:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise AssertionError(f"invalid JSON at {path.relative_to(ROOT)}: {error}") from error
    if not isinstance(payload, dict):
        raise AssertionError(f"{path.relative_to(ROOT)} must contain a JSON object")
    return payload


def main() -> int:
    manifest = load_json(PLUGIN_ROOT / ".codex-plugin" / "plugin.json")
    assert manifest["name"] == "memory-layer"
    assert re.fullmatch(r"0\.1\.0(?:\+codex\.[0-9]{14})?", manifest["version"])
    assert manifest["skills"] == "./skills/"
    assert manifest["mcpServers"] == "./.mcp.json"
    assert manifest["license"] == "AGPL-3.0-or-later"
    assert manifest["interface"]["displayName"] == "Memory Layer"

    mcp = load_json(PLUGIN_ROOT / ".mcp.json")
    server = mcp["mcpServers"]["memory-layer"]
    assert server == {
        "command": "memory",
        "args": ["mcp", "run"],
        "env_vars": ["MEMORY_LAYER_CLIENT_TOKEN"],
    }

    skill = PLUGIN_ROOT / "skills" / "memory-layer-codex" / "SKILL.md"
    contents = skill.read_text(encoding="utf-8")
    assert contents.startswith("---\nname: memory-layer-codex\n")
    assert "memory_query" in contents
    assert "memory checkpoint start-execution" in contents
    assert "memory remember" in contents

    marketplace = load_json(ROOT / ".agents" / "plugins" / "marketplace.json")
    assert marketplace["name"] == "memory-layer"
    assert marketplace["plugins"] == [
        {
            "name": "memory-layer",
            "source": {"source": "local", "path": "./plugins/memory-layer"},
            "policy": {"installation": "AVAILABLE", "authentication": "ON_INSTALL"},
            "category": "Productivity",
        }
    ]

    print("Memory Layer Codex plugin contract passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, KeyError) as error:
        print(f"Memory Layer Codex plugin contract failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
