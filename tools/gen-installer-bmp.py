"""Generate the NSIS installer bitmaps from the shipped lantern icon + design
tokens. Reproducible: re-run after an icon or token change, then commit the
BMPs. Windows-only (uses Segoe UI); the BMPs it emits are committed so CI never
runs this. Output: src-tauri/installer/{nsis-sidebar,nsis-header}.bmp."""

import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[1]
ICON = ROOT / "src-tauri" / "icons" / "icon.png"
OUT = ROOT / "src-tauri" / "installer"
OUT.mkdir(parents=True, exist_ok=True)

DARK_TOP = (0x26, 0x26, 0x26)  # --bg-surface dark
DARK_BOTTOM = (0x17, 0x17, 0x17)  # --bg-base dark
AMBER = (0xFC, 0xD3, 0x4D)  # lantern flame highlight
WORDMARK_LIGHT = (0xF5, 0xF5, 0xF5)
WORDMARK_DARK = (0x17, 0x17, 0x17)
WHITE = (0xFF, 0xFF, 0xFF)

# Header lantern x-anchor. The Tauri template defines MUI_HEADERIMAGE_BITMAP
# WITHOUT MUI_HEADERIMAGE_RIGHT, so MUI places the header image on the LEFT
# (default) and the page title text on the right -> lantern on the left. If a
# manual screenshot shows the title overlapping the lantern, flip to ~100.
HEADER_LANTERN_X = 12


def font(size):
    for path in (r"C:\Windows\Fonts\seguisb.ttf", r"C:\Windows\Fonts\segoeui.ttf"):
        if os.path.exists(path):
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def vgradient(w, h, top, bottom):
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        t = y / max(1, h - 1)
        row = tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        for x in range(w):
            px[x, y] = row
    return img


def main():
    lantern = Image.open(ICON).convert("RGBA")

    # --- sidebar 164x314 (Welcome + Finish) ---
    w, h = 164, 314
    side = vgradient(w, h, DARK_TOP, DARK_BOTTOM).convert("RGBA")
    cx, cy = w // 2, 120
    glow = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse([cx - 46, cy - 46, cx + 46, cy + 46], fill=(*AMBER, 90))
    glow = glow.filter(ImageFilter.GaussianBlur(26))
    side = Image.alpha_composite(side, glow)
    lh = 108
    lw = round(lantern.width * lh / lantern.height)
    side.alpha_composite(lantern.resize((lw, lh), Image.LANCZOS), (cx - lw // 2, cy - lh // 2))
    draw = ImageDraw.Draw(side)
    wm = font(23)
    tw = draw.textlength("Lucerna", font=wm)
    draw.text(((w - tw) / 2, 205), "Lucerna", font=wm, fill=WORDMARK_LIGHT)
    side.convert("RGB").save(OUT / "nsis-sidebar.bmp")

    # --- header 150x57 (inner pages): lantern only, no wordmark (avoids the
    #     title-collision in a 150px band shared with MUI's page title) ---
    hw, hh = 150, 57
    head = Image.new("RGB", (hw, hh), WHITE).convert("RGBA")
    lh2 = 40
    lw2 = round(lantern.width * lh2 / lantern.height)
    head.alpha_composite(lantern.resize((lw2, lh2), Image.LANCZOS), (HEADER_LANTERN_X, (hh - lh2) // 2))
    head.convert("RGB").save(OUT / "nsis-header.bmp")

    print("wrote", OUT / "nsis-sidebar.bmp", "and", OUT / "nsis-header.bmp")


if __name__ == "__main__":
    main()
