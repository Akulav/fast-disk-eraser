"""Generates the Fast Disk Eraser branding (run once; outputs are committed).

    python assets/make_icons.py

Writes:
  assets/icon.png        256x256 app icon (window, Linux menu, macOS .app, README)
  assets/icon.ico        multi-size Windows icon (exe, installer, uninstaller)
  assets/banner.png      1280x320 README banner
  installer/wizard.png   Inno Setup wizard side image (164x314)
  installer/wizard-small.png  Inno Setup small header image (55x55)

Design: a hard-disk platter cut by a lightning bolt on an ember gradient. Made by Akulav.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
INSTALLER = ROOT / "installer"

EMBER_TOP = (255, 150, 64)
EMBER_BOTTOM = (229, 46, 59)
INK = (14, 15, 19)


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def diagonal_gradient(size, c1, c2):
    w, h = size
    g = Image.new("RGB", (w, h))
    px = g.load()
    for y in range(h):
        for x in range(w):
            t = (x / max(w - 1, 1)) * 0.35 + (y / max(h - 1, 1)) * 0.65
            px[x, y] = lerp(c1, c2, t)
    return g


def bolt(cx, cy, s):
    """Lightning bolt polygon centred on (cx, cy), scale s (≈ height)."""
    pts = [
        (0.10, -0.50),
        (-0.22, 0.06),
        (-0.02, 0.06),
        (-0.12, 0.50),
        (0.24, -0.10),
        (0.03, -0.10),
        (0.16, -0.50),
    ]
    return [(cx + x * s, cy + y * s) for x, y in pts]


def app_icon(size=1024):
    s = size
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))

    # Rounded-square ember tile.
    tile = diagonal_gradient((s, s), EMBER_TOP, EMBER_BOTTOM).convert("RGBA")
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([s * 0.04, s * 0.04, s * 0.96, s * 0.96], radius=s * 0.22, fill=255)
    img.paste(tile, (0, 0), mask)

    # Soft top highlight.
    hl = Image.new("L", (s, s), 0)
    ImageDraw.Draw(hl).ellipse([-s * 0.2, -s * 0.75, s * 1.2, s * 0.45], fill=60)
    hl = Image.composite(hl, Image.new("L", (s, s), 0), mask).filter(ImageFilter.GaussianBlur(s * 0.03))
    img = Image.alpha_composite(img, Image.merge("RGBA", (*[Image.new("L", (s, s), 255)] * 3, hl)))

    # Drop shadow under the platter.
    shadow = Image.new("L", (s, s), 0)
    r = s * 0.30
    cx, cy = s * 0.5, s * 0.52
    ImageDraw.Draw(shadow).ellipse([cx - r, cy - r + s * 0.03, cx + r, cy + r + s * 0.03], fill=110)
    shadow = shadow.filter(ImageFilter.GaussianBlur(s * 0.035))
    img = Image.alpha_composite(img, Image.merge("RGBA", (*[Image.new("L", (s, s), 0)] * 3, shadow)))

    d = ImageDraw.Draw(img)
    # Platter.
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(250, 250, 252, 255))
    # Track rings.
    for k, a in ((0.80, 40), (0.60, 30)):
        rr = r * k
        d.ellipse([cx - rr, cy - rr, cx + rr, cy + rr], outline=(*INK, a), width=max(2, int(s * 0.008)))
    # Hub.
    hub = r * 0.17
    d.ellipse([cx - hub, cy - hub, cx + hub, cy + hub], fill=(*INK, 255))

    # Lightning bolt cut across the platter: dark outline, ember fill.
    pts = bolt(cx + s * 0.02, cy, s * 0.74)
    d.polygon(pts, fill=(*INK, 255))
    inner = bolt(cx + s * 0.02, cy, s * 0.62)
    bolt_fill = diagonal_gradient((s, s), (255, 196, 92), EMBER_TOP).convert("RGBA")
    bm = Image.new("L", (s, s), 0)
    ImageDraw.Draw(bm).polygon(inner, fill=255)
    img.paste(bolt_fill, (0, 0), bm)
    return img


def main():
    big = app_icon(1024)
    icon256 = big.resize((256, 256), Image.LANCZOS)
    icon256.save(ASSETS / "icon.png", optimize=True)
    sizes = [(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)]
    big.save(ASSETS / "icon.ico", sizes=sizes)

    # Banner for the README.
    bw, bh = 1280, 320
    banner = diagonal_gradient((bw, bh), (22, 24, 30), (10, 11, 14)).convert("RGBA")
    glow = Image.new("L", (bw, bh), 0)
    ImageDraw.Draw(glow).ellipse([40, 10, 360, 330], fill=120)
    glow = glow.filter(ImageFilter.GaussianBlur(60))
    banner = Image.alpha_composite(banner, Image.merge("RGBA", (Image.new("L", (bw, bh), EMBER_BOTTOM[0]), Image.new("L", (bw, bh), 70), Image.new("L", (bw, bh), 50), glow)))
    banner.alpha_composite(big.resize((220, 220), Image.LANCZOS), (90, 50))
    d = ImageDraw.Draw(banner)
    font_dir = ASSETS / "fonts"
    title = ImageFont.truetype(str(font_dir / "Inter-ExtraBold.otf"), 84)
    sub = ImageFont.truetype(str(font_dir / "Inter-SemiBold.otf"), 30)
    d.text((360, 92), "Fast Disk Eraser", font=title, fill=(240, 242, 246))
    d.text((364, 198), "Securely erase files and free space  ·  made by Akulav", font=sub, fill=(255, 150, 64))
    banner.convert("RGB").save(ASSETS / "banner.png", optimize=True)

    # Inno Setup wizard images (classic sizes; modern style scales them).
    INSTALLER.mkdir(exist_ok=True)
    side = diagonal_gradient((164, 314), (26, 28, 35), (10, 11, 14)).convert("RGBA")
    side.alpha_composite(big.resize((112, 112), Image.LANCZOS), (26, 60))
    ds = ImageDraw.Draw(side)
    f1 = ImageFont.truetype(str(font_dir / "Inter-ExtraBold.otf"), 19)
    f2 = ImageFont.truetype(str(font_dir / "Inter-SemiBold.otf"), 12)
    for i, line in enumerate(("Fast Disk", "Eraser")):
        w = ds.textlength(line, font=f1)
        ds.text(((164 - w) / 2, 192 + i * 24), line, font=f1, fill=(240, 242, 246))
    w = ds.textlength("made by Akulav", font=f2)
    ds.text(((164 - w) / 2, 270), "made by Akulav", font=f2, fill=(255, 150, 64))
    side.convert("RGB").save(INSTALLER / "wizard.png")
    small = Image.new("RGBA", (55, 55), (255, 255, 255, 255))
    small.alpha_composite(big.resize((51, 51), Image.LANCZOS), (2, 2))
    small.convert("RGB").save(INSTALLER / "wizard-small.png")
    print("icons written")


if __name__ == "__main__":
    main()
