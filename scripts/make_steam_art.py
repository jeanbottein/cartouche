#!/usr/bin/env python3
"""
Generate the Steam artwork used when Cartouche adds itself as a shortcut.

Retro style matching cartouche-ui: black background, the cartridge icon, the
VT323 pixel font, and an amber "cartridge stripe" on the leading edge (the same
accent the UI draws on each library row).

Usage:
    python3 scripts/make_steam_art.py            # writes lib/steam-art/
    python3 scripts/make_steam_art.py OUT_DIR
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

REPO = Path(__file__).resolve().parent.parent
ICON = Image.open(REPO / "lib/app-icon.png").convert("RGBA")
FONT = str(REPO / "cartouche-ui/assets/VT323-Regular.ttf")
TITLE = ".cartouche"

# Retro UI palette (cartouche-ui/src/engine/theme.rs)
BG = (0, 0, 0)
ACCENT = (38, 168, 108)
AMBER = (160, 82, 16)
BORDER = (65, 48, 30)


def centered_text(d, cx, y, text, size, fill):
    f = ImageFont.truetype(FONT, size)
    w = d.textlength(text, font=f)
    d.text((cx - w / 2, y), text, font=f, fill=fill)


def frame(d, w, h, inset, th):
    d.rectangle([inset, inset, w - inset - 1, h - inset - 1], outline=BORDER, width=th)


def paste_icon(img, size, cx, cy):
    ic = ICON.resize((size, size), Image.LANCZOS)
    img.alpha_composite(ic, (int(cx - size / 2), int(cy - size / 2)))


def cover():
    """Portrait library capsule (600x900)."""
    w, h = 600, 900
    img = Image.new("RGBA", (w, h), BG)
    d = ImageDraw.Draw(img)
    frame(d, w, h, 18, 8)
    d.rectangle([18, 18, 50, h - 19], fill=AMBER)
    paste_icon(img, 340, w / 2 + 16, 390)
    centered_text(d, w / 2 + 16, 620, TITLE, 120, ACCENT)
    return img


def wide():
    """Horizontal capsule (920x430)."""
    w, h = 920, 430
    img = Image.new("RGBA", (w, h), BG)
    d = ImageDraw.Draw(img)
    frame(d, w, h, 14, 7)
    d.rectangle([14, 14, 40, h - 15], fill=AMBER)
    paste_icon(img, 250, 220, h / 2)
    f = ImageFont.truetype(FONT, 112)
    top = d.textbbox((0, 0), TITLE, font=f)
    d.text((374, h / 2 - (top[1] + top[3]) / 2), TITLE, font=f, fill=ACCENT)
    return img


def hero():
    """Game page banner (3840x1240). Steam overlays the title bottom-left, so
    the art sits on the right."""
    w, h = 3840, 1240
    img = Image.new("RGBA", (w, h), BG)
    paste_icon(img, 760, w - 900, h / 2 - 20)
    return img


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else REPO / "lib/steam-art"
    out.mkdir(parents=True, exist_ok=True)
    for name, fn in [("cover.png", cover), ("wide.png", wide), ("hero.png", hero)]:
        fn().convert("RGB").save(out / name, optimize=True)
        print("wrote", out / name)


if __name__ == "__main__":
    main()
