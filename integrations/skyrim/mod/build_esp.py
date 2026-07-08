#!/usr/bin/env python3
"""Generate MemoryLayerChronicle.esp and its SEQ file.

The plugin contains exactly one record: a Start-Game-Enabled quest with the
MLChronicleQuest Papyrus script attached via VMAD. The binary layout mirrors
vanilla QUST records from Update.esm (DialogueGeneric: VMAD with one script
and no fragment section; DialogueGenericCommanded: minimal subrecord set;
DNAM flags 0x0011 = Start Game Enabled | Run Once).

Skyrim SE requires Start-Game-Enabled quests in new plugins to be listed in
Data/SEQ/<plugin>.seq (u32 little-endian form IDs, as stored in the plugin),
otherwise OnInit never fires on an existing save.

Usage: python3 build_esp.py <output-dir>
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

PLUGIN_NAME = "MemoryLayerChronicle"
SCRIPT_NAME = "MLChronicleQuest"
QUEST_EDID = "MLChronicleQuest"
QUEST_FULL = "Memory Layer Chronicle"
# One master (Skyrim.esm) -> our records live at load-order index 0x01.
QUEST_FORMID = 0x01000800
RECORD_VERSION = 44  # SE "form 44"
HEADER_VERSION = 1.71


def sub(sig: bytes, data: bytes) -> bytes:
    assert len(sig) == 4 and len(data) <= 0xFFFF
    return sig + struct.pack("<H", len(data)) + data


def zstring(text: str) -> bytes:
    return text.encode("ascii") + b"\0"


def wstring(text: str) -> bytes:
    raw = text.encode("ascii")
    return struct.pack("<H", len(raw)) + raw


def record(sig: bytes, formid: int, payload: bytes, flags: int = 0) -> bytes:
    return (
        sig
        + struct.pack("<IIII", len(payload), flags, formid, 0)
        + struct.pack("<HH", RECORD_VERSION, 0)
        + payload
    )


def vmad_single_script(script: str) -> bytes:
    # version 5, object format 2, one local script with zero properties.
    # Vanilla QUST DialogueGeneric (00013EB3) confirms the fragment/alias
    # section may be entirely absent when there are no fragments.
    return (
        struct.pack("<HHH", 5, 2, 1)
        + wstring(script)
        + bytes([0])  # status: local
        + struct.pack("<H", 0)  # property count
    )


def quest_record() -> bytes:
    payload = b"".join(
        [
            sub(b"EDID", zstring(QUEST_EDID)),
            sub(b"VMAD", vmad_single_script(SCRIPT_NAME)),
            sub(b"FULL", zstring(QUEST_FULL)),
            # DNAM: u16 flags (0x0011 = Start Game Enabled | Run Once),
            # u8 priority, u8 unknown, u32 unknown, u32 quest type (none).
            sub(b"DNAM", struct.pack("<HBBiI", 0x0011, 0, 0, 0, 0)),
            sub(b"NEXT", b""),
            sub(b"ANAM", struct.pack("<I", 0)),
        ]
    )
    return record(b"QUST", QUEST_FORMID, payload)


def tes4_header(record_and_group_count: int) -> bytes:
    payload = b"".join(
        [
            sub(b"HEDR", struct.pack("<fII", HEADER_VERSION, record_and_group_count, 0x801)),
            sub(b"CNAM", zstring("Memory Layer")),
            sub(b"SNAM", zstring("Chronicles player events to the MemoryLayer Papyrus user log.")),
            sub(b"MAST", zstring("Skyrim.esm")),
            sub(b"DATA", struct.pack("<Q", 0)),
        ]
    )
    return record(b"TES4", 0, payload)


def group(label: bytes, records: bytes) -> bytes:
    header = b"GRUP" + struct.pack("<I4siHHHH", 24 + len(records), label, 0, 0, 0, RECORD_VERSION, 0)
    return header + records


def build() -> tuple[bytes, bytes]:
    qust = quest_record()
    esp = tes4_header(2) + group(b"QUST", qust)
    seq = struct.pack("<I", QUEST_FORMID)
    return esp, seq


def verify(esp: bytes) -> None:
    """Re-parse the plugin with independent walking logic; raise on drift."""
    assert esp[0:4] == b"TES4"
    tes4_size = struct.unpack_from("<I", esp, 4)[0]
    off = 24 + tes4_size
    assert esp[off : off + 4] == b"GRUP"
    grup_size, label = struct.unpack_from("<I4s", esp, off + 4)
    assert label == b"QUST" and off + grup_size == len(esp)
    roff = off + 24
    sig, dsize, flags, formid = struct.unpack_from("<4sIII", esp, roff)
    assert sig == b"QUST" and flags == 0 and formid == QUEST_FORMID
    payload = esp[roff + 24 : roff + 24 + dsize]
    assert roff + 24 + dsize == len(esp)

    subs = []
    p = 0
    while p < len(payload):
        ssig = payload[p : p + 4].decode()
        ssize = struct.unpack_from("<H", payload, p + 4)[0]
        subs.append((ssig, payload[p + 6 : p + 6 + ssize]))
        p += 6 + ssize
    assert [s for s, _ in subs] == ["EDID", "VMAD", "FULL", "DNAM", "NEXT", "ANAM"], subs

    vmad = dict(subs)["VMAD"]
    ver, objfmt, nscripts = struct.unpack_from("<HHH", vmad, 0)
    assert (ver, objfmt, nscripts) == (5, 2, 1)
    nl = struct.unpack_from("<H", vmad, 6)[0]
    name = vmad[8 : 8 + nl].decode()
    status, nprops = vmad[8 + nl], struct.unpack_from("<H", vmad, 9 + nl)[0]
    assert (name, status, nprops) == (SCRIPT_NAME, 0, 0)
    assert 11 + nl == len(vmad), "VMAD must end after the scripts array"

    dnam = dict(subs)["DNAM"]
    assert len(dnam) == 12 and struct.unpack_from("<H", dnam, 0)[0] == 0x0011


def main() -> None:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("dist")
    out.mkdir(parents=True, exist_ok=True)
    esp, seq = build()
    verify(esp)
    (out / f"{PLUGIN_NAME}.esp").write_bytes(esp)
    seq_dir = out / "SEQ"
    seq_dir.mkdir(exist_ok=True)
    (seq_dir / f"{PLUGIN_NAME}.seq").write_bytes(seq)
    print(f"wrote {out / (PLUGIN_NAME + '.esp')} ({len(esp)} bytes) and SEQ ({len(seq)} bytes)")


if __name__ == "__main__":
    main()
