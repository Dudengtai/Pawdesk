"""Build multi-size ICO + wizard BMP from the tray PNG."""

from __future__ import annotations

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
# High-res head with alpha. 64px tray/icon.png is too small for Setup.exe.
SRC = ROOT / "assets" / "tray" / "icon_source.png"
OUT_ICO = Path(__file__).resolve().parent / "pawdesk.ico"
OUT_BMP = Path(__file__).resolve().parent / "wizard-small.bmp"
SIZES = (16, 24, 32, 48, 64, 128, 256)


def square_crop(im: Image.Image) -> Image.Image:
    w, h = im.size
    side = min(w, h)
    left = (w - side) // 2
    top = (h - side) // 2
    return im.crop((left, top, left + side, top + side))


def main() -> None:
    src = square_crop(Image.open(SRC).convert("RGBA"))
    # Pillow ICO writer: pass one large image + sizes; it resamples each entry.
    src.save(OUT_ICO, format="ICO", sizes=[(s, s) for s in SIZES])
    # Inno Setup WizardSmallImageFile: 64x64 24-bit BMP.
    small = src.resize((64, 64), Image.Resampling.LANCZOS).convert("RGB")
    small.save(OUT_BMP, format="BMP")
    print(f"wrote {OUT_ICO} ({OUT_ICO.stat().st_size} bytes) from {src.size}")
    print(f"wrote {OUT_BMP} ({OUT_BMP.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
