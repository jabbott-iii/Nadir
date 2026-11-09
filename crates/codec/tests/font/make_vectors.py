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

"""Font file (atlas/<font>.font.kbj, version 1) test vectors for nadir-codec.

    python3 -B crates/codec/tests/font/make_vectors.py crates/codec/tests/font fuzz/corpus/font

writes four fonts in BMFont's text format, with their pages, into a
temporary source folder, `fonts/` in it, cooks them with the cook
(`cargo run --release -p nadir-cook`, from the workspace root), and copies
each font file it writes, `atlas/fonts/NAME.font.kbj`, into the first
directory given, and as `NAME.kbj` into the second, the fuzz target's
seeds. The cook's output is the same bytes from the same sources, so a run
over the committed files changes none of them.

The pages are made here, PNGs of straight RGBA with Python's zlib: clear
but for each glyph's rectangle, whose alpha is a ramp, so that the cook
trims nothing from a glyph with pixels; a glyph whose rectangle is left
clear has no frame. The fonts:

  unicode_kerning  AngelCode's BMFont in Unicode: a space and a clear glyph
                   with no frames, negative offsets, characters past ASCII,
                   and kerning; `?` is the fallback
  ansi_fallback    adapted from fuzz/corpus/bmfont/angelcode_ansi_fallback.fnt,
                   its page renamed, with LF line endings: ANSI, a negative
                   size, and BMFont's fallback glyph, of id -1
  two_pages        Hiero, two pages, `unicode=0` with code points
  minimal          an empty face, no glyph but BMFont's fallback, no
                   kerning
"""

import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

FONTS = []


def png(path, w, h, rects):
    """A page of w x h, clear but for each rect (x, y, w, h), whose alpha is a ramp."""
    px = bytearray(w * h * 4)
    for (x, y, rw, rh) in rects:
        for j in range(rh):
            for i in range(rw):
                o = ((y + j) * w + (x + i)) * 4
                px[o:o + 4] = bytes([255, 255, 255, 64 + (i * 37 + j * 23) % 192])
    raw = b''.join(b'\x00' + bytes(px[r * w * 4:(r + 1) * w * 4]) for r in range(h))
    def chunk(t, d):
        return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    data = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) \
        + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')
    open(path, 'wb').write(data)


def font(src, name, text, pages):
    """`pages`: file -> rects with pixels; the rest of each page is clear."""
    open(os.path.join(src, 'fonts', name + '.fnt'), 'w', newline='').write(text)
    m = re.search(r'scaleW=(\d+) scaleH=(\d+)', text)
    w, h = int(m.group(1)), int(m.group(2))
    for file, rects in pages.items():
        png(os.path.join(src, 'fonts', file), w, h, rects)


# Unicode, as AngelCode's BMFont writes it: a space and a glyph with no pixels
# (no frames), negative offsets, characters past ASCII, and kerning.
FONTS.append(('unicode_kerning', '''info face="Seed Sans" size=16 bold=0 italic=0 charset="" unicode=1 stretchH=100 smooth=1 aa=1 padding=0,0,0,0 spacing=1,1 outline=0
common lineHeight=19 base=15 scaleW=64 scaleH=32 pages=1 packed=0 alphaChnl=0 redChnl=4 greenChnl=4 blueChnl=4
page id=0 file="unicode_kerning_0.png"
chars count=7
char id=32   x=0  y=0  width=0  height=0  xoffset=0  yoffset=0  xadvance=4  page=0 chnl=15
char id=63   x=0  y=0  width=6  height=11 xoffset=0  yoffset=4  xadvance=7  page=0 chnl=15
char id=65   x=8  y=0  width=9  height=11 xoffset=-1 yoffset=4  xadvance=8  page=0 chnl=15
char id=86   x=19 y=0  width=9  height=11 xoffset=-1 yoffset=4  xadvance=8  page=0 chnl=15
char id=160  x=30 y=0  width=3  height=3  xoffset=0  yoffset=0  xadvance=4  page=0 chnl=15
char id=233  x=36 y=0  width=7  height=14 xoffset=0  yoffset=1  xadvance=7  page=0 chnl=15
char id=8364 x=45 y=0  width=9  height=11 xoffset=-1 yoffset=4  xadvance=8  page=0 chnl=15
kernings count=3
kerning first=65 second=86 amount=-2
kerning first=86 second=65 amount=-2
kerning first=233 second=8364 amount=1
''', {'unicode_kerning_0.png': [(0, 0, 6, 11), (8, 0, 9, 11), (19, 0, 9, 11), (36, 0, 7, 14), (45, 0, 9, 11)]}))

# ANSI, with BMFont's fallback glyph (id -1) and a negative size: adapted from
# fuzz/corpus/bmfont/angelcode_ansi_fallback.fnt, its page renamed.
FONTS.append(('ansi_fallback', '''info face="Garamond" size=-20 bold=0 italic=0 charset="ANSI" unicode=0 stretchH=100 smooth=1 aa=1 padding=0,0,0,0 spacing=1,1 outline=0
common lineHeight=24 base=19 scaleW=64 scaleH=64 pages=1 packed=0 alphaChnl=0 redChnl=4 greenChnl=4 blueChnl=4
page id=0   file="ansi_fallback_0.png"
chars count=3
char id=-1   x=0     y=0     width=6     height=12    xoffset=1     yoffset=6     xadvance=8     page=0  chnl=15
char id=97   x=8     y=0     width=9     height=10    xoffset=0     yoffset=9     xadvance=9     page=0  chnl=15
char id=145  x=20    y=0     width=4     height=6     xoffset=1     yoffset=4     xadvance=5     page=0  chnl=15
kernings count=1
kerning first=145 second=97  amount=-1  
''',
     {'ansi_fallback_0.png': [(0, 0, 6, 12), (8, 0, 9, 10), (20, 0, 4, 6)]}))

# Two pages, as Hiero writes them, with no `common` channels.
FONTS.append(('two_pages', '''info face="Seed Mono" size=24 bold=0 italic=0 charset="" unicode=0 stretchH=100 smooth=1 aa=1 padding=1,1,1,1 spacing=-2,-2
common lineHeight=29 base=22 scaleW=32 scaleH=32 pages=2 packed=0
page id=0 file="two_pages_0.png"
page id=1 file="two_pages_1.png"
chars count=2
char id=63   x=0  y=0  width=10 height=20 xoffset=0  yoffset=3  xadvance=13 page=0 chnl=0
char id=8364 x=4  y=8  width=28 height=24 xoffset=-1 yoffset=1  xadvance=14 page=1 chnl=0
kernings count=0
''', {'two_pages_0.png': [(0, 0, 10, 20)], 'two_pages_1.png': [(4, 8, 28, 24)]}))

# The least a font file can hold: an empty face, no glyph but BMFont's
# fallback, no kerning.
FONTS.append(('minimal', '''info face="" size=8
common lineHeight=8 base=7 scaleW=8 scaleH=8 pages=1
page id=0 file="minimal_0.png"
char id=-1 x=0 y=0 width=4 height=6 xoffset=0 yoffset=1 xadvance=5 page=0
''', {'minimal_0.png': [(0, 0, 4, 6)]}))


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    vectors, seeds = sys.argv[1], sys.argv[2]
    # The workspace root: crates/codec/tests/font/ is four folders below it.
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..', '..'))
    with tempfile.TemporaryDirectory() as work:
        src = os.path.join(work, 'src')
        out = os.path.join(work, 'out')
        os.makedirs(os.path.join(src, 'fonts'))
        for name, text, pages in FONTS:
            font(src, name, text, pages)
        subprocess.run(['cargo', 'run', '--quiet', '--release', '-p', 'nadir-cook', '--', src, out],
                       cwd=root, check=True)
        os.makedirs(vectors, exist_ok=True)
        os.makedirs(seeds, exist_ok=True)
        for name, _, _ in FONTS:
            cooked = os.path.join(out, 'atlas', 'fonts', name + '.font.kbj')
            shutil.copyfile(cooked, os.path.join(vectors, name + '.font.kbj'))
            shutil.copyfile(cooked, os.path.join(seeds, name + '.kbj'))


if __name__ == '__main__':
    main()
