#!/usr/bin/env python3
"""Regenerates flags/<code>.png from the game's own flag icons.

Codes are every scoreboard flag used in vehicles.json ("operator"/"operators"), the ten
tech-tree flags and EXTRA: team flags of event battles that no vehicle carries. Icons come from War-Thunder-Datamine (atlases.vromfs.bin_u/gameuiskin) and
are rendered to 40x26, the scoreboard size at 1440p. Needs rsvg-convert (librsvg).

    python3 tools/update_flags.py [vehicles.json] [flags_dir]
"""
import json, subprocess, sys, tempfile, urllib.request
from pathlib import Path

RAW = "https://raw.githubusercontent.com/gszabi99/War-Thunder-Datamine/master/atlases.vromfs.bin_u/gameuiskin"
TREES = ["usa", "germany", "ussr", "britain", "japan", "china", "italy", "france", "sweden", "israel"]
# event battles show these as a whole team's flag (WWI: triple_entente vs germany_empire)
# (the game's icon table has more country flags; only ones seen in battle go here)
EXTRA = ["triple_entente"]

def codes(db_path):
    db = json.load(open(db_path, encoding="utf-8-sig"))
    out = set(TREES) | set(EXTRA)
    for v in db.values():
        out.update(v.get("operators") or ([v["operator"]] if v.get("operator") else []))
    out.discard("invisible")
    return sorted(out)

def fetch(code):
    for name in (f"country_{code}.svg", f"flag_{code}.svg"):
        try:
            with urllib.request.urlopen(f"{RAW}/{name}", timeout=30) as r:
                return r.read()
        except Exception:
            pass
    return None

def main():
    db = sys.argv[1] if len(sys.argv) > 1 else "vehicles.json"
    out = Path(sys.argv[2] if len(sys.argv) > 2 else "flags")
    out.mkdir(exist_ok=True)
    missing = []
    with tempfile.TemporaryDirectory() as tmp:
        for code in codes(db):
            svg = fetch(code)
            if svg is None:
                missing.append(code)
                continue
            src = Path(tmp) / f"{code}.svg"
            src.write_bytes(svg)
            subprocess.run(["rsvg-convert", "-w", "40", "-h", "26", str(src), "-o", str(out / f"{code}.png")], check=True)
    print(f"{len(list(out.glob('*.png')))} flags in {out}" + (f", no icon for: {', '.join(missing)}" if missing else ""))

if __name__ == "__main__":
    main()
