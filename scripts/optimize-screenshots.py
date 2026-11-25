#!/usr/bin/env python3
"""Recompresses the guide's screenshots (needs Pillow), keeping the 2x size
and full color: a 256-color palette lost the small link lights and thin
icons, and resizing made the files larger. Run after scripts/docs-screenshots.mjs."""
from pathlib import Path

from PIL import Image

DIR = Path(__file__).resolve().parent.parent / "website" / "docs" / "assets" / "screenshots"
for path in sorted(DIR.glob("*.png")):
    im = Image.open(path).convert("RGB")
    im.save(path, optimize=True)
    print(f"{path.name}: {path.stat().st_size // 1024} KB")
