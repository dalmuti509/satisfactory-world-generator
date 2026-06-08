#!/usr/bin/env python3
"""Extract embedded world-outline.json from a built WASM binary."""
import json
import re
import sys
from pathlib import Path


def extract_outline(wasm_path: Path, output_path: Path) -> None:
    data = wasm_path.read_bytes()
    best_points: list[list[float]] | None = None

    for match in re.finditer(rb"\[\[0\.", data):
        start = match.start()
        chunk = data[start : start + 10_000_000]
        depth = 0
        end = 0
        for i, byte in enumerate(chunk):
            if byte == ord("["):
                depth += 1
            elif byte == ord("]"):
                depth -= 1
                if depth == 0:
                    end = i + 1
                    break

        if end <= 1000:
            continue

        try:
            parsed = json.loads(chunk[:end].decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            continue

        if not isinstance(parsed, list) or not parsed:
            continue

        if not isinstance(parsed[0], list):
            continue

        if not parsed[0] or not isinstance(parsed[0][0], (int, float)):
            continue

        # Flat list of [x, y] points (the format embedded in upstream builds).
        if len(parsed[0]) == 2 and isinstance(parsed[0][0], (int, float)):
            points = parsed
        else:
            continue

        if best_points is None or len(points) > len(best_points):
            best_points = points

    if best_points is None:
        raise SystemExit("Could not find world outline data in WASM")

    output_path.write_text(json.dumps(best_points), encoding="utf-8")
    print(f"Extracted {len(best_points)} outline points to {output_path}")


if __name__ == "__main__":
    wasm = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("swg.wasm")
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("../src/world-outline.json")
    extract_outline(wasm, out)
