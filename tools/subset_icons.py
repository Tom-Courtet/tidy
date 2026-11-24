#!/usr/bin/env python3
"""Réduit la police d'icônes Lucide aux glyphes utilisés par Tidy.

Source : paquet npm officiel `lucide-static` (dossier `font/`), version 1.77.0 :
  https://registry.npmjs.org/lucide-static/-/lucide-static-1.77.0.tgz
Licence : ISC, et MIT pour les icônes issues de Feather (voir
assets/fonts/LICENSE-Lucide.txt, copie du fichier LICENSE du paquet).

Sortie : assets/fonts/Lucide.ttf. Les noms et les points de code doivent
correspondre aux constantes de crates/tidy-ui/src/icons.rs : le script les affiche pour comparer.

Usage (fontTools installé, `pip install fonttools`) :
  tar -xzf lucide-static-1.77.0.tgz
  python3 tools/subset_icons.py package/font

Équivaut à `pyftsubset lucide.ttf --unicodes=<liste> --layout-features= --no-hinting
--notdef-outline --name-IDs='*'`, mais en retirant d'abord la table GSUB de Lucide, que
pyftsubset ne sait pas relire (ligatures).
"""

import json
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

# Constante de crates/tidy-ui/src/icons.rs -> nom de l'icône Lucide.
ICONS = {
    # Boîtes et types d'éléments.
    "BOX": "package",
    "BOXES": "layout-dashboard",
    "FOLDER": "folder",
    "FOLDER_OPEN": "folder-open",
    "FOLDER_PLUS": "folder-plus",
    "PORTAL": "folder-symlink",
    "FILE": "file",
    "DOCUMENT": "file-text",
    "IMAGE": "image",
    "VIDEO": "film",
    "MUSIC": "music",
    "ARCHIVE": "file-archive",
    "APP": "app-window",
    "EXECUTABLE": "square-terminal",
    "LINK": "link",
    "GAME": "gamepad-2",
    "OTHER": "shapes",
    "DRAW_BOX": "square-dashed-mouse-pointer",
    # Boîte privée.
    "LOCK": "lock",
    "UNLOCK": "lock-open",
    "KEY": "key-round",
    "SHIELD": "shield-check",
    # Sections de la fenêtre principale.
    "HOME": "house",
    "WALLPAPER": "wallpaper",
    "WIDGETS": "layout-grid",
    "SEARCH": "search",
    "SETTINGS": "settings",
    "OPTIONS": "sliders-horizontal",
    "LAYOUT": "layers",
    "SNAPSHOT": "history",
    "DIAGNOSTIC": "bug",
    "QUICK_ACTIONS": "mouse-pointer-click",
    "AUTO_TIDY": "wand-sparkles",
    "ROCKET": "rocket",
    # Widgets.
    "CLOCK": "clock",
    "NOTE": "sticky-note",
    "TIMER": "timer",
    "WEATHER": "cloud-sun",
    "CALENDAR": "calendar",
    "AI": "sparkles",
    "TOOLS": "wrench",
    "CAMERA": "camera",
    "BELL": "bell",
    # Actions courantes.
    "PLUS": "plus",
    "CLOSE": "x",
    "OK": "check",
    "DELETE": "trash-2",
    "EDIT": "pencil",
    "COPY": "copy",
    "CUT": "scissors",
    "PASTE": "clipboard-paste",
    "OPEN": "external-link",
    "RESTORE": "rotate-ccw",
    "UNDO": "undo-2",
    "REFRESH": "refresh-cw",
    "PIN": "pin",
    "STAR": "star",
    "EYE": "eye",
    "EYE_OFF": "eye-off",
    "POWER": "power",
    "PLAY": "play",
    "PAUSE": "pause",
    "VOLUME": "volume-2",
    "MUTE": "volume-x",
    # Affichage des boîtes.
    "MENU": "menu",
    "SORT": "arrow-up-down",
    "ROLL_UP": "chevrons-up",
    "ROLL_DOWN": "chevrons-down",
    "TABS": "panels-top-left",
    "PALETTE": "palette",
    "MAGNET": "magnet",
    "GRIP": "grip-vertical",
    "LIST": "list",
    "GRID": "grid-3x3",
    "MONITOR": "monitor",
    # Divers.
    "KEYBOARD": "keyboard",
    "LANGUAGE": "languages",
    "GLOBE": "globe",
    "INFO": "info",
    "WARNING": "triangle-alert",
    "CHEVRON_DOWN": "chevron-down",
    "CHEVRON_UP": "chevron-up",
    "CHEVRON_LEFT": "chevron-left",
    "CHEVRON_RIGHT": "chevron-right",
    "BACK": "arrow-left",
}

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "assets" / "fonts" / "Lucide.ttf"
ICONS_RS = ROOT / "crates" / "tidy-ui" / "src" / "icons.rs"


def write_icons_rs(codepoints: dict) -> None:
    """Régénère src/ui/icons.rs à partir de ICONS (même ordre, mêmes points de code)."""
    lines = [
        "//! Icônes de l'interface : glyphes de la police Lucide (ISC/MIT), réduite aux icônes",
        "//! listées ici (`assets/fonts/Lucide.ttf`, régénérée par `tools/subset_icons.py`,",
        "//! qui réécrit aussi ce fichier : ne pas le modifier à la main).",
        "//!",
        "//! Chaque constante est un texte d'un caractère, à mêler au texte normal :",
        '//! `format!("{} Supprimer", icons::DELETE)`.',
        "",
        "",
    ]
    for const, name in ICONS.items():
        lines += [f"/// `{name}`", f'pub const {const}: &str = "\\u{{{codepoints[name]:04X}}}";']
    lines += ["", "/// (nom, glyphe) de toutes les icônes, pour les tests.", "pub const ALL: &[(&str, &str)] = &["]
    lines += [f'    ("{const}", {const}),' for const in ICONS]
    lines += ["];", ""]
    ICONS_RS.write_text("\n".join(lines))
    print(f"{len(ICONS)} constantes -> {ICONS_RS}")


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit("usage : python3 tools/subset_icons.py <dossier font/ de lucide-static>")
    font_dir = Path(sys.argv[1])
    codepoints = json.loads((font_dir / "codepoints.json").read_text())
    unicodes = []
    for const, name in ICONS.items():
        cp = codepoints[name]
        unicodes.append(cp)
        print(f"{const:<12} {name:<16} U+{cp:04X}")
    font = TTFont(font_dir / "lucide.ttf")
    # Les ligatures de Lucide (GSUB) sont inutiles ici, et fontTools ne sait pas
    # relire cette table : on retire les tables de mise en forme avant de réduire.
    for tag in ("GSUB", "GPOS", "GDEF"):
        if tag in font:
            del font[tag]
    options = subset.Options()
    options.layout_features = []
    options.hinting = False
    options.notdef_outline = True
    options.name_IDs = ["*"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=unicodes)
    subsetter.subset(font)
    # Dans lucide.ttf, l'approche gauche (hmtx) vaut 0 alors que les contours commencent
    # à x = 42 : selon le moteur, l'icône serait décalée vers la gauche. On la recale sur
    # le contour, qui est centré dans sa chasse de 1000.
    glyf = font["glyf"]
    for name in font.getGlyphOrder():
        advance, _ = font["hmtx"][name]
        glyph = glyf[name]
        if glyph.numberOfContours:
            glyph.recalcBounds(glyf)
            font["hmtx"][name] = (advance, glyph.xMin)
    # La police voyage seule dans l'exe : on y inscrit l'avis de licence (ISC/MIT).
    name = font["name"]
    notice = "Copyright (c) Lucide Icons and Contributors (ISC); Feather icons (c) Cole Bemis (MIT)"
    name.setName(notice, 0, 3, 1, 0x409)
    name.setName("ISC License and MIT License, see https://lucide.dev/license", 13, 3, 1, 0x409)
    name.setName("https://lucide.dev/license", 14, 3, 1, 0x409)
    font.save(OUTPUT)
    write_icons_rs(codepoints)
    print(f"{len(unicodes)} glyphes -> {OUTPUT} ({OUTPUT.stat().st_size} octets)")


if __name__ == "__main__":
    main()
