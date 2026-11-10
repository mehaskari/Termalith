#!/usr/bin/env python3
"""Shrinks the guide's screenshots (needs Pillow): full windows to 1600 px wide,
dialogs to 760 px, 256-color PNG. Run after scripts/docs-screenshots.mjs."""
from pathlib import Path

from PIL import Image

DIR = Path(__file__).resolve().parent.parent / "website" / "docs" / "assets" / "screenshots"
for path in sorted(DIR.glob("*.png")):
    im = Image.open(path).convert("RGB")
    width = 760 if im.width <= 1100 else 1600  # dialogs are clipped narrow, windows full width
    if im.width > width:
        im = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)
    im = im.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    im.save(path, optimize=True)
    print(f"{path.name}: {path.stat().st_size // 1024} KB")
