"""Render the Remote Access Crew logo (same geometry as assets/remote-access-crew.svg)
into assets/remote-access-crew.ico and assets/remote-access-crew.png. Needs Pillow."""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent / "assets"
S = 16  # supersampling: draw at 1024px, scale down
BG, BACK, FRONT, LINK = "#2a1b4a", "#c4b5fd", "#a78bfa", "#f5f3ff"


def p(*xy):
    return [v * S for v in xy]


def draw() -> Image.Image:
    im = Image.new("RGBA", (64 * S, 64 * S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    w = int(3.2 * S)
    d.rounded_rectangle(p(2, 2, 62, 62), radius=14 * S, fill=BG)
    d.rounded_rectangle(p(25, 11, 53, 30), radius=3 * S, outline=BACK, width=w)
    d.line(p(39, 30, 39, 34), fill=BACK, width=w)
    d.line(p(34, 35, 44, 35), fill=BACK, width=w)
    d.rounded_rectangle(p(11, 27, 39, 46), radius=3 * S, fill=BG, outline=FRONT, width=w)
    d.line(p(25, 46, 25, 50), fill=FRONT, width=w)
    d.line(p(20, 51, 30, 51), fill=FRONT, width=w)
    lw = int(2.6 * S)
    d.line(p(17, 37, 33, 37), fill=LINK, width=lw)
    d.line(p(21, 33, 17, 37, 21, 41), fill=LINK, width=lw, joint="curve")
    d.line(p(29, 33, 33, 37, 29, 41), fill=LINK, width=lw, joint="curve")
    return im


if __name__ == "__main__":
    big = draw().resize((256, 256), Image.LANCZOS)
    big.save(ROOT / "remote-access-crew.png")
    big.save(ROOT / "remote-access-crew.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print("wrote", ROOT / "remote-access-crew.ico")
