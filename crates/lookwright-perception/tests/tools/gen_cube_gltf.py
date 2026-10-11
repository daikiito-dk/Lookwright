#!/usr/bin/env python3
"""Generate tests/fixtures/cube.gltf for lookwright-perception.

Emits a minimal, valid glTF 2.0 document:

* one unit cube (8 corner positions, 36 indices = 12 triangles),
* one node named "Test Cube" (identity transform),
* one mesh with a single primitive (POSITION + indices),
* one embedded buffer as a base64 data URI, byte offsets aligned to 4.

Run from anywhere:  python3 tests/tools/gen_cube_gltf.py
The output path is resolved relative to this script so the fixture is
deterministic regardless of the current working directory.
"""

import base64
import json
import struct
from pathlib import Path

# 8 corners of a unit cube centered at the origin.
VERTICES = [
    (-0.5, -0.5, -0.5),  # 0
    (0.5, -0.5, -0.5),   # 1
    (0.5, -0.5, 0.5),    # 2
    (-0.5, -0.5, 0.5),   # 3
    (-0.5, 0.5, -0.5),   # 4
    (0.5, 0.5, -0.5),    # 5
    (0.5, 0.5, 0.5),     # 6
    (-0.5, 0.5, 0.5),    # 7
]

# 12 triangles (counter-clockwise seen from outside), flat index list.
TRIANGLES = [
    # bottom (y = -0.5)
    (0, 2, 1), (0, 3, 2),
    # top (y = +0.5)
    (4, 5, 6), (4, 6, 7),
    # front (z = -0.5)
    (0, 1, 5), (0, 5, 4),
    # back (z = +0.5)
    (2, 3, 7), (2, 7, 6),
    # left (x = -0.5)
    (0, 4, 7), (0, 7, 3),
    # right (x = +0.5)
    (1, 2, 6), (1, 6, 5),
]

POSITIONS_BYTE_LENGTH = len(VERTICES) * 3 * 4  # 96, multiple of 4
INDICES_BYTE_LENGTH = len(TRIANGLES) * 3 * 4   # 144, multiple of 4


def build_buffer() -> bytes:
    blob = b"".join(struct.pack("<3f", *v) for v in VERTICES)
    assert len(blob) == POSITIONS_BYTE_LENGTH
    # The indices bufferView starts at offset 96, which is 4-byte aligned.
    blob += b"".join(struct.pack("<I", i) for tri in TRIANGLES for i in tri)
    assert len(blob) == POSITIONS_BYTE_LENGTH + INDICES_BYTE_LENGTH
    return blob


def build_document(blob: bytes) -> dict:
    uri = "data:application/octet-stream;base64," + base64.b64encode(blob).decode("ascii")
    return {
        "asset": {"version": "2.0", "generator": "lookwright-perception gen_cube_gltf.py"},
        "scene": 0,
        "scenes": [{"name": "Scene", "nodes": [0]}],
        "nodes": [{"name": "Test Cube", "mesh": 0}],
        "meshes": [
            {
                "name": "Test Cube Mesh",
                "primitives": [
                    {
                        "attributes": {"POSITION": 0},
                        "indices": 1,
                        "mode": 4,
                    }
                ],
            }
        ],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,  # FLOAT
                "count": len(VERTICES),
                "type": "VEC3",
                "min": [-0.5, -0.5, -0.5],
                "max": [0.5, 0.5, 0.5],
            },
            {
                "bufferView": 1,
                "componentType": 5125,  # UNSIGNED_INT
                "count": len(TRIANGLES) * 3,
                "type": "SCALAR",
            },
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": POSITIONS_BYTE_LENGTH},
            {
                "buffer": 0,
                "byteOffset": POSITIONS_BYTE_LENGTH,
                "byteLength": INDICES_BYTE_LENGTH,
            },
        ],
        "buffers": [
            {
                "byteLength": POSITIONS_BYTE_LENGTH + INDICES_BYTE_LENGTH,
                "uri": uri,
            }
        ],
    }


def main() -> None:
    out = Path(__file__).resolve().parent.parent / "fixtures" / "cube.gltf"
    out.parent.mkdir(parents=True, exist_ok=True)
    blob = build_buffer()
    text = json.dumps(build_document(blob), indent=2) + "\n"
    out.write_text(text, encoding="utf-8")
    print(f"wrote {out} ({len(blob)} binary bytes embedded)")


if __name__ == "__main__":
    main()
