#!/usr/bin/env python3
"""Génère l'icône de l'app Tidy sans dépendance externe (repris de Snappy).

Motif : une boîte (corps gris, barre de titre lime) qui contient quatre icônes
violettes, sur un carré arrondi noir. Même dessin que `widgets::paint_logo`
(crates/tidy-ui/src/widgets.rs), sur une grille de 48.

Sorties (dans assets/) :
- icon.png      256×256, pour le README
- icon.ico      16 à 64 px en DIB, 256 px en PNG (exe Windows, plus tard)
- icon_64.rgba  64×64 RGBA brut, icône de fenêtre chargée par eframe
- icon_mac_256.rgba  256×256 RGBA brut avec la marge de la grille d'Apple
- icon.icns     icône du bundle macOS (via `iconutil`, tailles 16 à 1024)

L'icône de la barre des menus est dessinée par le code (app/src/agent/tray.rs).

Usage : python3 tools/gen_icon.py
"""

import shutil
import struct
import subprocess
import tempfile
import zlib
from pathlib import Path

BG = (0x00, 0x00, 0x00)
BORDER = (0x2A, 0x2A, 0x30)
VIOLET = (0x7F, 0x3A, 0xED)
LIME = (0xC6, 0xFF, 0x34)
SUPERSAMPLE = 4

# Logo sur une grille de 48 : corps (0..48, coins de 10), barre de titre (hauteur 12),
# quatre icônes de 12×10 (coins de 3).
ICONS = [(8, 18), (28, 18), (8, 34), (28, 34)]


def inside_rounded_rect(x, y, x0, y0, x1, y1, r):
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def logo(gx, gy):
    """Couleur du logo au point (gx, gy) de la grille de 48, ou None."""
    if not inside_rounded_rect(gx, gy, 0, 0, 48, 48, 10):
        return None
    if gy <= 12:
        return LIME
    for x, y in ICONS:
        if inside_rounded_rect(gx, gy, x, y, x + 12, y + 10, 3):
            return VIOLET
    return BORDER


def sample(u, v, size):
    """Couleur RGBA au point (u, v) de [0, 1]² pour une icône de `size` pixels."""
    if size <= 24:
        # Petites tailles : le logo seul, sans tuile, pour rester lisible.
        color = logo((u - 0.02) / 0.96 * 48, (v - 0.02) / 0.96 * 48)
        return (*color, 255) if color else (0, 0, 0, 0)
    if not inside_rounded_rect(u, v, 0.02, 0.02, 0.98, 0.98, 0.22):
        return (0, 0, 0, 0)
    # Bordure fine.
    if not inside_rounded_rect(u, v, 0.045, 0.045, 0.955, 0.955, 0.2):
        return (*BORDER, 255)
    margin = 0.2
    color = logo((u - margin) / (1 - 2 * margin) * 48, (v - margin) / (1 - 2 * margin) * 48)
    return (*(color or BG), 255)


# Grille d'Apple : le carré arrondi occupe 824/1024 du canevas.
MAC_MARGIN = (1024 - 824) / 2 / 1024


def sample_mac(u, v, size):
    """Icône macOS : même dessin, réduit dans la grille d'Apple."""
    # La tuile de `sample` commence à 0.02 : on la cale sur la marge d'Apple.
    m = MAC_MARGIN - 0.02 * (1 - 2 * MAC_MARGIN)
    if u < m or u > 1 - m or v < m or v > 1 - m:
        return (0, 0, 0, 0)
    return sample((u - m) / (1 - 2 * m), (v - m) / (1 - 2 * m), size)


def render(size, sampler=None):
    sampler = sampler or sample
    pixels = bytearray()
    ss = SUPERSAMPLE
    for y in range(size):
        for x in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(ss):
                for sx in range(ss):
                    r, g, b, a = sampler(
                        (x + (sx + 0.5) / ss) / size, (y + (sy + 0.5) / ss) / size, size
                    )
                    acc[0] += r * a
                    acc[1] += g * a
                    acc[2] += b * a
                    acc[3] += a
            alpha = acc[3] / (ss * ss)
            if acc[3]:
                pixels += bytes(round(c / acc[3]) for c in acc[:3])
            else:
                pixels += b"\0\0\0"
            pixels.append(round(alpha))
    return bytes(pixels)


def png(size, rgba):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\0" + rgba[y * size * 4 : (y + 1) * size * 4] for y in range(size))
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def bmp_entry(size, rgba):
    """Entrée ICO au format DIB 32 bits (BGRA, de bas en haut + masque AND vide).
    L'Explorateur Windows n'affiche pas toujours les petites tailles en PNG."""
    mask_row = ((size + 31) // 32) * 4
    header = struct.pack(
        "<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, size * size * 4 + mask_row * size, 0, 0, 0, 0
    )
    pixels = bytearray()
    for y in range(size - 1, -1, -1):
        row = rgba[y * size * 4 : (y + 1) * size * 4]
        for x in range(size):
            r, g, b, a = row[x * 4 : x * 4 + 4]
            pixels += bytes((b, g, r, a))
    return header + bytes(pixels) + b"\0" * (mask_row * size)


def ico(images):
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, data = b"", b""
    for size, blob in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


def icns(out_path, tmp):
    """icon.icns via `iconutil` : .iconset temporaire de 16 à 1024 px (+ @2x)."""
    iconset = Path(tmp) / "icon.iconset"
    iconset.mkdir()
    for base in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            px = base * scale
            name = f"icon_{base}x{base}{'@2x' if scale == 2 else ''}.png"
            (iconset / name).write_bytes(png(px, render(px, sample_mac)))
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(out_path)], check=True)


def main():
    out = Path(__file__).resolve().parent.parent / "assets"
    out.mkdir(exist_ok=True)
    rendered = {size: render(size) for size in (16, 24, 32, 48, 64, 256)}
    (out / "icon.png").write_bytes(png(256, rendered[256]))
    entries = [
        (s, png(s, rgba) if s >= 256 else bmp_entry(s, rgba)) for s, rgba in rendered.items()
    ]
    (out / "icon.ico").write_bytes(ico(entries))
    (out / "icon_64.rgba").write_bytes(rendered[64])
    (out / "icon_mac_256.rgba").write_bytes(render(256, sample_mac))
    if shutil.which("iconutil"):
        with tempfile.TemporaryDirectory() as tmp:
            icns(out / "icon.icns", tmp)
    else:
        print("iconutil introuvable (hors macOS) : icon.icns non généré")
    print("icônes écrites dans", out)


if __name__ == "__main__":
    main()
