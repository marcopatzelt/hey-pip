#!/usr/bin/env python3
"""Clean the Pip poses for the overlay: remove the white floor shadow, drop stray specks,
put every pose on the same 384x384 canvas with the feet on one baseline (no jump when the pose changes).

usage: python3 clean_pip.py <src_dir> <dst_dir>
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage

SIZE = 384
FOOT_MARGIN = 6


def clean(img: Image.Image) -> Image.Image:
    a = np.asarray(img.convert("RGBA")).astype(np.int16)
    h, w = a.shape[:2]
    rgb, alpha = a[..., :3], a[..., 3]
    lo, hi = rgb.min(axis=2), rgb.max(axis=2)
    whiteish = (lo > 150) & ((hi - lo) < 60)
    # Only the floor region: the paper hat and eye highlights are higher up.
    floor = np.zeros_like(whiteish)
    floor[int(h * 0.72):, :] = True
    alpha[whiteish & floor] = 0
    # Soft grey fringe of the shadow (semi-transparent, unsaturated).
    alpha[floor & (alpha < 200) & ((hi - lo) < 70)] = 0
    # Keep only big blobs (the figure); stray specks go.
    labels, n = ndimage.label(alpha > 24)
    if n:
        sizes = ndimage.sum(np.ones_like(alpha), labels, index=range(1, n + 1))
        keep = {i + 1 for i, s in enumerate(sizes) if s >= 400}
        alpha[~np.isin(labels, list(keep))] = 0
    out = np.dstack([rgb, alpha]).clip(0, 255).astype(np.uint8)
    im = Image.fromarray(out, "RGBA")
    l, t, r, b = im.getbbox()
    im = im.crop((0, t, w, b))
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    scale = min(1.0, (SIZE - FOOT_MARGIN) / im.height, SIZE / im.width)
    if scale < 1.0:
        im = im.resize((round(im.width * scale), round(im.height * scale)), Image.LANCZOS)
    canvas.paste(im, ((SIZE - im.width) // 2, SIZE - FOOT_MARGIN - im.height), im)
    return canvas


def main() -> None:
    src, dst = Path(sys.argv[1]), Path(sys.argv[2])
    dst.mkdir(parents=True, exist_ok=True)
    for p in sorted(src.glob("*.png")):
        clean(Image.open(p)).save(dst / p.name, optimize=True)
        print("cleaned", p.name)


if __name__ == "__main__":
    main()
