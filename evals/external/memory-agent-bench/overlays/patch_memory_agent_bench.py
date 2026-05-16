"""Patch a pinned MemoryAgentBench checkout to expose Memory Layer as an agent."""

from __future__ import annotations

import argparse
from pathlib import Path


IMPORT = "from memory_layer_adapter import MemoryLayerAdapter\n"

INIT_NEEDLE = """        if 'Long_context_agent' in self.agent_name:
            self._initialize_long_context_agent()
"""
INIT_REPLACEMENT = """        if self._is_agent_type("memory_layer"):
            self.memory_layer_adapter = MemoryLayerAdapter(self, agent_config, dataset_config)
        elif 'Long_context_agent' in self.agent_name:
            self._initialize_long_context_agent()
"""

SEND_NEEDLE = """        if 'Long_context_agent' in self.agent_name:
            return self._handle_long_context_agent(message, memorizing)
"""
SEND_REPLACEMENT = """        if self._is_agent_type("memory_layer"):
            return self.memory_layer_adapter.send_message(message, memorizing, query_id, context_id)
        elif 'Long_context_agent' in self.agent_name:
            return self._handle_long_context_agent(message, memorizing)
"""

SAVE_NEEDLE = """        if not self._is_agent_type("letta") and not self._is_agent_type("zep"):
            print("\\n\\n Agent not saved (not implemented for this agent type) \\n\\n")
            return
"""
SAVE_REPLACEMENT = """        if self._is_agent_type("memory_layer"):
            self.memory_layer_adapter.save_agent()
            print("\\n\\n Memory Layer agent marker saved...\\n\\n")
            return
        if not self._is_agent_type("letta") and not self._is_agent_type("zep"):
            print("\\n\\n Agent not saved (not implemented for this agent type) \\n\\n")
            return
"""

LOAD_NEEDLE = """        if not self._is_agent_type("letta") and not self._is_agent_type("zep"):
            print("\\n\\nAgent loading not implemented for this agent type\\n\\n")
            return None
"""
LOAD_REPLACEMENT = """        if self._is_agent_type("memory_layer"):
            self.memory_layer_adapter.load_agent()
            print("\\n\\n Memory Layer agent marker loaded...\\n\\n")
            return None
        if not self._is_agent_type("letta") and not self._is_agent_type("zep"):
            print("\\n\\nAgent loading not implemented for this agent type\\n\\n")
            return None
"""


def replace_once(text: str, needle: str, replacement: str) -> str:
    if replacement in text:
        return text
    if needle not in text:
        raise RuntimeError(f"patch needle not found: {needle.splitlines()[0]}")
    return text.replace(needle, replacement, 1)


def patch_agent(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    if IMPORT not in text:
        text = text.replace("import time\n", f"import time\n{IMPORT}", 1)
    text = replace_once(text, INIT_NEEDLE, INIT_REPLACEMENT)
    text = replace_once(text, SEND_NEEDLE, SEND_REPLACEMENT)
    text = replace_once(text, SAVE_NEEDLE, SAVE_REPLACEMENT)
    text = replace_once(text, LOAD_NEEDLE, LOAD_REPLACEMENT)
    path.write_text(text, encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("checkout", type=Path)
    args = parser.parse_args()
    patch_agent(args.checkout / "agent.py")


if __name__ == "__main__":
    main()
