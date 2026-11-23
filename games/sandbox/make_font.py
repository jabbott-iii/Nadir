#!/usr/bin/env python3
# Copyright 2026 Joseph Anthony Abbott III
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""The sandbox's menu font, and the white image its menus draw boxes with.

    python3 -B games/sandbox/make_font.py [ASSETS]

writes, into ASSETS (the workspace's `assets/` if none is given):

  fonts/menu.fnt    a pixel font in BMFont's text format, Unicode: printable
                    ASCII and the gamepad prompts' symbols past it, ✕ ○ □ △
                    and − (assets/data/input/prompts.jsonc), each glyph 5 × 7
                    above the baseline and 2 below, 6 pixels apart, lines 10
                    apart
  fonts/menu_0.png  its page, 128 × 64: white glyphs in the alpha, clear
                    around them
  fonts/white.png   4 × 4 white pixels, which the menus' boxes are drawn of,
                    tinted

The cook packs the glyphs and the white image into the atlas of the group
`fonts`, `atlas/fonts.kbj`, and writes the font file
`atlas/fonts/menu.font.kbj`, which the sandbox loads.

The ASCII glyphs are the engine's debug font's (crates/engine/src/debug_ui/
font.rs), drawn here again as rows of `#`, ink, and `.`, none; the symbols
are drawn in the same cell. The files are committed, as the sandbox's other
sources are: this script is how they were made, and it is run by hand to
remake them, never by a build. It writes the same bytes each time.
"""

import os
import struct
import sys
import zlib

# A glyph's cell: 5 wide, and 9 high, 7 rows above the baseline and 2 below.
WIDTH = 5
HEIGHT = 9
# The pen's move after each glyph, the lines' spacing, and the baseline from
# a line's top: a row of space above the capitals, which start a row down.
ADVANCE = 6
LINE_HEIGHT = 10
BASE = 8
TOP = 1
# The page, and its cells of WIDTH + 1 by HEIGHT + 1, so that no two glyphs
# touch.
PAGE_W = 128
PAGE_H = 64
COLUMNS = PAGE_W // (WIDTH + 1)

# The glyphs of U+0020 to U+007E, twelve to a band: a band's characters,
# then its nine rows, each the five pixels of every glyph's row, a space
# between two glyphs.
BANDS = [
    (
        ' !"#$%&\'()*+',
        [
            '..... ..#.. .#.#. .#.#. ..#.. ##... .##.. ..#.. ...#. .#... ..... .....',
            '..... ..#.. .#.#. .#.#. .#### ##..# #..#. ..#.. ..#.. ..#.. ..#.. ..#..',
            '..... ..#.. ..... ##### #.#.. ...#. #.#.. ..... .#... ...#. #.#.# ..#..',
            '..... ..#.. ..... .#.#. .###. ..#.. .#... ..... .#... ...#. .###. #####',
            '..... ..#.. ..... ##### ..#.# .#... #.#.# ..... .#... ...#. #.#.# ..#..',
            '..... ..... ..... .#.#. ####. #..## #..#. ..... ..#.. ..#.. ..#.. ..#..',
            '..... ..#.. ..... .#.#. ..#.. ...## .##.# ..... ...#. .#... ..... .....',
            '..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
            '..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
        ],
    ),
    (
        ',-./01234567',
        [
            '..... ..... ..... ....# .###. ..#.. .###. ##### ...#. ##### ..##. #####',
            '..... ..... ..... ....# #...# .##.. #...# ...#. ..##. #.... .#... ....#',
            '..... ..... ..... ...#. #..## ..#.. ....# ..#.. .#.#. ####. #.... ...#.',
            '..... ##### ..... ..#.. #.#.# ..#.. ...#. ...#. #..#. ....# ####. ..#..',
            '..... ..... ..... .#... ##..# ..#.. ..#.. ....# ##### ....# #...# .#...',
            '..... ..... ..... #.... #...# ..#.. .#... #...# ...#. #...# #...# .#...',
            '..#.. ..... ..#.. #.... .###. .###. ##### .###. ...#. .###. .###. .#...',
            '..#.. ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
            '.#... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
        ],
    ),
    (
        '89:;<=>?@ABC',
        [
            '.###. .###. ..... ..... ...#. ..... .#... .###. .###. .###. ####. .###.',
            '#...# #...# ..... ..... ..#.. ..... ..#.. #...# #...# #...# #...# #...#',
            '#...# #...# ..#.. ..#.. .#... ##### ...#. ....# #.### #...# #...# #....',
            '.###. .#### ..... ..... #.... ..... ....# ...#. #.#.# ##### ####. #....',
            '#...# ....# ..... ..... .#... ##### ...#. ..#.. #.### #...# #...# #....',
            '#...# ...#. ..#.. ..#.. ..#.. ..... ..#.. ..... #.... #...# #...# #...#',
            '.###. .##.. ..... ..#.. ...#. ..... .#... ..#.. .###. #...# ####. .###.',
            '..... ..... ..... .#... ..... ..... ..... ..... ..... ..... ..... .....',
            '..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
        ],
    ),
    (
        'DEFGHIJKLMNO',
        [
            '###.. ##### ##### .###. #...# .###. ..### #...# #.... #...# #...# .###.',
            '#..#. #.... #.... #...# #...# ..#.. ...#. #..#. #.... ##.## #...# #...#',
            '#...# #.... #.... #.... #...# ..#.. ...#. #.#.. #.... #.#.# ##..# #...#',
            '#...# ####. ####. #.### ##### ..#.. ...#. ##... #.... #.#.# #.#.# #...#',
            '#...# #.... #.... #...# #...# ..#.. ...#. #.#.. #.... #...# #..## #...#',
            '#..#. #.... #.... #...# #...# ..#.. #..#. #..#. #.... #...# #...# #...#',
            '###.. ##### #.... .#### #...# .###. .##.. #...# ##### #...# #...# .###.',
            '..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
            '..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
        ],
    ),
    (
        'PQRSTUVWXYZ[',
        [
            '####. .###. ####. .#### ##### #...# #...# #...# #...# #...# ##### .###.',
            '#...# #...# #...# #.... ..#.. #...# #...# #...# #...# #...# ....# .#...',
            '#...# #...# #...# #.... ..#.. #...# #...# #...# .#.#. .#.#. ...#. .#...',
            '####. #...# ####. .###. ..#.. #...# #...# #.#.# ..#.. ..#.. ..#.. .#...',
            '#.... #.#.# #.#.. ....# ..#.. #...# #...# #.#.# .#.#. ..#.. .#... .#...',
            '#.... #..#. #..#. ....# ..#.. #...# .#.#. #.#.# #...# ..#.. #.... .#...',
            '#.... .##.# #...# ####. ..#.. .###. ..#.. .#.#. #...# ..#.. ##### .###.',
            '..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
            '..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....',
        ],
    ),
    (
        '\\]^_`abcdefg',
        [
            '#.... .###. ..#.. ..... .#... ..... #.... ..... ....# ..... ..##. .....',
            '#.... ...#. .#.#. ..... ..#.. ..... #.... ..... ....# ..... .#..# .....',
            '.#... ...#. #...# ..... ..... .###. ####. .###. .#### .###. .#... .####',
            '..#.. ...#. ..... ..... ..... ....# #...# #.... #...# #...# ###.. #...#',
            '...#. ...#. ..... ..... ..... .#### #...# #.... #...# ##### .#... #...#',
            '....# ...#. ..... ..... ..... #...# #...# #...# #...# #.... .#... #...#',
            '....# .###. ..... ..... ..... .#### ####. .###. .#### .###. .#... .####',
            '..... ..... ..... ##### ..... ..... ..... ..... ..... ..... ..... ....#',
            '..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .###.',
        ],
    ),
    (
        'hijklmnopqrs',
        [
            '#.... ..#.. ...#. #.... .##.. ..... ..... ..... ..... ..... ..... .....',
            '#.... ..... ..... #.... ..#.. ..... ..... ..... ..... ..... ..... .....',
            '#.##. .##.. ..##. #..#. ..#.. ##.#. #.##. .###. ####. .#### #.##. .####',
            '##..# ..#.. ...#. #.#.. ..#.. #.#.# ##..# #...# #...# #...# ##..# #....',
            '#...# ..#.. ...#. ##... ..#.. #.#.# #...# #...# #...# #...# #.... .###.',
            '#...# ..#.. ...#. #.#.. ..#.. #.#.# #...# #...# #...# #...# #.... ....#',
            '#...# .###. ...#. #..#. .###. #.#.# #...# .###. ####. .#### #.... ####.',
            '..... ..... #..#. ..... ..... ..... ..... ..... #.... ....# ..... .....',
            '..... ..... .##.. ..... ..... ..... ..... ..... #.... ....# ..... .....',
        ],
    ),
    (
        'tuvwxyz{|}~',
        [
            '.#... ..... ..... ..... ..... ..... ..... ...## ..#.. ##... .....',
            '.#... ..... ..... ..... ..... ..... ..... ..#.. ..#.. ..#.. .....',
            '###.. #...# #...# #...# #...# #...# ##### ..#.. ..#.. ..#.. .#...',
            '.#... #...# #...# #...# .#.#. #...# ...#. .#... ..#.. ...#. #.#.#',
            '.#... #...# #...# #.#.# ..#.. #...# ..#.. ..#.. ..#.. ..#.. ...#.',
            '.#..# #..## .#.#. #.#.# .#.#. #...# .#... ..#.. ..#.. ..#.. .....',
            '..##. .##.# ..#.. .#.#. #...# .#### ##### ...## ..#.. ##... .....',
            '..... ..... ..... ..... ..... ....# ..... ..... ..... ..... .....',
            '..... ..... ..... ..... ..... .###. ..... ..... ..... ..... .....',
        ],
    ),
]

# The gamepad prompts' symbols past ASCII: PlayStation's face buttons and
# Nintendo's minus, as the band's rows are.
SYMBOLS = [
    (
        "\u2715\u25cb\u25a1\u25b3\u2212",
        [
            "..... ..... ..... ..#.. .....",
            "#...# .###. ##### ..#.. .....",
            ".#.#. #...# #...# .#.#. .....",
            "..#.. #...# #...# .#.#. #####",
            ".#.#. #...# #...# #...# .....",
            "#...# .###. ##### #...# .....",
            "..... ..... ..... ##### .....",
            "..... ..... ..... ..... .....",
            "..... ..... ..... ..... .....",
        ],
    ),
]


def glyphs():
    """Each character and its rows of WIDTH pixels, 1 for ink, in order."""
    out = []
    for chars, rows in BANDS + SYMBOLS:
        cells = [row.split(" ") for row in rows]
        assert len(cells) == HEIGHT, chars
        for index, char in enumerate(chars):
            pixels = [[1 if c == "#" else 0 for c in cells[y][index]] for y in range(HEIGHT)]
            assert all(len(row) == WIDTH for row in pixels), char
            out.append((char, pixels))
    return out


def png(width, height, rgba):
    """A PNG of straight RGBA at 8 bits, from rows of `width` pixels."""
    raw = b"".join(
        b"\0" + bytes(rgba[y * width * 4 : (y + 1) * width * 4]) for y in range(height)
    )

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def font():
    """The font's text, and its page's pixels."""
    page = bytearray(PAGE_W * PAGE_H * 4)
    chars = glyphs()
    assert len(chars) <= COLUMNS * (PAGE_H // (HEIGHT + 1)), "the page is full"
    lines = [
        'info face="Sandbox menu" size=-10 bold=0 italic=0 charset="" unicode=1 '
        "stretchH=100 smooth=0 aa=1 padding=0,0,0,0 spacing=1,1 outline=0",
        f"common lineHeight={LINE_HEIGHT} base={BASE} scaleW={PAGE_W} scaleH={PAGE_H} "
        "pages=1 packed=0 alphaChnl=0 redChnl=4 greenChnl=4 blueChnl=4",
        'page id=0 file="menu_0.png"',
        f"chars count={len(chars)}",
    ]
    for index, (char, pixels) in enumerate(chars):
        left = (index % COLUMNS) * (WIDTH + 1)
        top = (index // COLUMNS) * (HEIGHT + 1)
        for y, row in enumerate(pixels):
            for x, ink in enumerate(row):
                if ink:
                    at = ((top + y) * PAGE_W + left + x) * 4
                    page[at : at + 4] = b"\xff\xff\xff\xff"
        lines.append(
            f"char id={ord(char)} x={left} y={top} width={WIDTH} height={HEIGHT} "
            f"xoffset=0 yoffset={TOP} xadvance={ADVANCE} page=0 chnl=15"
        )
    return "\n".join(lines) + "\n", page


def main():
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    assets = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, "assets")
    folder = os.path.join(assets, "fonts")
    os.makedirs(folder, exist_ok=True)
    text, page = font()
    with open(os.path.join(folder, "menu.fnt"), "w", encoding="utf-8", newline="\n") as out:
        out.write(text)
    with open(os.path.join(folder, "menu_0.png"), "wb") as out:
        out.write(png(PAGE_W, PAGE_H, page))
    with open(os.path.join(folder, "white.png"), "wb") as out:
        out.write(png(4, 4, b"\xff" * 64))
    print(f"{folder}: menu.fnt, {text.count('char id=')} glyphs; menu_0.png; white.png")


if __name__ == "__main__":
    main()
