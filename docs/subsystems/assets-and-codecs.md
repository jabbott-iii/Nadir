# Subsystem: `nadir-codec`, `nadir-assets`, `nadir-cook`

## Design in one paragraph

Authoring formats (PNG, Aseprite JSON, LDtk JSON, WAV, BMFont) are parsed
**only by the cook tool**. The cook tool turns them into a few simple
runtime formats that we specify and own:

- QOI for textures
- QOA or PCM for audio
- KBJ (binary JSON) for data
- our own level and font formats
- a `.kpak` archive with optional KLZ compression

The shipped game therefore contains only small, fuzzable parsers, while
artists keep their normal tools (ADR-0008, ADR-0012).

## `nadir-codec`

Every decoder:

- takes `&[u8]`, or `&str` for a text format, plus a `Limits` struct
  (`qoa::header` and `qoa::decode_frame`, which allocate nothing, take
  none; `qoa::decode` does)
- returns `Result<_, CodecError>` and **never panics** on any input
- checks every length and offset against the input size before use
- checks every allocation size against `Limits` before allocating

| Module | Spec | Encode | Decode | Shipped in game? | Notes |
| :- | :- | :- | :- | :- | :- |
| `adler32` | RFC 1950 | ✓ | ✓ | no | zlib trailer: `adler32::checksum` |
| `inflate` | RFC 1950/1951 | — | ✓ | no (cook only) | Stored, fixed and dynamic Huffman blocks; output cap from `Limits`. See below |
| `png` (feature `png`) | PNG 1.2 / ISO 15948 | — | ✓ | no | See subset below |
| `qoi` | qoiformat.org spec | ✓ | ✓ | yes | Textures, golden references |
| `qoa` | qoaformat.org spec | ✓ | ✓ | yes | Music and long SFX; LMS predictor, 20-sample slices; a frame at a time, for music as it plays. See subset below |
| `wav` | RIFF/WAVE | — | ✓ | no | PCM 16/24-bit int, 32-bit float; mono/stereo. See subset below |
| `kpcm` | The sound file, version 1, below | ✓ | ✓ | yes | 16-bit samples of 1 or 2 channels as they are, for short sounds (M6.8, ADR-0049). See below |
| `pcm` | Below | — | — | yes (a sound's conversion as it loads, M6.9) | `resample`, a windowed sinc the same bits on every system, and `to_i16`. See below |
| `klz` | Below | ✓ | ✓ | yes | Pack entry compression. See below |
| `json` | RFC 8259 + JSONC (`//`, `/* */`, trailing commas) | ✓ | ✓ | yes (settings, dev data) | Order-preserving objects: `Vec<(String, Value)>` |
| `kbj` | Below | ✓ | ✓ | yes | Binary form of the JSON value tree for cooked data. See below |
| `bmfont` | AngelCode BMFont text format | — | ✓ | no | `info`, `common`, `page`, `char`, `kerning` lines. See subset below |
| `aseprite` | Aseprite's `--format json-array` sprite sheet data | — | ✓ | no (cook only) | Frames, sheet, tags expanded to clips (ADR-0035). See subset below |
| `atlas` | The cook's atlas file, version 2, below | — | ✓ | yes | KBJ, then checked strictly; read by render's `Atlas` asset (ADR-0033, ADR-0035). See below |
| `font` | The cook's font file, version 1, below | — | ✓ | yes | KBJ, then checked strictly; the cook reads back what it writes (M7.7, ADR-0053), and render's `Font` asset reads it (M7.1, ADR-0054). See below |
| `ldtk` | LDtk 1.5.3's project `.ldtk` and level `.ldtkl` JSON | — | ✓ | no (cook only) | The members the cook uses, checked against the project's definitions (ADR-0038). See subset below |
| `level` | The cook's level file, version 1, below | — | ✓ | yes | KBJ, then checked strictly (ADR-0038). See below |
| `prefab` | The prefab file, below | — | ✓ | yes | KBJ, or its source's JSON, then checked strictly (ADR-0038). See below |
| `kpak` | The pack file, version 1, below | — | ✓ | yes | Header, TOC and string table checked strictly; each entry's CRC before its KLZ (ADR-0041). See below |

### Inflate (documented, tested)

`inflate::zlib(&[u8], Limits)` decompresses a zlib stream (RFC 1950), and
`inflate::raw(&[u8], Limits)` a bare DEFLATE stream (RFC 1951), each into a
new `Vec`. The PNG decoder uses the same code.

- **Accepted:** the streams zlib 1.3 decodes to their end given its largest
  window, and nothing else. As zlib does, it rejects a set of code lengths
  that is over-subscribed, or incomplete unless it is one code of one bit
  (a code-length code may never be incomplete; a distance code may have no
  codes at all, for a block of literals only); a literal/length code
  without the end of block; more than 286 literal/length or 30 distance
  codes; symbols 286 and 287 and distances 30 and 31; a distance before the
  start of the output; a preset dictionary. It decodes length code 284 with
  extra bits 31 as 258, as zlib does. The zlib header's method, window size
  (at most 32 KiB) and check bits, and the Adler-32 at the end, are
  checked.
- **Rejected, although zlib accepts it:** bytes after the end of the stream
  (zlib leaves them unread), and output past `max_decoded_bytes`.
- **Not checked, as by zlib given its largest window:** that every distance
  fits the window the zlib header declares. libpng asks zlib for the
  header's window instead, and rejects a PNG whose image data reaches
  further back.
- **Limits:** the output, at most `max_decoded_bytes`. It grows by doubling,
  clamped to the limit, so no allocation passes it.
- **Errors:** `CodecError::Inflate { offset, kind }`, with the offset of the
  byte that holds the first bit of the item at fault: a block header, a
  code, its extra bits, the zlib header or the Adler-32. A problem with a
  dynamic block's code lengths (too many codes, over-subscribed,
  incomplete, no end of block) is placed at the block's header.
  `UnexpectedEnd` is placed at the end of the input, and says what should
  have come next.
- **Vectors:** hand-assembled streams in the unit tests of
  `crates/codec/src/inflate.rs`, among them every length and distance code
  at its edges, the farthest match (258 bytes from 32,768 back), and the
  code shapes zlib accepts but never writes; property tests in
  `crates/codec/tests/inflate.rs`, whose encoder writes stored, fixed and
  dynamic blocks with codes of up to 15 bits.
- **Fuzz target:** `inflate`, with seeds in `fuzz/corpus/inflate/`: streams
  zlib made, streams the property tests' encoder made with shapes zlib
  never writes, and one hand-assembled match from 32,768 back. It decodes each input as a zlib stream, and after its two
  header bytes as a bare one, with a limit of 1 MiB so that `TooMuchOutput`
  is reachable. It also checks the answers: a zlib stream that decodes holds
  a bare stream that decodes to the same bytes, and decodes with a limit of
  exactly its length but not one less.

### PNG subset (documented, tested)

`png::decode(&[u8], Limits)` reads a PNG file into an `Image`: its width,
height and RGBA8 pixels, rows from the top, alpha straight, and the names
of the chunks it ignored that the cook tool should warn about.

- **Supported** (ADR-0027):
  - every color type and bit depth: grayscale at 1, 2, 4, 8 and 16 bits;
    RGB, grayscale with alpha and RGBA at 8 and 16; palettes at 1, 2, 4
    and 8
  - bit depth 16, down-converted to the nearest 8-bit value,
    `round(v / 257)`; libpng's `png_set_strip_16` truncates instead
  - grayscale below 8 bits, scaled to 0–255
  - `tRNS`, compared at the image's own bit depth (16-bit samples before
    they are rounded)
  - Adam7 interlacing; a pass with no columns or no rows has no rows at
    all, not even filter bytes
  - all five filter types
  - every chunk's CRC, and the image data's Adler-32, verified
- **Ignored, and named in `Image::ignored`:** the chunks that say how the
  colors are encoded, `gAMA`, `cHRM`, `sRGB`, `iCCP`, `cICP`, `mDCV` and
  `cLLI`, and the text chunks `tEXt`, `zTXt` and `iTXt`; each once, in the
  order first met. We assume sRGB. The decoder logs nothing: the cook tool
  warns once for a file that names a color chunk other than `sRGB`
  (ADR-0031). Other ancillary chunks are ignored without a word.
- **Rejected with a clear error:** unknown critical chunks (Apple's `CgBI`
  among them: it comes before `IHDR`, so the error is that the first chunk
  is not `IHDR`), APNG animation chunks (`acTL`, `fcTL`, `fdAT`).
- **Rejected, although libpng 1.6 loads it**, with a warning or none
  (each measured through GdkPixbuf; a file this damaged is better exported
  again than cooked):
  - a CRC that does not match in an ancillary chunk
  - bytes after `IEND`, or `IEND` with data
  - bytes after the image data's zlib stream, or image data that inflates
    to more than the image needs
  - an `IDAT` after another chunk once the image data has ended, empty or
    not
  - a palette index past the end of the palette (libpng draws it black)
  - `PLTE` in a grayscale image, after `IDAT`, or after `tRNS`; a suggested
    `PLTE` in an RGB or RGBA image of a length no palette has
  - `tRNS` in an image with alpha, after `IDAT`, a second time, before the
    palette, of a length a gray or RGB value does not have, or with more
    alphas than the palette has entries that the bit depth can index
- **Accepted, as libpng accepts it** (silently for the first, with a
  warning for the other two):
  - a palette longer than the bit depth can index, whose extra entries no
    pixel can use (Go's image/png rejects it)
  - a palette `tRNS` with no alphas, as if there were none
  - a gray or RGB `tRNS` value past the bit depth, read in the bits the
    depth has: in an 8-bit image, 0x0100 makes 0x00 transparent. Go's image/png reads the low byte at 8 bits, but does not
    mask the value at gray 1, 2 and 4 bits.
- **Accepted, although libpng rejects it:** image data that reaches further
  back than the window its zlib header declares (see Inflate above).
- **Limits:** width and height each at most `max_texture_dimension` (and
  PNG's own 2³¹ − 1 whatever the limits say), and `width × height × 4` at
  most `max_decoded_bytes`. Decoding holds the inflated image data beside
  the pixels, and it may pass the limit: at most 2.25 times the pixels'
  bytes (16-bit RGBA one pixel wide, 9 bytes a row for 4). With the image
  data split over several `IDAT` chunks, it also holds a copy of the
  compressed data. Before it allocates, the decoder checks the claimed size against the
  input: DEFLATE gives at most 1,032 bytes a byte, so image data shorter
  than `needed / 1032` is `TooLittleImageData`. The cook tool decodes
  source sheets wider than a texture with a raised `max_texture_dimension`
  (M5.4).
- **Errors:** `CodecError::Png { offset, kind }`, with the byte offset of
  the problem in the file. A problem inside the image data is placed at its
  byte, in whichever `IDAT` chunk holds it; one found after inflating (a
  filter type, a palette index) at the first `IDAT` chunk.
- **Vectors:** `crates/codec/tests/png/`: 109 small images, each with the
  pixels it decodes to (`.rgba`), made by `make_vectors.py` there, which
  shares no code with the decoder. They cover every color type and bit
  depth with and without interlacing, each filter on first rows, sizes that
  leave Adam7 passes empty, transparency and its edge cases, padding bits
  set, split and stored image data, and ancillary chunks. libpng decodes
  them to the same pixels, but truncates 16-bit samples. Go's image/png
  gives the same pixels too, but for the two cases above: it does not mask
  the `tRNS` of `g2n_trns_masked`, and rejects the long palette of
  `p2n_pal256`. Error cases are in the
  unit tests of `crates/codec/src/png.rs`; property tests in
  `crates/codec/tests/png.rs`.
- **Fuzz target:** `png`, with seeds in `fuzz/corpus/png/` (a selection of
  the vectors). It decodes to 16 MiB of pixels at most (2048 × 2048 RGBA),
  so that a valid input of the fuzzer's 64 KiB that inflates to a large
  image takes a third of a second, not the seconds it would with the
  game's limit (ADR-0028). It decodes each input, then a repaired copy, so
  that mutations get past the checksums: the signature restored; the first
  chunk taken as `IHDR` of 13 bytes; the chunks after an insertion or a
  deletion found again by their types; a length past the end cut at the
  next chunk; the walk ended at the first `IEND`, which is emptied, or
  added; every CRC and the Adler-32 rewritten (when the image data inflates
  to 1 MiB or less); the `IDAT` chunks kept as they were. When that copy's
  image data is the wrong size for its header, it also decodes one with the
  image data re-packed to the size the header needs; when bytes follow the
  zlib stream, one with them cut. Of 20,000 inputs mutated from the seeds,
  2 reach the image data as they are; repaired, 9,370 do, and 4,141 reach
  the filters.

### KLZ block format (our own, LZ77, byte-oriented; documented, tested)

`klz::decompress(&[u8], raw_size, Limits)` decodes a stream into exactly
`raw_size` bytes, which the caller knows (a pack's table of contents holds
it). `klz::compress(&[u8])` writes one.

```
stream := block*            ; until input exhausted
block  := token:u8
          [extra literal length: 255-run bytes + final byte]  if token>>4 == 15
          literals[lit_len]
          offset:u16 LE                                       (absent on last block)
          [extra match length: 255-run bytes + final byte]    if token&15 == 15
lit_len   = (token >> 4) + extras
match_len = (token & 15) + 4 + extras
```

- **The last block** holds literals only, maybe none: the input ends right
  after its literals, and its token's match half is 0. Every stream but the
  empty one ends with one. The empty stream, no bytes at all, decodes to no
  bytes.
- **A match** copies `match_len` bytes from `offset` back, and may overlap
  what it writes (offset 1 repeats the last byte).
- **Rejected:** offset 0; an offset past the bytes produced so far; a
  stream that ends after a match, or inside a block; a last block whose
  token has a match half; output longer or shorter than `raw_size`; extra
  length bytes that add up past what a `usize` counts.
- **Limits:** `raw_size` at most `max_decoded_bytes`. Before it allocates,
  the decoder also checks `raw_size` against the stream: a block with `k`
  extra match length bytes gives at most `255k + 18` bytes from `k + 3`,
  so no stream gives 255 bytes a byte, and `raw_size > 255 × len` is cut
  short (`UnexpectedEnd` at the end of the stream).
- **Errors:** `CodecError::Klz { offset, kind }`, with the byte offset of
  the item at fault: the token for a last block with a match half, the
  match's offset for a bad offset or a match past `raw_size`, the literals
  for literals past it, and a length's first extra byte for one too large
  to count. `TooShort` and `UnexpectedEnd` are placed at the end of the
  stream (`UnexpectedEnd` says what should have come next), and
  `TooManyBytes` at 0.
- **The encoder** is greedy. A table of 2^16 positions is indexed by the
  top 16 bits of the next 4 bytes, read as a little-endian `u32`, times
  2,654,435,761 (mod 2^32). At each position it looks up the last position
  with the same index and stores its own; it takes the candidate when it is
  at most 65,535 bytes back and its 4 bytes are the same, extends the match
  as far as the bytes agree, and goes on after it, storing no positions
  inside it. It ends with a block of the literals left. Incompressible data
  grows by at most `n / 255 + 2` bytes.
- **Encoder output is not part of the format.** Any stream the grammar
  allows is valid, whoever wrote it. Changing the encoder changes what the
  cook writes, and the vectors' encoder streams, but not the format, so it
  needs no format version.
- **Vectors:** `crates/codec/tests/klz/`, made by `make_vectors.py` there,
  which has its own decoder and its own port of the encoder: 30 streams
  with the bytes they give. Hand-assembled ones cover literal and match
  lengths at the edges of their 4-bit parts and extra bytes, runs at offset
  1, the farthest offset, an empty last block, and the most 6 bytes can
  give (274). The encoder's streams, which `klz::compress` must write byte
  for byte, cover runs, text, noise, repeats too far apart to match, and a
  repeat exactly 65,535 back, which it must use, beside one 65,536 back,
  which it must not. Error cases are in the unit tests of
  `crates/codec/src/klz.rs`; property tests in `crates/codec/tests/klz.rs`
  (round trips, a 1 MiB run, the growth bound, every stream cut short,
  wrong sizes, damage).
- **Fuzz target:** `klz`, with seeds in `fuzz/corpus/klz/`, each a 4-byte
  size and a stream (`.klzs`). The size, little-endian and modulo
  1 MiB + 1, is the size the stream must give, with a limit of 1 MiB. A
  stream that ends short of its size must then decode at the size it
  reached, and the input itself, compressed, must decompress to itself.

### JSON and JSONC subset (documented, tested)

`json::parse(&str, Limits)` reads RFC 8259 JSON plus the JSONC extensions
into a `Value` tree. It takes `&str` rather than bytes because JSON is text.
`to_string_compact` and `to_string_pretty` write strict RFC 8259 JSON.

```rust
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),   // members in document order
}
```

- **Accepted beyond RFC 8259:**
  - `//` and `/* */` comments wherever whitespace is allowed; block comments
    do not nest
  - one trailing comma after the last entry of an array or object
  - a leading byte-order mark
- **Numbers:**
  - Written as an integer and fits `i64`: `Int`.
  - Anything else: `Float`. That includes a fraction or exponent, an integer
    too large for `i64`, and `-0`.
  - Too large for `f64`: an error.
  - The writers always give a float a `.` or an exponent, so `Int` and
    `Float` survive a round trip. KBJ relies on that distinction.
  - NaN and the infinities cannot be written as JSON; the writers emit
    `null` for them.
- **Rejected, although some parsers allow it:**
  - the same key twice in one object
  - a `\uXXXX` escape that is half of a surrogate pair
  - raw control characters in strings, including tabs and newlines
  - leading zeros, a leading `+`, hex numbers, `NaN`, `Infinity`
  - single quotes and unquoted keys
- **Limits** (see the table below): nesting depth, string length in decoded
  bytes (keys included), and entries per array or object. A document of
  only scalars has depth 0. Exceeding a limit is an error.
- **Errors:** `CodecError::Json { line, column, kind }`. Lines and columns
  start at 1, and columns count characters, not bytes.
- **Not preserved:** comments and layout are lost when a document is parsed
  and written back. Member order is kept.
- **Vectors:** `crates/codec/tests/json/`.
- **Fuzz target:** `json`, with seeds in `fuzz/corpus/json/`.

### QOI subset (documented, tested)

`qoi::decode(&[u8], Limits)` reads a QOI file (specification 1.0) into an
`Image`: its `Header` and its pixels. `qoi::encode(&Header, &[u8])` writes
one, byte for byte as the reference encoder (`qoi.h`) does.

- **Pixels:** always RGBA8, 4 bytes each, rows from the top. The header's
  channel count (3 or 4) and color space (0 sRGB, 1 linear) describe the
  image; they do not change how chunks are read, so an RGB file decodes
  to RGBA with whatever alpha its chunks give (255 unless they change it).
  The encoder refuses an RGB image with an alpha other than 255.
- **Accepted:** every chunk of the specification. The decoder stores each
  pixel in its table after every chunk, runs and lookups included, as the
  reference decoder does.
- **Rejected, although the reference decoder accepts it:**
  - a run that goes past the last pixel
  - chunks that run out before the last pixel, where the reference repeats
    the last pixel decoded: here the end marker is read as chunks, and the
    file is cut short or its end marker is wrong
  - an end marker that is missing or wrong, and any byte after it
- **Rejected, as by the reference:** a magic other than `qoif`, a width or
  height of 0, a channel count other than 3 or 4, a color space other than
  0 or 1.
- **Limits** (see the table below): width and height each at most
  `max_texture_dimension`, and `width × height × 4` at most
  `max_decoded_bytes`. Before it allocates, the decoder also checks the
  claimed size against the input: a chunk byte gives at most 62 pixels, so
  `ceil(pixels / 62) > len − 14` is cut short (`UnexpectedEnd` at the end of
  the input).
- **Errors:** `CodecError::Qoi { offset, kind }`, with the byte offset of
  the problem in the file. `UnexpectedEnd` says what should have come next.
  The encoder's errors are `CodecError::QoiEncode { kind }`.
- **Vectors:** hand-computed in the unit tests of `crates/codec/src/qoi.rs`,
  one or more for each chunk and each edge of its range; property tests in
  `crates/codec/tests/qoi.rs`.
- **Fuzz target:** `qoi`, with seeds in `fuzz/corpus/qoi/`, made by a
  separate port of the reference encoder. A test in `xtask` checks that
  `qoi::encode` writes each one back byte for byte.

### KBJ (binary JSON value tree; documented, tested)

`kbj::encode(&Value, Limits)` writes a JSON `Value` (see above) as a KBJ
file, and `kbj::decode(&[u8], Limits)` reads one back.

```
value := tag:u8 payload
  0x00 null | 0x01 false | 0x02 true
  0x03 i64 (zigzag LEB128) | 0x04 f64 (8 bytes LE)
  0x05 string: len:LEB128 bytes(UTF-8)
  0x06 array:  count:LEB128 value*
  0x07 object: count:LEB128 (key:string-payload value)*
file  := "KBJ1" value
```

- **Numbers:** LEB128 is unsigned, 7 bits a byte, low bits first, at most
  10 bytes for 64 bits; an `i64` is zigzag-encoded first (0, −1, 1, −2, …
  as 0, 1, 2, 3, …). A float is its IEEE 754 bits, so −0.0, subnormals and
  the infinities are kept exactly, unlike in JSON text, where they become
  `null` (ADR-0028).
- **One encoding per value** (ADR-0028): `decode` accepts only the bytes
  `encode` writes. It rejects a LEB128 number with more bytes than it needs
  (a last byte of 0 after the first, `OverlongNumber`, however long) or
  past 64 bits (a 10th byte above 1, or bits in an 11th or later,
  `NumberTooLarge`), any NaN but the quiet NaN
  `0x7ff8_0000_0000_0000`, which `encode` writes for every NaN, the same
  key twice in one object (as the JSON parser does), strings that are not
  UTF-8, unknown tags, and bytes after the value. A magic other than `KBJ`
  is `NotKbj`; a version other than `1` is `UnsupportedVersion`.
- **Limits**, as for JSON: nesting depth (a file of one scalar has depth 0;
  64 levels pass, 65 do not), string length in bytes (keys included), and
  entries per array or object. Before it reserves room for a container,
  the decoder checks the count against the bytes left (an item takes at
  least 1, a member 2), and reserves at most 1,024 entries ahead: up to 64
  containers are open at once, each claiming the same bytes, so reserving
  every claim in full could reserve 64 times the input's worth of values.
- **The encoder refuses** what the decoder would, with the same limits:
  the same key twice, nesting, entries or a string past a limit. So every
  file written can be read back, a value survives the round trip (NaN
  payloads aside), and a file decoded and encoded again gives the same
  bytes.
- **Duplicate keys** are found as the JSON parser finds them: as each key
  is read in objects of up to 16 members, by sorting in larger ones, so a
  large object cannot make the check quadratic.
- **Errors:** `CodecError::Kbj { offset, kind }`, with the byte offset of
  the tag, number or string at fault; `UnexpectedEnd` at the end of the
  input, saying what should have come next. The encoder's errors are
  `CodecError::KbjEncode { kind }`.
- **Vectors:** byte strings worked out by hand in the unit tests of
  `crates/codec/src/kbj.rs`, for every tag, LEB128 from one byte to two
  and at ten, zigzag at both ends of `i64`, and each error; property tests
  in `crates/codec/tests/kbj.rs`: random values, compared bit for bit, and
  the JSON test documents survive a round trip; files cut short and damaged
  files.
- **Fuzz target:** `kbj`, with seeds in `fuzz/corpus/kbj/`: the JSON seed
  corpus encoded, and `large_objects.kbj`, whose objects of more than 16
  members let mutations reach the sorted duplicate-key check. Every file
  that decodes must encode back to its own bytes.

### WAV subset (documented, tested)

`wav::decode(&[u8], Limits)` reads a WAV file into a `Wave`: its sample
rate, its channels, how it stored its samples, and the samples as `f32`,
interleaved. The cook turns those below `audio/` into the sound file or
QOA (M6.8; below, "Sounds").

- **Supported:** `WAVE_FORMAT_PCM` at 16 and 24 bits,
  `WAVE_FORMAT_IEEE_FLOAT` at 32 bits, and `WAVE_FORMAT_EXTENSIBLE` with
  either as its subformat, whose valid bits may be anywhere from 1 to all
  the bits its samples hold; mono and stereo. Integer samples are scaled
  to [−1, 1) as libsndfile scales them, by 1/32768 and 1/8388608; float
  samples are kept as they are, past ±1 too.
- **Ignored:** every chunk other than `fmt ` and `data`, the byte rate, the
  extensible format's channel mask, and everything after `data`: so a
  missing pad byte after odd data, as GStreamer and Python's `wave` module
  write it, does not matter. A RIFF size of odd length may be followed by
  its pad byte or not.
- **Rejected:** other sample formats (8-bit, 32-bit integer, 64-bit float,
  compressed ones), with an error that says to export again as one of the
  supported ones: jsfxr, among others, writes 8-bit WAVs unless it is told
  to write 16; more than two channels, or none; a sample rate of 0; RF64
  and BW64; a NaN or infinite float sample; a RIFF size that is not the
  file's; a chunk past the end of the form; a missing `fmt ` or `data`, a
  second `fmt `, or `data` before `fmt `; a `fmt ` too short for its
  format; an extensible subformat that is not PCM or float, or valid bits
  of 0 or more than the samples hold; data that ends inside a frame.
- **Rejected, although libsndfile reads it** (ADR-0028): 8-bit and 32-bit
  integer PCM, PCM of 12 or 20 bits in larger samples, 64-bit float, and
  compressed formats (A-law, µ-law, ADPCM); more than two channels; RF64
  and BW64; a block
  alignment other than the channels times the bytes a sample, which
  libsndfile replaces with its own; a RIFF size that does not match the
  file, as a file written to a pipe or cut short has (the error says so),
  and a chunk past the end; NaN and infinite samples; valid bits of 0, or
  more than the samples hold; data that ends inside a frame. Each is better
  exported again than cooked.
- **Limits:** the samples, at 4 bytes each as `f32`, at most
  `max_decoded_bytes`: with the default, about 11.6 minutes of stereo at
  48 kHz. The cook reads WAVs within 512 MiB, about 23 minutes (M6.8).
- **Errors:** `CodecError::Wav { offset, kind }`, with the byte offset of
  the chunk or field at fault (for an extensible format, the subformat's
  tag or its valid bits), or of the sample for a NaN.
- **Vectors:** `crates/codec/tests/wav/`: 21 files with the samples each
  decodes to (`.f32`), made by `make_vectors.py` there, which writes RIFF
  itself and computes the samples from the ones it wrote. They cover each
  format in mono and stereo, plain and extensible, odd data with and
  without its pad byte, chunks to skip before and after the data, no
  frames, one frame, 8 kHz and 96 kHz, a float `fmt ` of 16 bytes as
  arecord writes it, and 20 valid bits of 24. libsndfile decodes every one
  to the same samples. Of 294 WAV files on the development machine, 288
  decode to the samples libsndfile gives, bit for bit; 5 are 8-bit or
  32-bit integer, and 1 has a wrong block alignment. Error cases are in the
  unit tests of `crates/codec/src/wav.rs`.
- **Fuzz target:** `wav`, with seeds in `fuzz/corpus/wav/` (a selection of
  the vectors).

### QOA subset (documented, tested)

`qoa::encode(&Header, &[i16])` writes a QOA file (qoaformat.org; the
reference is `qoa.h`) of 16-bit samples, interleaved, at 3.2 bits a
sample. `qoa::decode(&[u8], Limits)` reads one back into a `Sound`: its
`Header` (channels, sample rate, samples a channel) and its samples.
`qoa::header(&[u8])` reads and checks the file's header and its first
frame's, and that the bytes are the whole file, and
`qoa::decode_frame(&[u8], &Header, offset, &mut [i16])` decodes one frame, 5120
samples a channel (`FRAME_LEN`) but the last, into the caller's buffer,
allocating nothing: each frame carries its predictors' state, so frames
decode alone and in any order, and the mixer can stream music from the
file in memory (M6.2). `decode` is a `header` and a `decode_frame` for each
frame (ADR-0045).

- **Accepted:** 1 to 8 channels, as the reference's `QOA_MAX_CHANNELS`;
  sample rates of 1 to 16,777,215 Hz; 1 to 2^32 − 1 samples a channel.
- **Rejected, as by the reference:** a magic other than `qoaf`; a header
  of 0 samples, which marks a stream whose length its encoder did not know;
  0 channels or more than 8; a sample rate of 0.
- **Rejected, where the reference gives the samples of the frames before
  the fault, or reads more than the format allows** (ADR-0045): a frame
  whose channels or sample rate are not the first frame's; a frame whose
  samples, size or place is not the one the file's sample count gives it
  (every frame but the last holds 5120 samples a channel, and the last the
  rest, each in exactly the bytes its samples take), where the reference
  also reads a frame larger than its slices and stops at the first short
  frame; a file shorter or longer than its frames; a last slice whose
  unused residuals are not 0, which the reference does not look at and the
  specification says must be.
- **The encoder** searches each slice's scale factor as the reference
  does, from the last slice's on, with its penalty for weights grown large,
  in the same 32-bit arithmetic, which wraps where C's `int` does. Where
  the weights it carries into a frame no longer fit in the 16 bits the
  frame holds, the reference goes on from its own values; this encoder goes
  on from the 16 bits, as the decoder will (ADR-0045). Loud, regular
  signals reach the case: a pattern of period 4 at full scale carries
  weights of 57,865 into its second frame (the vector `wide_weights`); the
  other vectors carry at most 16,388 into a frame (`vectors.txt`). The
  encoder refuses 0 samples, channels or a rate outside the above, and
  samples that are not `samples × channels`.
- **Limits:** the samples, at 2 bytes each, at most `max_decoded_bytes`.
  The header's sample count gives the file's length exactly, and the
  decoder checks it before it allocates, so a file claims at most about 5
  bytes of samples for each of its own.
- **Errors:** `CodecError::Qoa { offset, kind }`, with the byte offset of
  the field or slice at fault; the encoder's are
  `CodecError::QoaEncode { kind }`.
- **Vectors:** `crates/codec/tests/qoa/`: 13 sounds, made by
  `make_vectors.py` there, a port of the reference encoder in Python that
  shares no code with the crate: the samples encoded (`.src`), the file
  (`.qoa`), and the samples its encoder reconstructed as it chose each
  residual (`.i16`), with the largest weight carried into a frame in
  `vectors.txt`. They cover one sample, one slice, short slices in stereo,
  8 channels, the highest sample rate, full-scale extremes, two frames in
  mono and stereo, clipped sines that bring in the search's penalty, a
  second frame whose search starts again, two channels whose searches go
  their own ways, and weights past 16 bits at a frame's start. Each decodes
  to its reconstruction sample for sample, and `qoa::encode` writes each
  byte for byte. Error cases,
  the tables against their formulas, and the predictor are in the unit
  tests of `crates/codec/src/qoa.rs`.
- **Signal-to-noise ratio** (`crates/codec/tests/qoa.rs`), measured on a
  second of each signal and asserted to the dB: a 440 Hz tone at −6 dBFS
  71 dB, a chord in stereo 58 dB, a sweep from 20 Hz to 20 kHz 64 dB, a
  tone at −50 dBFS 39 dB, white noise 16 dB, a square wave 19 dB, and
  plucks of noise 13 dB, each at 3.23 to 3.24 bits a sample.
- **Fuzz target:** `qoa`, with seeds in `fuzz/corpus/qoa/`: the small
  vectors, and three of two frames. A sound it decodes is encoded again
  and decoded, which fuzzes the encoder too. Most mutations change a file's
  length, which the decoder refuses first; the damage tests in
  `crates/codec/tests/qoa.rs` change bytes in place and reach the frames.

### Sound file reader and writer (documented, tested)

`kpcm::encode(&Header, &[i16])` writes the sound file, `.kpcm` (below,
"Sound file"), of 16-bit samples, interleaved; `kpcm::decode(&[u8], Limits)`
reads one back into a `Sound`, its header and its samples. The format is
lossless and canonical: a file that decodes encodes back to its own bytes.
As built in M6.8 (ADR-0049).

- **Accepted:** version 1; 1 or 2 channels, the mixer's; a sample rate of 1
  to 384,000 Hz (`kpcm::MAX_SAMPLE_RATE`, nadir-audio's); 1 frame or more;
  the reserved byte 0; exactly the samples the header gives, and nothing
  after them.
- **The checks run in this order**, and the first that fails is the error:
  the magic, `KPCM`, or as much of it as there is (so a short file of
  another kind is "not a sound file", not "cut short"); the header's 16
  bytes; the version (any other is "not supported"); the channels; the
  reserved byte; the rate; the frames; the samples' bytes, 2 each, against
  `max_decoded_bytes`; the length. Nothing is allocated before.
- **The encoder** refuses, in this order, channels other than 1 or 2, a
  rate of 0 or above 384,000, no frames, and samples that are not frames ×
  channels of them.
- **Errors:** `CodecError::Kpcm { offset, kind }`, with the byte offset of
  the field, or of the input's end where it is cut short, or of the first
  byte after the samples; the encoder's `CodecError::KpcmEncode { kind }`.
- **Vectors:** `crates/codec/tests/kpcm/`: 23 files, made by
  `make_vectors.py` there field by field with Python's `struct`, sharing no
  code with the reader. Six read, each with its samples written apart
  (`.i16`), chosen so that bytes or channels swapped would read otherwise
  (mono, stereo, one frame, the extremes of 16 bits at 384,000 Hz, silence,
  100 frames); the encoder writes each byte for byte. Seventeen each break
  one rule (versions 0 and 2, channels 0 and 3, rates 0 and 384,001, a
  3-byte file that is not one, a QOA header, cut in the header or the
  samples, a byte after them, the limit), with the kind and the offset.
  Every prefix of a good file, and one byte more, is refused; 3,000
  damaged ones are read or refused without a panic, and those read encode
  back to their bytes.
- **Fuzz target:** `kpcm`, with seeds in `fuzz/corpus/kpcm/` (the six good
  vectors): a file it decodes has the samples its header gives, and
  encodes back to its own bytes.

### Resampler (documented, tested)

`pcm::resample(&[f32], channels, from, to, looped, Limits)` changes a
sound's rate; `pcm::resampled_frames` says how many frames it gives;
`pcm::to_i16` makes a 16-bit sample of an `f32` one. As built in M6.8
(ADR-0049), for the cook, and for M6.9's conversion at load.

- **The filter:** a windowed sinc over 32 zero crossings each side, under a
  Kaiser window of β = 9, its −6 dB cutoff at 0.91 of the lower of the two
  rates' Nyquist frequencies, tabulated at 512 points a zero crossing and
  interpolated; each output sample's weights are divided by their sum, so a
  constant stays the constant. Measured, alike at every ratio tried, from
  384,000 Hz to 8,000 and back: a tone up to 0.82 of the lower Nyquist
  frequency keeps its level within 0.00021 dB and stays 92 dB or more below
  the tone itself; 0.84 of it loses 0.015 dB, and 0.86, 0.24 dB; a tone 1%
  above the output's Nyquist frequency is down by 91 dB. Ten seconds of
  mono at 44.1 kHz take about 0.4 s to 48 kHz, optimized.
- **Rates:** 1 to 384,000 Hz (`pcm::MAX_RATE`, the sound file's), falling
  by 48 times at most (`pcm::MAX_FALL`, 384,000 Hz to 8,000), which bounds
  the kernel's reach to 1,688 input frames each side of a point.
- **Length and place:** `frames × to / from` frames, rounded to the
  nearest, 1 or more; output frame `n` is at input frame `n × frames /
  output frames`, as a whole number and a remainder, so nothing drifts.
- **Edges:** 0 past either end of a sound; a looped one goes on from its
  other end, so its seam is resampled as any other point, and the loop's
  length is exact (its pitch moves by at most half an output frame over
  the loop): a tone looped in 10,000 frames at 44.1 kHz keeps its seam
  within −100 dB at 48 kHz.
- **Equal rates** give the samples as they are.
- **Determinism:** `f64` with `+ − × ÷`, `sqrt` and `floor` only, which IEEE
  754 fixes and Rust neither fuses nor reorders; `sin(πx)` is a polynomial
  of the module's, exact at whole and half numbers, and the Kaiser window's
  I₀ a running product, as a system's `sin`, `exp` and `powi` may differ in
  their last bit. Tests pin the hashes of two resampled signals and of the
  kernel's own `f64` bits, which the `f32` output can hide, and CI checks
  them on Linux, Windows and arm64 macOS.
- **The output** can pass ±1 where the input's peaks fell between its
  samples: `to_i16` (`× 32768`, rounded half away from 0, clamped to
  −32768 … 32767, so a sample read from 16 bits comes back as it was)
  clamps them, and the cook counts them.
- **Errors:** `CodecError::Pcm { kind }`, for no channels, a rate of 0 or
  above 384,000 Hz, a fall of more than 48 times, samples that are not
  whole frames, or an output past `max_decoded_bytes` at 4 bytes a sample,
  before anything is allocated.

### BMFont subset (documented, tested)

`bmfont::parse(&str, Limits)` reads a font in BMFont's text format into a
`Font`: its face, size, line height and base, its pages' size and files,
its glyphs, its kerning pairs, what each channel of the pages holds, and
its fallback glyph, if it has one. The cook turns it into the glyphs'
frames of an atlas and a font file (M5.5; below, "Font file").

- **Lines:** a tag, then `key=value` pairs, separated by spaces or tabs. A
  value is an integer, a list of integers with commas, or a string in
  double quotes, with no escapes: it ends at the first quote that a space,
  a tab or the end of the line follows, so `letter="""`, as exporters
  write the quote character, is a quote. LF or CRLF; blank lines,
  trailing spaces and a leading byte-order mark are skipped.
- **Tags:** `info` first and `common` second, each once; then `page`,
  `chars`, `char`, `kernings` and `kerning`, in any order. Any other tag is
  rejected, which also catches the XML and binary forms given by mistake.
  The keys the font asset needs must be there, each once a line; others,
  which generators add of their own, are ignored. `chnl` is 0 to 15 (the
  channels 1, 2, 4 and 8, together), and may be left out, meaning all
  four.
- **Ids:** code points when `info` says `unicode=1` (Snowb lists the ids in
  `charset` beside it) or has an empty `charset` (AngelCode's BMFont in
  Unicode, and Hiero, which writes `unicode=0` with code points);
  Windows-1252 bytes, mapped to code points, for `charset="ANSI"`. Any
  other charset is rejected, saying to export as Unicode. A `char` of id −1
  is the fallback glyph that BMFont writes when asked to.
- **Checked:** a `page` line for every page, each once; each glyph's page
  exists, and its rectangle lies inside the page; the ids of glyphs and
  kerning pairs are characters (no surrogates); each glyph and each
  kerning pair once; `chars count` and `kernings count`, where given, are
  the numbers of `char` and `kerning` lines in the file.
- **Rejected, for what the text pipeline cannot draw:** `packed=1` (glyphs
  in each channel; other values than 0 and 1 are out of range), `xFpBits`
  other than 0 (Snowb's fixed-point advances),
  a number with a fraction, and a `distanceField` line (both from
  distance-field generators).
- **Not checked here:** that the page files exist, are PNG, are
  `scaleW × scaleH`, and have safe paths; the cook does that, and checks
  their alpha (M5.5).
- **Limits:** `max_string_bytes` for each string the font keeps (the face
  and the page files; the strings of keys it ignores are not copied),
  `max_entries` for the pages, the glyphs and the kerning pairs. Each line
  is split into its pairs as it is reached, so a limit or an error stops
  the parse there.
  Repeated keys on a line of more than 16 pairs are found at the line's
  end, and repeated pages, glyphs and kerning pairs at the file's, by
  sorting, so that no check is quadratic.
- **Errors:** `CodecError::BmFont { line, column, kind }`, with the line
  and column of the tag or pair at fault. Lines and columns start at 1,
  and columns count characters.
- **Vectors:** `crates/codec/tests/bmfont/`: hand-written files in the
  dialects of AngelCode's BMFont (Unicode, and ANSI with a fallback glyph),
  Hiero (two pages, and `unicode=0` with code points), a tool that writes
  tabs, CRLF and a byte-order mark, and the least a file can be. Error
  cases are in the unit tests of `crates/codec/src/bmfont.rs`.
- **Fuzz target:** `bmfont`, with seeds in `fuzz/corpus/bmfont/`. Bytes
  that are not UTF-8 become U+FFFD, so that every input reaches the
  parser.

### Aseprite subset (documented, tested)

`aseprite::parse(&str, Limits)` reads the data file Aseprite writes for a
sprite sheet, `aseprite -b in.aseprite --sheet out.png --data out.json
--format json-array --list-tags`, into an `Export`: the sheet's file name
and size, its `ExportFrame`s, and its `Tag`s, each expanded to the clip it
makes. Only the cook reads it (M5.5, ADR-0035). Checked against Aseprite's
exporter (`src/app/doc_exporter.cpp`) and player (`src/doc/playback.cpp`).

- **Text:** read with `json::parse`, so the JSONC leniencies pass, and a
  raw tab or carriage return in any string fails, as the JSON parser
  refuses them; Aseprite writes names and user data with only `\`, `"`
  and the newline escaped.
- **Members:** those used must be there with their types; the others
  (`filename`, `trimmed`, `app`, `version`, `format`, `scale`, a tag's
  `color` and `data`, `layers`, `slices`, and what later versions add) are
  ignored. `frames` must be a list: the json-hash form is refused, saying
  to export with `--format json-array`.
- **Frames:** one at least. Not rotated (Aseprite never rotates). `frame`,
  the pixels' rectangle, is 1 × 1 or more and inside `meta.size`;
  `spriteSourceSize` has its size (they differ with `--inner-padding`,
  refused) and places it inside `sourceSize`, which every frame shares
  (sprites of other sizes in one export differ, refused; split layers
  share their sprite's size, and are not seen);
  `duration` is 1 to 65535 ms, as an `.aseprite` file holds it.
- **Sheet:** `meta.image` is a file name with no `/` or `\`, as Aseprite
  writes it; missing, it says to export with `--sheet`.
- **Tags:** `meta.frameTags` may be left out (no `--list-tags`). Each has
  a name that is not empty and has no `/`, no two the same (found by
  sorting); `from <= to <` the frame count; a direction of `forward`,
  `reverse`, `pingpong` or `pingpong_reverse`; and maybe `repeat`, a
  string of 1 to 5 ASCII digits of at most 65535, as Aseprite writes it
  when it is more than 0.
- **Clips**, the frames Aseprite plays:
  - no `repeat`, or 0, loops: `forward` is `from` to `to`, `reverse` `to`
    to `from`, `pingpong` `from` to `to` and back to the frame after
    `from`, so that each turning frame shows once a cycle, and
    `pingpong_reverse` the same from `to`;
  - `repeat` N plays N passes and stops on the last frame shown: a pass of
    `forward` or `reverse` is the whole tag; of a ping-pong, one way
    across it, each pass after the first going back without the frame it
    turned on;
  - a tag of one frame is that frame, once a cycle, or N times.
  - All the tags' clips together are at most `max_entries` frames: each
    tag's clip is counted, with those before it, before it is made, in 64
    bits, where the count cannot overflow.
- **Not supported, and not always detectable:** `--split-layers`,
  `--split-tags`, `--ignore-empty`, `--tag` and `--frame-range`, which
  number the export's frames apart from its tags' `from` and `to`.
- **Errors:** after the JSON, `CodecError::Aseprite { at, kind }`, with
  `at` a member path such as `frames[3].frame.w`, since the JSON tree keeps
  no line and column; the JSON's own errors keep theirs
  (`CodecError::Json`).
- **Vectors:** `crates/codec/tests/aseprite/`, written by hand as the
  exporter writes them: Aseprite 1.2 with layers and slices, 1.3 with
  repeats, `pingpong_reverse` and user data, a trimmed packed sheet with
  merged duplicates, and an export with no tags; and the sandbox's
  `assets/sprites/person.json`. Error cases and each direction's clips are
  in the unit tests of `crates/codec/src/aseprite.rs`.
- **Fuzz target:** `aseprite`, with seeds in `fuzz/corpus/aseprite/`.
  Bytes that are not UTF-8 become U+FFFD. What it accepts must keep the
  promises above, which the target asserts.

### Atlas file reader (documented, tested)

`atlas::parse(bytes, limits)` reads the cook's `atlas/<group>.kbj`
(below, "Atlas file") into an `AtlasFile` of `Page`s and `SpriteEntry`s,
each sprite a `SpriteKind::Pivot` or `SpriteKind::Tiles` with its `Frame`s
and its `ClipEntry`s. The types are `#[non_exhaustive]`: only the reader
makes them. As built in M5.9 (ADR-0033), and given clips in M5.5
(version 2, ADR-0035):

- **KBJ first**, with the KBJ decoder and its limits; a file that is not
  KBJ is a `CodecError::Kbj`.
- **The version first:** any whole number other than 2, 1 included, is
  `Version { found }`, "not supported", whatever else the file holds, so a
  later file's new members never read as wrong; a version 1 file is cooked
  again.
- **Strict:** each object has its members and no others; one page at
  least, of 1 to 4096 a side, with a non-empty path; sprite names
  non-empty, in byte order, none twice; one frame at least each; a pivot of
  two finite numbers that fit an `f32`, or a tile size of two whole numbers
  of 1 or more, not both.
- **Frames:** whole numbers that fit 32 bits, on a page that exists and
  inside it, and inside their untrimmed cell of 1 × 1 or more (checked
  sums). An empty frame is 0 × 0 on page 0 at (0, 0) with no offset; a
  frame of one side 0 is refused.
- **Tile sets:** one frame, the whole of its page, untrimmed, a whole
  number of tiles across and down, and no clips.
- **Clips:** only a sprite with a pivot has `clips`, and then one at
  least; each a name, not empty and with no `/` (it would make its full
  name, `sprites/person/walk`, another's), in byte order with no two the
  same; `loop`, true or false; and one step at least, each a frame of the
  sprite and a time of 1 ms or more that fits 32 bits.
- **Errors** name where in the file: `CodecError::Atlas { at, kind }`, with
  `at` a member path such as `sprites[2].frames[0].w`, since a KBJ value
  carries no byte offset; empty for the file as a whole.
- **Tests:** a rule a test, from hand-written JSON text encoded to KBJ, in
  `crates/codec/src/atlas.rs`, and the sandbox's file as M5.9 cooked it,
  `crates/codec/tests/atlas/version_1.kbj`, refused. **Fuzz target:**
  `atlas`, with seeds in `fuzz/corpus/atlas/` (the sandbox's cooked file,
  with its clips, the least a file can be, a tile set, a sheet with an
  empty frame, a sprite with clips that loop and do not); what it accepts
  must keep the promises above, which the target asserts.

### Font file reader (documented, tested)

`font::parse(bytes, limits)` reads the cook's `atlas/<font>.font.kbj`
(below, "Font file") into a `FontFile`: the face, size, line height and
base, the atlas file and sprite of the glyphs' frames, its `Glyph`s by
character, its `Kerning` pairs, and its `Fallback`. The types are
`#[non_exhaustive]`: only the reader makes them. As built in M7.7
(ADR-0053); the cook reads back each font file it writes with it, and
render's `Font` asset reads it (M7.1, ADR-0054):

- **KBJ first**, with the KBJ decoder and its limits, which bound the
  glyphs and kerning pairs by `max_entries` and the strings by
  `max_string_bytes`; a file that is not KBJ is a `CodecError::Kbj`.
- **The version first:** any whole number other than 1 is
  `Version { found }`, "not supported", whatever else the file holds.
- **Strict:** each object has its members and no others, but a glyph's
  `frame`, which one with no pixels leaves out; the fallback has a frame.
- **Numbers:** whole; `size`, each `x`, `y` and `advance`, and a kerning
  pair's amount fit 32 bits signed, as BMFont's do (a negative `size` is
  BMFont's "match the characters' height"); `line_height`, `base` and the
  frames fit 32 bits unsigned.
- **Strings:** `face` may be empty, as BMFont's may; `atlas` and `sprite`
  may not.
- **Characters:** the glyphs' ids and each kerning pair's two are code
  points of characters, read as Rust's `char`: from 0 to U+10FFFF, and no
  surrogate.
- **Order:** the glyphs by character, and the kerning pairs, each a list of
  three, by their first and then their second, each after the one before
  with no two the same, as the cook writes them.
- **Not checked here:** that a frame is one of the sprite's in the atlas
  file, which the font asset reads beside it; and what BMFont does not
  promise, that a kerning pair's characters are the font's, or that
  `base` is within `line_height`.
- **Errors** name where in the file: `CodecError::Font { at, kind }`, with
  `at` a member path such as `glyphs[3].id`; empty for the file as a whole.
- **Tests:** a rule a test, from hand-written JSON text encoded to KBJ, in
  `crates/codec/src/font.rs`; four files as the cook wrote them, in
  `crates/codec/tests/font/`, checked field by field in
  `crates/codec/tests/font.rs`, and each cut at every length and with every
  byte changed:
  Unicode with kerning, a space and a clear glyph with no frames, and
  characters past ASCII; ANSI with BMFont's fallback glyph and a negative
  size; two pages; and an empty face with no glyph but BMFont's fallback.
  They are made by
  `crates/codec/tests/font/make_vectors.py`, which writes their BMFonts
  and pages and runs the cook. **Fuzz target:** `font`, with the same
  files as seeds in `fuzz/corpus/font/`; what it accepts must keep the
  promises above, which the target asserts.

### LDtk subset (documented, tested)

`ldtk::parse_project(&str, Limits)` reads a project `.ldtk`, and
`ldtk::parse_level(&str, &Project, Limits)` one of its level files,
`.ldtkl`, as LDtk 1.5.3 saves them, into a `Project` of `TilesetDef`s,
`LayerDef`s, `EntityDef`s and `Level`s, each level's `LayerInstance`s with
their `TileInstance`s and `EntityInstance`s, and fields as
`FieldInstance`s: a type's name and the JSON value. The types are
`#[non_exhaustive]`. As built in M5.10 (ADR-0038), for the cook only:

- **JSON first**, with the JSON parser and its limits; its errors keep
  their line and column.
- **The version first:** a `jsonVersion` of 1.5.x, three numbers, the first
  two 1 and 5; any other, whatever else the file holds, is
  `Version { found }`: "LDtk 2.0.0 is not supported: save the project with
  LDtk 1.5.3" (`ldtk::VERSION`).
- **Members:** those the types hold, with their types; one that may be
  `null` may be left out too, as LDtk's schema allows; the others LDtk
  writes, or adds later, are ignored. A non-empty `worlds`, Multi-Worlds,
  is refused. `imageExportMode` is one of LDtk's four values. No check is
  quadratic: definitions are found by their uids sorted, and IntGrid values
  among their layer's sorted.
- **Definitions:** tile sets, layers and entities each of a uid of their
  own; a tile set's grid of `__cWid` × `__cHei` at most `max_entries`
  tiles, and its `customData` of tiles it has; a layer definition's
  `tilesetDefUid`, if any, a tile set the project has; IntGrid values of
  1 or more.
- **Levels:** a size of 1 pixel or more a side; `__bgColor` `#rrggbb`. In a
  project, a level has an `externalRelPath` exactly when its
  `layerInstances` is `null`, and every level does when `externalLevels`
  is true, none otherwise.
- **Layers:** of a definition the project has, and of its type; a grid of at
  most `max_entries` cells; an IntGrid's `intGridCsv` one value a cell, row
  by row, each 0 or one of its definition's, and any other layer's empty.
  A tile (`gridTiles`, `autoLayerTiles`) is of the layer's tile set
  (`__tilesetDefUid`), which a layer with tiles must have; its `f` 0 to 3;
  its `a` 0 to 1. An entity's `defUid` is a definition the project has.
  Two tiles in one cell are kept, in the order they are drawn: the cook
  refuses them (below, "LDtk levels").
- **Level files:** one level, with its layers, checked against the
  project's definitions; its `iid` one of the project's levels saved
  apart.
- **Errors** name where in the file: `CodecError::Ldtk { at, kind }`, with
  `at` a member path such as `levels[0].layerInstances[2].gridTiles[5].t`.
- **Tests:** `crates/codec/tests/ldtk.rs`, on projects that
  `crates/codec/tests/ldtk/make_vectors.py` writes from LDtk 1.5.3's JSON
  schema, with every member it requires (no LDtk editor wrote them): one
  level of each layer type, with an entity's field of each type, tiles by
  rules and by hand, two in one cell; a project that saves its two levels
  apart, and their level files; each rule broken on a copy, and other
  versions. **Fuzz target:** `ldtk`, each input read as a project and as a
  level file of the seed `external.ldtk`, with seeds in
  `fuzz/corpus/ldtk/`; what it accepts must keep the promises above, which
  the target asserts.

### Level file reader (documented, tested)

`level::parse(bytes, limits)` reads the cook's `levels/<name>.klvl`
(below, "Level file") into a `LevelFile` of `TilesetEntry`s (with their
`Animation`s), `LayerEntry`s (with their `Cell`s), a `GridEntry`,
`TallEntry`s, `EntityEntry`s and `Bounds`. The types are
`#[non_exhaustive]`. As built in M5.10 (ADR-0038):

- **KBJ first**; a file that is not KBJ is a `CodecError::Kbj`.
- **The version first:** any whole number other than 1 is `Version
  { found }`, "not supported", whatever else the file holds.
- **Strict:** each object has its members and no others; `grid`, `music`,
  `ambience` and `camera` may be left out.
- **The rules** are the format's, below: sizes, channels, assets in byte
  order, each tile set's atlas among them, at most 65535 tiles a set,
  animations in order and apart, one cell for each of a layer's and the
  grid's, each cell's tile of its set and its flips 0 to 3, tall blocks
  inside their set, prefab and component names, no `null` override.
- **Errors** name where: `CodecError::Level { at, kind }`, such as
  `layers[0].cells[130]` or `entities[2].overrides.Health`.
- **Tests:** a rule a test, from hand-written JSON text encoded to KBJ, in
  `crates/codec/src/level.rs`. **Fuzz target:** `level`, with seeds in
  `fuzz/corpus/level/`, cooked from JSONC by the cook's data pipeline: a
  level of every member, the least a level can be (a size and a
  background, and empty lists), and one of two tile sets and layers, mirrored
  every way; and the sandbox's field, `field.klvl`, as the cook writes it
  from `assets/levels/field.ldtk` (M5.12). The target asserts the promises
  above.

### Prefab file reader (documented, tested)

`prefab::parse(bytes, limits)` reads a cooked prefab,
`data/prefabs/<name>.kbj` (below, "Prefab file"), and `prefab::read(&Value)`
the same from a value, so that the cook can check a prefab's JSONC before
it writes it, into a `PrefabFile`: its name, the one it extends, and its
components' values in the file's order, which nadir-scene reflects. As
built in M5.10 (ADR-0038):

- **Strict:** `name` and `components`, `extends` if it has a base, no
  other member.
- **Names:** a prefab's, and its base's, lowercase ASCII letters, digits and
  `_` (`prefab::is_name`), the base not its own; a component's, an ASCII
  letter then letters, digits and `_` (`prefab::is_component_name`), as a
  type's `TYPE_NAME` is.
- **No `null`:** a component's value is not `null`, as version 1 has no way
  to remove a base's component.
- **Errors** name where: `CodecError::Prefab { at, kind }`, such as
  `components.Health`.
- **Tests:** beside the reader, in `crates/codec/src/prefab.rs`. **Fuzz
  target:** `prefab`, with seeds in `fuzz/corpus/prefab/`: a prefab with a
  base, its base, and one with no components.

### Pack file reader (documented, tested)

`kpak::header(start, file_size, limits)` reads a pack's first 32 bytes,
`kpak::toc(&header, bytes, limits)` its TOC from the `toc_size` bytes at
`toc_offset`, and `Toc::decode(index, stored, limits)` an entry from its
stored bytes, so that a reader of a file takes only the parts it needs;
`kpak::parse` and `Toc::read` do the same for a pack in memory.
`Toc::find(path)` is a binary search of the path hashes; with a string
table, an entry of another path is not found. The format is below
(`nadir-assets`, "Pack file `.kpak` (version 1)"). As built in M5.13
(ADR-0041):

The checks run in the order below, and the first that fails is the error:

- **The header:** the magic, then the version (any other is not
  supported), the flags (bit 0 only) and the reserved field; at most
  `max_entries` entries, the 1,000,000 pack entries of "Limits"; the TOC at
  a multiple of 16 after the header, with room for its entries (exactly,
  without a string table), at most `max_decoded_bytes`, and ending where
  the file does.
- **The TOC:** its size, the header's, and its CRC-32; at most
  `max_entries` entries; then each entry in turn: its codec, 0 or 1, its
  reserved bytes, its stored bytes between the header and the TOC at a
  multiple of 16, both sizes at most `max_decoded_bytes`, a stored entry's
  two sizes equal, and its path hash greater than the one before it (so
  none twice); then entries that share bytes only as one blob (the same
  offset, sizes, codec and CRC); then each path of the string table UTF-8,
  not empty, and of its entry's hash; nothing after the last.
- **An entry:** the CRC-32 of its stored bytes, before they are
  decompressed, so that a damaged entry is `ErrorKind::Damaged`, which
  names its path; a KLZ stream that cannot be read under a right CRC is
  `BadStream`, which names it too.
- **Not checked:** the bytes between entries (the writer's padding), and
  whether a path is an asset path, which nadir-assets checks of the paths
  it asks for.
- **Errors** say where: `CodecError::Kpak { offset, kind }`, at the byte in
  the file of the field that is wrong, or of the entry's bytes.
- **Test vectors:** `crates/codec/tests/kpak/`, written field by field by
  `make_vectors.py`, which shares no code with the reader, and read by
  `crates/codec/tests/kpak.rs`: 6 packs that read (no entries; two; two
  with no string table; blobs shared and an empty file; 42 entries, one
  with a path of 300 bytes; a KLZ entry larger than its data), 31 that
  each break a rule of the format, with the error and its byte, and 3
  whose TOC reads but one entry does not (damaged, with and without a
  string table, and a bad KLZ stream). Each broken pack breaks only its
  rule, its checksums right, where it can; one whose rule cannot be
  broken alone, such as an entry count no file of its size could hold,
  breaks rules checked after it too, and is refused by its own.
  Every prefix of three of them, and each with a bit changed, reads or is
  refused without a panic.
- **Fuzz target:** `kpak`, with seeds in `fuzz/corpus/kpak/`: the 5 packs
  that read, the damaged one and the bad stream. Its TOCs and entries are
  at most 1 MiB, as `klz`'s streams are. It reads each input, then a
  repaired copy: the header's magic, version, reserved field and unknown
  flags restored; the TOC found again from the end when its offset is
  lost, moved to a multiple of 16 by zeros before it; its size and its
  entry count mended; each entry's path hash taken from its path, and the
  entries put back in order; every CRC rewritten. It asserts what
  `PackSource` relies on: the TOC of the header, the same read whole or in
  parts; its entries in order of their hashes, inside the data region at a
  multiple of 16, a stored one of its raw size; each path of its entry's
  hash, and found by it; each entry read to its raw size, its stored bytes
  if stored, or refused as damaged or a bad stream, and nothing else. Of
  the first 20,000 inputs a run with the default seed mutates from the
  seeds, 487 read as they are and 3,368 once repaired; 303 reach an
  entry's KLZ stream as they are, and 1,639 repaired.

## `nadir-assets`

### Handles and storage

As built in M3.2 and M5.3 (ADR-0030): loading on worker threads from a
`Source`, finalizing on the main thread within a frame budget, and freeing
by scope. As built in M5.16 (ADR-0042): reloading behind a handle, and the
watcher (below, "Hot reload (dev)").

```rust
pub use nadir_core::Handle;                 // { id: GenIndex, marker: PhantomData<fn() -> T> }: Copy, Eq, Hash
pub struct Assets<T> { arena: GenArena<Entry<T>>, by_path, pending: Vec<Handle<T>>, /* result channel, released, reloading, swapped */ }
struct Entry<T> { path: String, scopes, slot: Slot<T>, reads: u64, landed: u64, reloaded: Option<Result<T::Decoded, AssetError>> }
enum Slot<T> { Decoding, Pending(Result<T::Decoded, AssetError>), Loaded(T), Failed { error: AssetError, placeholder: Option<T> }, Finalizing }
pub enum LoadState { Loading, Loaded, Failed }   // Loading: decoding, or waiting to be finalized
pub enum OnFailure { Placeholder, Nothing }
pub enum Scope { Global, Level, Transient }

impl AssetServer {
    pub fn new(root: impl Into<PathBuf>, on_failure: OnFailure) -> Self;          // a LooseSource
    pub fn with_source(source: impl Source + 'static, on_failure: OnFailure) -> Self;
    pub fn load<T: Asset>(&mut self, path: &str) -> Handle<T>;                    // load_in(path, Scope::Global)
    pub fn load_in<T: Asset>(&mut self, path: &str, scope: Scope) -> Handle<T>;   // dedups by path; queues the read
    pub fn load_now<T: Asset>(&mut self, path: &str, ctx: &mut FinalizeCtx<'_>) -> Result<Handle<T>, AssetError>;  // load_now_in(.., Global, ..)
    pub fn load_now_in<T: Asset>(&mut self, path: &str, scope: Scope, ctx: &mut FinalizeCtx<'_>) -> Result<Handle<T>, AssetError>; // M5.9: made before it returns
    pub fn unload_scope(&mut self, scope: Scope) -> usize;                        // how many went
    pub fn get<T: Asset>(&self, h: Handle<T>) -> Option<&T>;        // loaded, or a failed one's placeholder
    pub fn state<T: Asset>(&self, h: Handle<T>) -> Option<LoadState>;  // None: nothing under that handle
    pub fn error<T: Asset>(&self, h: Handle<T>) -> Option<&AssetError>;
    pub fn path<T: Asset>(&self, h: Handle<T>) -> Option<&str>;
    pub fn assets<T: Asset>(&self) -> Option<&Assets<T>>;
    pub fn has_pending(&self) -> bool;                             // loads or reloads out, or assets to free
    pub fn set_finish_budget(&mut self, budget: Duration);         // 2 ms unless set
    pub fn finish_completed_loads(&mut self, ctx: &mut FinalizeCtx<'_>); // main thread, once a frame; never waits
    pub fn finish_all_loads(&mut self, ctx: &mut FinalizeCtx<'_>);       // waits for every load: tools, tests, offscreen frames
    pub fn reload(&mut self, path: &str) -> usize;                 // M5.16: each type at the path; how many
    pub fn start_watcher(&mut self) -> std::io::Result<()>;        // M5.16: the thread nadir-assets-watch
    pub fn reload_changed(&mut self) -> usize;                     // M5.16: once a frame; allocates nothing when nothing changed
    pub fn drain_swapped<T: Asset>(&mut self) -> impl Iterator<Item = Handle<T>> + '_; // M5.16: the handles a reload swapped
}

pub trait Asset: Sized + Send + 'static {
    type Decoded: Send + 'static;                                  // made on a worker
    const EXTENSIONS: &'static [&'static str];
    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<Self::Decoded, AssetError>;
    fn finalize(decoded: Self::Decoded, ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError>; // main thread (GPU upload)
    fn placeholder(ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError>;   // default: NoPlaceholder
    fn release(self, ctx: &mut FinalizeCtx<'_>) -> Result<(), AssetError>;  // default: drop; a texture destroys itself
}

pub struct DecodeCtx<'a> { /* path, limits */ }
pub struct FinalizeCtx<'a> { /* up to 4 `&mut dyn Any` resources, by type: no allocation */ }
```

- **Workers:**
  - Each server starts `clamp(cores − 2, 1, 4)` threads, `nadir-assets-N`,
    which take jobs from one queue. That leaves the main thread a core and
    one to spare, so that no other thread takes the main thread's core
    while assets load. A job reads the file through the
    server's `Source` and decodes it, and sends the result on its asset
    type's channel.
  - A panic while reading or decoding fails that asset alone, as
    `Invalid("reading or decoding it panicked")`.
  - With no thread started, jobs run on the thread that asks for the load.
  - Dropping the server drops the jobs still queued and waits for those
    running.
- **Loading:**
  - `load` gives the same handle for the same path, whatever happened to
    its load, until every scope it was loaded in is unloaded. Otherwise it
    checks the path and queues the job: it never reads, so it never
    waits. A path that fails the check fails with no job.
  - `finish_completed_loads` takes the workers' results and frees the
    assets of unloaded scopes, all of them, outside the budget. It then
    makes decoded assets until the budget runs out, and makes one at least
    each call. An asset stays `Loading` until then.
  - Each type's assets are made in the order they were loaded, passing
    over those still decoding, so a later load can be made first. The
    types share the budget and take turns at going first, so one type's
    backlog cannot hold another's back for long.
  - `load_now` (M5.9, ADR-0033) gives the handle `load_in` would, and
    makes the asset before it returns: a new path is read and decoded on
    the calling thread; one a worker is decoding is waited for, and one
    waiting to be made is made now. It makes and frees nothing else, and
    spends no budget. It is for the data a game needs before its first
    tick, such as an atlas's regions: its context must hold whatever
    `finalize` needs, a failure is final, and before the window opens
    there is no GPU device, so only assets that need none can be made then.
    On a failure it returns the error; `load` of the path gives the failed
    handle, with its placeholder if shown.
  - `finish_all_loads` waits for every job and makes every asset, with no
    budget: type by type, in the order of each type's first load, and each
    type's in load order, so the same loads are made in the same order on
    every run.
  - A failure shows only when it is finished, and is logged with the path:
    a missing file is `NotFound`.
  - The facade finishes within the budget after each window frame, and
    waits for every load after each offscreen frame (`App::offscreen`), so
    that recordings are the same on every run. A headless run finishes
    nothing, so tick systems must not depend on load state (ADR-0025).
- **Paths (`AssetPath`):**
  - ASCII letters are lowercased.
  - The rest is checked as one byte grammar on every system: `/` between
    parts, no empty, `.` or `..` part, and no part ending in a dot or a
    space or named like a Windows device.
  - No control character, `\`, `:` or `*?"<>|` is allowed, and a path is
    at most 1,024 bytes. A path that fails is refused, not repaired.
  - The last part must end in one of `T::EXTENSIONS`.
- **`FinalizeCtx`** holds the resources `finalize` and `release` need, by
  type, such as render's `gfx::Device`. A missing one is
  `AssetError::MissingContext`.
- **`AssetError`** holds no path: the server logs each failure with its
  path. Its variants are `BadPath`, `Extension`, `NotFound`, `Read`,
  `Decode(CodecError)`, `Invalid(String)` (an asset type's own reason),
  `MissingContext` and `NoPlaceholder`.
- **Reference handling:** handles are plain IDs; there is no reference
  counting.
  - An asset belongs to every scope (global, level, transient) it was
    loaded in, and lives until all of them are unloaded. `load` loads in
    the global scope.
  - `unload_scope` removes the assets it leaves in no scope. Their handles
    stop resolving at once. The next finish frees each with
    `Asset::release`, and each failed one's placeholder too. One still
    decoding is dropped when its result comes.
  - Levels declare their asset list. Unloading a level frees every asset
    scoped to it that no other scope also holds.
  - The facade's `Clips` and `TileLayers` are not assets (ADR-0023,
    ADR-0024): `load_level` removes a level's own (M5.6).
  - This is explicit, predictable, and has no hidden `Drop` cost.
- **Placeholders in dev:**
  - The server's `OnFailure::Placeholder` shows them. It is a runtime
    setting, which the facade picks under its `dev` feature. The user chose
    this on 2026-10-19.
  - A failed texture becomes a magenta/black 8×8 checker. Its state stays
    `Failed`, and `get` gives the placeholder.
  - A failed sound becomes a frame of silence, which plays as silence, and
    a failed music track a QOA frame of silence, which the music fades to
    (nadir-audio's `Sound` and `Music`, M6.9, ADR-0050).
  - Failures log with the path and the error.
- **Release:** a missing level file or prefab is an error from `load_level`
  (a `LevelError`, M5.6), and a missing texture a hard error with a dialog
  in M9.1's loading state (ADR-0038). Until then, with
  `OnFailure::Nothing`, it is `Failed` and logged.

### Virtual file system

```rust
pub trait Source: Send + Sync {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>;      // NotFound, Read, or Decode (a damaged pack entry)
    fn modified(&self, path: &AssetPath) -> Option<std::time::SystemTime> { None }  // loose files tell
}
```

- **`LooseSource`** reads files under a root directory: in development,
  `target/cooked/`. It stays in every build (ADR-0041).
- **Which the engine reads** (M5.7, ADR-0041): `App::new` reads
  `<Config::assets>/game.kpak` through a `PackSource` in builds without
  `dev`, and the loose files when there is no pack there
  (`PackSource::open` gives `NotFound`); with `dev` it reads the loose
  files, which hot reload watches (M5.8). A pack that is there but does
  not open (`Read`, or `Decode` for one the reader refuses) is logged, and
  every load then fails with that error, rather than reading loose files
  that may not be its cook's. The release files and the demo page's
  download ship `assets/game.kpak` alone.
- **`PackSource`** reads the cook's pack, `game.kpak`
  (`PackSource::FILE_NAME`), through `nadir_codec::kpak`. As built in
  M5.14 (ADR-0041):
  - `PackSource::open(path)` reads the header and the TOC, within the
    game's `Limits`, and keeps the file open: `NotFound` with no file,
    `Read` for one that cannot be read, `Decode` with the codec's
    `CodecError::Kpak` for one the reader refuses.
  - `read` finds the entry by a binary search of the TOC, then, holding
    the file's mutex, seeks to it and reads its stored bytes alone; the
    CRC check and the decompression run after the lock is let go, so the
    workers decode at once. A path not in the pack is `NotFound`; a pack
    cut short since it was opened is `Read`; a damaged entry is
    `Decode(Kpak { kind: Damaged { name, .. }, .. })`, naming its path.
  - `modified` is `None`: a pack cannot tell when a file changed.
- **Paths are normalized** as `AssetPath`s, above: lowercase, forward
  slashes, no `..`. The cook tool rejects source paths that differ only in
  case, so Windows/macOS (case-insensitive) and Linux (case-sensitive)
  behave the same.

### Pack file `.kpak` (version 1)

Numbers are little-endian; ADR-0041 filled in what the TOC's and the
entries' CRCs cover, the string table and where the TOC ends. Read by
`nadir_codec::kpak` (M5.13, above).

```
header (32 bytes):
  magic "KPAK" | version u16 (1) | flags u16 | entry_count u32 | toc_offset u64 | toc_size u32 | toc_crc32 u32 | reserved u32 (0)
data region: entries, each 16-byte aligned, identical blobs stored once (dedup by their bytes)
toc (toc_size bytes at toc_offset, a multiple of 16, to the end of the file; toc_crc32 is their CRC-32):
  entries, sorted by path_hash for binary search, none twice:
    path_hash u64 (fnv1a64 of normalized path) | offset u64 | stored_size u32 | raw_size u32 | codec u8 (0 none, 1 klz) | reserved [u8;3] (0) | crc32 u32 (of the stored bytes)
  string table, if flags bit 0: for each entry in TOC order, its path's length u16 and its bytes
```

- Hash collisions between distinct paths are detected at cook time, which
  fails the build: `CookError::OutputCollision`, whose message says so.
- **The writer** (`tools/cook/src/kpak.rs`, M5.14): every cook writes
  `OUT/game.kpak` of the outputs its cache lists, after the stale files
  are removed and before the cache is written. The files are laid out in
  path order, each at a multiple of 16, one of the same bytes as an
  earlier one stored once with it; each is KLZ when that is smaller, and
  stored as it is otherwise; the TOC and its string table follow, in order
  of the paths' hashes. The pack is read back with `nadir_codec::kpak` and
  the game's `Limits`, every entry decompressed, before it is written, and
  it is written only when its bytes change, so that a cook that changes
  nothing leaves it as it was. A file larger than the game reads, more
  files than it reads, or a TOC larger, is `CookError::Encode`, as is a
  pack the reader refuses; a path longer than 65,535 bytes is
  `CookError::BadPath`. The sandbox's six cooked files, 19,030 bytes,
  pack into 5,304, each smaller as KLZ.
- At runtime, each read of an entry checks the CRC-32 of its stored bytes
  before it decompresses them. A mismatch is a corruption error naming the
  path, from the string table, which the cook always writes (ADR-0041), or
  the entry's index in a pack without one.

### Hot reload (dev)

Built in four tasks (ADR-0042): M5.15, the cook's side, M5.16
(nadir-assets), M5.17 (nadir-render) and M5.8, the game's side, in the
facade (ADR-0043).

- **The cook's side** (M5.15): `cargo xtask cook --watch [--force]` builds
  `nadir-cook` in release once and cooks, `--force` for that first cook
  alone. It then looks below `assets/` every 100 ms at what the cook's
  walk sees: files, folders, and links, not followed, with names starting
  with `.` left out; a file or a link by its time of change and its
  length, compared for equality. What cannot be read is marked so, and
  the next look tells; only `assets/` itself must be read. Once
  something changed, and nothing more by the next look, it runs the built
  cook again, and prints how long the cook took and how long since the
  change was seen. A cook that fails is printed, and the watch goes on. A
  change to the cook's own code needs the watch started again. The engine
  never links the cook tool, which keeps the layering clean.
- **What the cook writes:** an output only when its bytes change, and an
  atlas group's pages before the atlas file and the fonts' files that name
  them (below, "Incremental cache"), so that a game watching times
  of change reloads only what changed, in an order it can read. On the
  sandbox's assets, an edit is cooked 0.15 to 0.20 s after it is saved: a
  prefab's comment, which rewrites the cache alone, and a sprite's pixel,
  its page, the cache and the pack.
- **The game's watcher** (M5.16): a thread polls the times of change of
  the loaded loose cooked files every 250 ms, through `Source::modified`,
  and hands the changed paths to the main thread. It is nadir-assets',
  compiled in every build, and the facade starts it under `dev`, in
  `App::new` (M5.8).
  As built:
  - `AssetServer::start_watcher` starts the thread, `nadir-assets-watch`;
    starting it again does nothing, and dropping the server stops it.
  - A path is watched from just before its file is first read: each read
    of a watched path, by a worker or `load_now`, a load's or a reload's,
    takes its time of change before reading if no look and no read has, so
    a change made after the first read is seen at the next look, if it
    gives the file a new time of change. On a system that keeps coarse
    times, two writes within one tick of its clock look like one. Paths
    loaded before the watcher started are watched from its first look. A
    path stays watched until no asset of any type is at it
    (`unload_scope`). Every ask of the source is made with the paths'
    lock let go, so a load never waits for one, and a panic in it tells
    nothing.
  - A look copies the watched paths, with what was seen of each, into a
    buffer it keeps, asks the source, and then, under the lock, sends each
    path whose time is not the one seen before, on a channel; a missing
    file is a time of `None`, so one that appears or goes is a change too.
    A path whose `seen` changed since the copy, as a read took its time, is
    left for the next look. A path is sent once until the main thread
    receives it, however often it changes meanwhile. A stopping watcher
    ends a look before its next ask. The buffer grows only when more paths
    are watched; a `LooseSource` still makes each file's full path, each
    look.
  - `AssetServer::reload_changed`, once a frame on the main thread, reloads
    each path received, and allocates nothing when nothing changed.
- **Reload** (M5.16) swaps the data behind an existing handle, so
  references stay valid. GPU textures are re-uploaded in `finalize`. A
  reload that fails keeps the value it had. As built:
  - `AssetServer::reload(path)` queues a read of the file for each type
    with an asset at the path, and returns how many; a path that failed
    its type's check has no file to read again.
  - Each entry numbers its reads. A result older than one already back is
    dropped: up to four workers read at once, so two reloads can finish
    out of order, and the later read's value is the one that stays. A
    reload of an asset still loading lands as its first value.
  - The read waits in the entry's own reload state, `reloaded`, while the
    asset keeps its value and its `LoadState`; a list of the reloading
    handles keeps it apart from the pending loads, and
    `AssetServer::has_pending` is true until it is made.
  - A finish makes it after its type's pending loads, within the same
    budget, and swaps it in; the value it replaces, or a failed asset's placeholder, is
    released with `Asset::release` then, once, and the swap is logged. A
    reload that fails to read, decode or make is logged with its path, and
    the asset keeps its value: a failed asset stays failed. An asset
    unloaded while it reloads is dropped, as a load is.
  - `AssetServer::drain_swapped::<T>()` gives the handles of type `T` that
    a reload was swapped in for since the last call, each once, so that a
    game can find again what it copied out of them (M5.8).
- **Shaders and textures** (M5.17, M5.8): nadir-render rebuilds a program
  from sources it is given (`Renderer::reload_program`), keeping the one
  it had when they do not compile or do not match its interface, and draws
  a texture reloaded behind its handle from the next frame, with no change
  of its own (`docs/subsystems/render.md`, "Reloading"). Under `dev` the
  facade reads render's shader files as they change, and hands them over
  (M5.8).
- **The game's side** (M5.8, ADR-0043), in the facade, under `dev`:
  `App::new` starts the asset watcher, so that the files the game's setup
  reads are watched from before they are read, and `App::run`, where there
  is a device, starts the hot reload and its shader watcher, a thread,
  `nadir-shader-watch`, that looks at the times of change of render's
  shader files (`SHADER_DIR`) every 250 ms. After each drawn frame:
  - it takes the atlases, prefabs and level files a reload swapped in
    (`drain_swapped`). An atlas's clips are replaced in place by name,
    under the handles they had (`Clips::reload_atlas`): a clip the atlas
    no longer has stays as it was, with a warning, and an animator plays
    on from its time, a finished clip that plays once holding the new last
    frame. The atlas is the one of its pages, or of one of them when it
    gains or loses a page, and a page it gains is loaded in
    `Scope::Global`, as the game and a level may share it. A prefab's new
    file replaces it in the `PrefabLibrary`, under its name, and the bases
    it now extends are loaded and checked; one naming another prefab is
    refused, and the prefab stays as it was;
  - once an atlas, a prefab (one the library has not read too, as one a
    level could not load before it was cooked) or the current level's
    file was swapped, and no load or reload is pending, it makes the
    current level again behind its handles: no scope is unloaded, its
    tile layers keep their handles, its atlases' clips are held again
    before the last hold goes, and its entities are spawned anew, with new
    ids. A level that cannot be made again stays as it was. A cook whose
    files straddle a look of the watcher makes it twice, the first time
    maybe refused with half the cook's files, and logged. Before a level's
    prefabs are read, by a load or a reload, those whose files are no
    longer loaded, as after the last load unloaded `Scope::Level`, are
    removed from the library (`PrefabLibrary::remove`), so that they are
    read, and watched, again;
  - it rebuilds each program whose files changed, from its files as they
    are (`Renderer::reload_program`), or logs why not and keeps the last.
    The renderer was built from the sources compiled in, and the watcher
    starts from the files' times when the loop starts, so a shader edited
    before then is seen at its next save;
  - it hands the paths the watcher saw change to the asset server
    (`reload_changed`), whose workers read them for the next frames'
    finishes.
  - A texture needs nothing: the renderer looks its handle up each frame.
    Each reload is logged with its time: the server's swap, the clips
    found again, the prefab, the level and how long it took to make, the
    shaders and how long they took to build, and, once all are made, how
    long since the watcher handed them over. With nothing changed, a
    frame allocates nothing (the facade's count).
  - `App::offscreen` and `App::update` never reload, so recordings and
    captures do not change; nor does a build without `dev`. What the game
    copied out of an asset outside these, such as the sandbox's `Sprites`
    for its stress scenes, and the entities it spawned of prefabs itself,
    stay as they were. Under `dev` the facade takes the swapped atlases,
    prefabs and levels from `drain_swapped`, so the game does not see
    them there.
  - Measured on the sandbox, from the save of a source to the game's log
    line of its reload, which the next frame draws: 0.05 to 0.43 s for a
    sprite's pixels, an Aseprite export's tags, a prefab, the LDtk level
    and a shader, the level slowest (M5.8's notes in `docs/roadmap.md`).

## `nadir-cook` (library + CLI)

As built in M5.4, M5.5, M5.12, M5.14 and M6.8 (ADR-0031, ADR-0035,
ADR-0038, ADR-0041, ADR-0049): the atlas pipeline, with PNGs, Aseprite
exports and BMFont fonts; sounds; data, its prefabs checked; LDtk
projects' levels; the pack of every output (above, "Pack file"); the
cache; and the CLI.

### Pipelines

| Source | Pipeline | Output |
| :- | :- | :- |
| `*.png` (+ optional `*.sprite.jsonc` sidecar: sheet frames, pivot, trim, tile set) | Decode → cut into frames → trim → atlas pack → QOI | Atlas pages `atlas/<group>_<n>.qoi`, tile sets' pages `atlas/<sprite>.tiles.qoi`, and `atlas/<group>.kbj` (where each frame went) |
| `*.json`, an Aseprite export (`aseprite -b in.aseprite --sheet out.png --data out.json --format json-array --list-tags`) | Parse frames and tags (above, "Aseprite subset") → its sheet's frames join the atlas group, placed in their canvas, then trimmed; each tag a clip of them, a tag's direction and repeat count expanded to the frames Aseprite plays. `Clips::add_atlas` makes them `Clip`s (ADR-0024, ADR-0035) | The sprite's frames and clips in `atlas/<group>.kbj` |
| `*.ldtk` (LDtk 1.5.3 project, and its `.ldtkl` level files) | Read (above, "LDtk subset") → by ADR-0038's conventions (below, "LDtk levels"): tile layers → a level's layers of cells; the IntGrid `Collision` → its grid; tall tiles → its tall sprites; entities → prefab names and field overrides | `levels/<name>.klvl` (below, "Level file"), one a level |
| `audio/**/*.wav` (+ optional `*.sound.jsonc` sidecar: format, rate, loop) | Decode → resample to 48 kHz, or the sidecar's rate → 16 bits → the sound file, or QOA below `audio/music/` or by the sidecar (below, "Sounds") | `audio/<path>.kpcm` (below, "Sound file") or `audio/<path>.qoa` |
| `*.fnt` + pages (BMFont text) | Parse, check the pages, pack each glyph into the atlas group (below, "Font file") | `atlas/<font>.font.kbj` + the sprite's frames in `atlas/<group>.kbj` |
| `data/**/*.jsonc` data (prefabs, items, input defaults) | Validate syntax → KBJ; each `data/prefabs/<name>.jsonc` read as a prefab file too (below, "Prefab file"); what the values mean is checked at game load, by reflection | `data/<path>.kbj` |

A file no pipeline cooks yet is listed once a run, with why, and left
alone. So are, rather than failing the cook, such files' names that are
not asset paths, differ from another only in case, are not UTF-8, or are
symbolic links. Names starting with `.` are left out.

### Sources and names

- What the cook reads, by name: `*.png` images, `*.sprite.jsonc`
  sidecars, folders' `atlas.jsonc`, every `*.json` as an Aseprite export,
  `*.fnt` fonts, `*.ldtk` LDtk projects and `*.ldtkl` their level files
  (never read as exports), every `*.wav` below `audio/` as a sound, and its
  `*.sound.jsonc` sidecar, and every `*.jsonc` below `data/` as data
  (sidecars and `atlas.jsonc` there are data too). Any other file is
  listed, not read: a WAV or a sound's sidecar outside `audio/`, as "a
  sound is cooked only below `audio/`".
- A file the cook reads must have
  an asset path (`AssetPath`: lowercased, then checked) and a name that is
  UTF-8, and not be a symbolic link. No other name in a folder above or
  beside it may equal its own once folded: each character uppercased, then
  lowercased, so that `Σ`, `σ` and `ς` are one, as Windows and macOS see
  them. Each is an error naming the file.
- An image's, an export's or a font's folder names its **atlas group**:
  `sprites/hero.png` is in group `sprites`. One directly in the source
  folder is an error.
- An export's sheet (`meta.image`) and a font's pages are matched as asset
  paths beside it, so their case does not matter. Each must be a PNG the
  cook found, with no sidecar of its own, taken by one export or font
  only; it is then not a sprite of its own. A sprite is named by its
  file's path without its extension: an image, an export and a font of
  one name are an error naming both.
- A folder's `atlas.jsonc` can put its images, not its subfolders', in
  another group: `{ "group": "characters" }`. The name must be lowercase
  and make asset paths. Several folders may share a group.
- Two groups' atlas files or tile sets' pages, or the folders they are
  in, that fold alike are an error.
- A group's sprites, its tile sets' pages, and the packing's ties go by
  sprite name, never by the order or the case the files were found in.

### Sidecar `<stem>.sprite.jsonc`

```jsonc
{
  "frames": [12, 20], // a sheet: cells of 12 × 20, row by row; they must divide the image
  "pivot": [6, 20],   // in a frame's untrimmed pixels (default: its bottom centre)
  "trim": false       // keep each frame whole (default: true)
}
{ "tiles": [16, 16] } // a tile set instead: tiles that divide the image; its own page, as it is
```

Sizes are whole numbers of 1 or more. Unknown keys, wrong types, `frames`
with `tiles`, and `trim` or `pivot` with `tiles`, are errors. A sheet has at
most `max_entries` frames. Source PNGs decode with any width or height
PNG allows: `max_decoded_bytes` bounds them.

A sidecar of an Aseprite export's name (`person.sprite.jsonc` beside
`person.json`) goes with the export, and gives `pivot` and `trim` only:
the frames are the export's. On an export, `"trim": false` keeps the
frames as Aseprite cut them, trimmed by Aseprite if it was asked to. A
font takes no sidecar.

### Atlas groups

- **Frames:** an image is one frame, or a sheet's cells row by row. An
  Aseprite export's are its frames, each its `frame` rectangle of the
  sheet placed at `spriteSourceSize` in a cell of `sourceSize`. A font's
  are its glyphs with a pixel above alpha 0, by id, then BMFont's fallback
  glyph, each its rectangle on its page. Each is trimmed to the box of its
  pixels whose alpha is above 0, unless its sidecar says `"trim": false`.
  A frame with no such pixel is empty: it takes no space.
- **Packing:**
  - MaxRects with the best short-side fit (ties: the shortest long side,
    then the topmost, then the leftmost); of two equal free rectangles
    one is kept. Frames are never rotated.
  - Frames go in by height, then width (the largest first), then name and
    index.
  - 2 px padding between frames, and a 1 px edge extrusion around each
    (corners too), which prevents bleeding under filtering and sub-pixel
    camera offsets.
  - Pages are squares of 64, 128, … 4096. If one page holds every frame,
    the smallest that does is used. Otherwise each frame goes into the
    first 4096 page it fits, opening pages as needed, and each page is
    then packed again into the smallest side that holds its frames.
  - A page is a bin 2 px wider than its side, so that a frame's padding
    may fall off its edge: a frame fits if its width and height, with
    their extrusion, are at most 4096. A larger one is an error naming it.
  - A group with frames to pack, but all of them empty, gets one clear
    page of 64.
- **Tile sets:** a page of their own, the image as it is (its own size, at
  most 4096 × 4096), at `atlas/<sprite>.tiles.qoi`, so that its name does
  not move when the group's other pages change. They come after the packed
  pages, by name.
- **Pages** are QOI, RGBA, sRGB. Pixels no frame covers are (0, 0, 0, 0).
- **PNG warning:** once per file that has a color chunk other than `sRGB`
  (`gAMA`, `cHRM`, `iCCP`, `cICP`, `mDCV`, `cLLI`): the cook takes every
  PNG as sRGB.

### Atlas file `atlas/<group>.kbj` (version 2)

KBJ, its members in this order:

```
{ "version": 2,
  "pages": [ { "path": "atlas/sprites_0.qoi", "w": 128, "h": 128 }, … ],   // packed pages, then tile sets'
  "sprites": [                                                         // by name
    { "name": "sprites/person",          // the image's, export's or font's path without its extension
      "frames": [ { "page": 0, "x": 33, "y": 85, "w": 10, "h": 20,      // its pixels in the page
                    "ox": 1, "oy": 0, "sw": 12, "sh": 20 }, … ],      // their offset in the untrimmed frame, its size
      "pivot": [6.0, 20.0],                                            // floats
      "clips": [ { "name": "stand", "loop": true, "frames": [[0, 100]] },          // an export's tags, by name
                 { "name": "walk", "loop": true,
                   "frames": [[1, 100], [2, 100], [3, 100], [4, 100]] } ] },     // [frame, ms]
    { "name": "sprites/tiles",
      "frames": [ { "page": 1, "x": 0, "y": 0, "w": 128, "h": 64, "ox": 0, "oy": 0, "sw": 128, "sh": 64 } ],
      "tiles": [16, 16] } ] }                                          // a tile set: no pivot
```

- A frame's `uv` is its `x, y, w, h` over its page's `w, h`.
- The game reads it with `nadir_codec::atlas::parse` (above, "Atlas file
  reader"), through render's `Atlas` asset (`docs/subsystems/render.md`),
  and the cook reads each one it writes the same way, so that it never
  writes one that reader refuses: a sidecar's pivot past what an `f32`
  holds fails the cook, naming the atlas file. The reader does not check
  that page paths are asset paths, as the game also does: a source path
  near an asset path's 1,024 bytes could still cook to a page the game
  cannot load. The version is the reader's
  `atlas::VERSION`.
- The pivot is in the untrimmed frame's pixels, from (0, 0) at its top-left
  corner to (`sw`, `sh`) at its bottom-right; in the trimmed quad it is
  ((px − ox) / w, (py − oy) / h).
- An empty frame is page 0 at (0, 0), of size 0, with offset 0 and its
  cell's `sw`, `sh`.
- A tile set has one frame: its whole page.
- `clips` is written only for an Aseprite export with tags, after the
  pivot: each tag's clip (above, "Aseprite subset"), each step a frame of
  the sprite and its Aseprite duration. The facade names them
  `<sprite>/<clip>`, as `sprites/person/walk` (`Clips::add_atlas`), and
  each frame's quad is its own trimmed size, with the pivot where the cell
  has it (`ClipFrame::from_atlas`).
- A font's sprite has the pivot (0, 0): its font file places the glyphs.

### Font file `atlas/<font>.font.kbj` (version 1)

Written by the font's group's job, beside the atlas that holds its glyphs:
`fonts/pixel.fnt` cooks to `atlas/fonts/pixel.font.kbj`. KBJ, its members
in this order:

```
{ "version": 1, "face": "Pixel", "size": 16, "line_height": 18, "base": 14,   // BMFont's info and common
  "atlas": "atlas/fonts.kbj", "sprite": "fonts/pixel",                       // where the glyphs' frames are
  "glyphs": [ { "id": 32, "x": 0, "y": 13, "advance": 4 },                    // by id; no pixels: no frame
              { "id": 65, "frame": 0, "x": 0, "y": 2, "advance": 7 }, … ],   // frame: of the sprite
  "kerning": [ [65, 86, -1], … ],                                             // first, second, amount; sorted
  "fallback": { "frame": 9, "x": 0, "y": 2, "advance": 7 } }                 // what a missing character draws
```

- `x`, `y` are BMFont's offsets of the glyph's rectangle, untrimmed: its
  frame's `ox`, `oy` place the trimmed pixels in it. `advance` is BMFont's
  `xadvance`. Ids are code points.
- A glyph with no pixel above alpha 0 in its rectangle, a space or one of
  size 0, has no frame.
- The fallback is BMFont's glyph of id −1, else `?`; it must have pixels,
  or the cook fails naming the font.
- The cook checks the pages: each a PNG of the font's `scaleW × scaleH`,
  with the glyphs in its alpha (`alphaChnl` 0 or 2, or not given, as
  Hiero writes), and some pixel clear (a page with no alpha channel has
  none). Each error names the `.fnt`.
- The cook reads back each font file it writes with the game's reader
  (above, "Font file reader"), as it does the atlas file, and refuses one
  the reader would (M7.7); render's `Font` asset reads it (M7.1).

### Sounds `audio/<path>.kpcm` and `audio/<path>.qoa`

As built in M6.8 (ADR-0049). Each `.wav` below the source's `audio/` is a
job of its own, `audio/<path without .wav>`, whose inputs are the WAV and
its sidecar, `<stem>.sound.jsonc` beside it, if it has one. Its output has
its path, with the extension of how it is stored: `audio/sfx/step.wav`
cooks to `audio/sfx/step.kpcm`, and `audio/music/field.wav` to
`audio/music/field.qoa`, as a level's `music` names it. The cook owns
`OUT/audio/`.

```jsonc
{
  "format": "qoa",  // "pcm16", the sound file, or "qoa" (default: "qoa" below audio/music/, else "pcm16")
  "rate": 22050,    // the rate to cook it to, 8000 to 384000 (default: 48000, the engine's output's)
  "loop": true      // it loops: resampled as a loop (default: true below audio/music/, else false)
}
```

- **What is cooked:** the WAV's samples (16 or 24 bits, or float of 32;
  mono or stereo; above, "WAV subset"), within 512 MiB as `f32`; then, if
  its rate is not the one to cook to, resampled (above, "Resampler"), as a
  loop where it loops; then each sample made 16 bits (`pcm::to_i16`, so a
  16-bit WAV at 48 kHz cooks to the sound file sample for sample); then
  encoded, and read back with the codec's reader before it is written, as
  the game reads it: a sound, PCM16 or QOA, whole, within half the game's
  limits, as nadir-audio's `Sound` does, which holds its samples as `f32`
  (M6.9, ADR-0050); music, QOA below `audio/music/`, a frame at a time, as
  the game's `Music` plays it. A PCM16 sound below `audio/music/` is a
  sound: `Music` reads only QOA.
- **Why by folder, not by length:** the output's name is its path, which
  code and levels name; one that changed with the sound's length would
  move under them. A sound of 10 s or more kept as PCM16 by default gets a
  warning instead, which suggests QOA, about a fifth the size.
- **Refused, naming the WAV:** a WAV the reader refuses
  (`CookError::Decode`, an 8-bit one among them, as jsfxr writes unless it
  is told otherwise); one of no samples; one of a rate above 384,000 Hz,
  the game's, whatever the sidecar says; one that resampled would have more
  than `u32::MAX` frames, or take more than 512 MiB as `f32`; a sound, not
  music, whose samples would take the game more than its 256 MiB as `f32`,
  2²⁶ of them, about 11.6 minutes of 48 kHz stereo (`CookError::Config`,
  each checked once the WAV is read, before the resample or the encoder
  allocates); and a sound at another rate than 48 kHz that would take more
  than that once the game's `Sound` converts it to 48 kHz as it loads,
  which it does if the sound's last 2 ms, and 36 frames at least, are
  within −48 dBFS (`CookError::Config`, checked on the samples read back,
  as the game decodes them). These two suggest shortening the sound or
  making it music, which the game decodes as it plays. The cook restates
  the `Sound`'s rule, as it may not depend on nadir-audio. A WAV whose
  name without `.wav` ends in a dot or a space, or whose output's path,
  with `.kpcm` for `.wav`, would be longer than 1024 bytes, is
  `CookError::BadPath`, and a sidecar with no WAV of its name beside it is
  `CookError::Config`, each before anything is written.
- **Warnings:** samples past full scale, which the 16 bits clip, after a
  resample or in a float WAV; a sound that does not loop and whose own last
  frame, the WAV's, is above −48 dBFS, which clicks when it stops (the WAV's,
  as a resample rings past it; the mixer fades a sound in, not at its end:
  ADR-0046); and the long PCM16 sound, above. Nothing changes a sound's
  last samples: a fade would break a loop's seam.

### Sound file `audio/<path>.kpcm` (version 1)

```text
header (16 bytes):
  "KPCM" | version u16 (1) | channels u8 (1 or 2) | reserved u8 (0) |
  sample_rate u32 (1 to 384000) | frames u32 (1 or more)
samples: frames × channels i16, interleaved (left, then right)
```

Little-endian, and exactly `16 + frames × channels × 2` bytes: nothing
follows the samples (above, "Sound file reader and writer"). A sound plays
whole, and loops whole when the game asks: version 1 holds no loop points,
and adding them would be version 2, and a change of `kpcm::Header`.

### Data `data/<path>.kbj`

Each `.jsonc` below the source's `data/` is a job of its own,
`data/<path without .jsonc>`: read as JSONC (UTF-8, duplicate keys
refused, `CodecError::Json` naming the line and column), written as the
same value in KBJ at `data/<path>.kbj`. What the value means is checked
where the game loads it.

### LDtk levels

Each `.ldtk` the cook finds is a job, `ldtk/<its path without .ldtk>`, that
writes each of its levels as a level file (below, "Level file"), by
ADR-0038's conventions. A project whose name without `.ldtk` ends in a dot
or a space, which would make a job's name the cache cannot keep, is an
error. As built in M5.12:

- **Names:** a level is `levels/<its identifier in snake_case>.klvl`
  (`Field` → `levels/field.klvl`, `CaveEntry` → `levels/cave_entry.klvl`):
  an uppercase letter starts a word after a lowercase letter or a digit,
  or before a lowercase letter after an uppercase one. The name must be
  lowercase letters, digits and `_`, and name a file; two levels of one
  name, in one project or two, are an error naming both.
- **Paths** that LDtk writes, a tile set's `relPath` and a level's
  `externalRelPath`, are relative to the project's folder, matched as asset
  paths, and may use `..` only to stay inside the source folder.
- **Level files:** a level saved apart is read from the `.ldtkl` its
  `externalRelPath` names, which must be one the cook found, of that level
  (its `iid`), and saved by no other level. An `.ldtkl` that no project
  saves a level in is an error. A mistake in it is named in it.
- **Tile sets:** those of the layers the cook draws. Each must have an
  image of the project's (not LDtk's own icons), a PNG the cook found and
  cooks as a tile set (a sidecar of `{"tiles": [n, n]}`, `n` its
  `tileGridSize`), of the size the project gives it, with no spacing or
  padding, and at most 65535 tiles. Its atlas is its image's group's, and
  its sprite its image's path without `.png`. Its tiles' `customData` is
  `{"frames": N, "ms": M}`, an animation of N tiles from that one (N of 2
  or more, inside the set, M of 1 to 65535, two animations apart), or
  `{"tall": [C, R]}`, a block of C × R tiles from that one, inside the
  set; any other custom data is an error.
- **Tile layers:** a layer that is shown and has tiles (`gridTiles`, or
  the `autoLayerTiles` of an IntGrid or an auto-layer) is drawn. Layers are
  taken from the bottom of LDtk's list up, and each one with a tile that is
  not tall is a layer of the level file, of sort layer 0, its depth its
  place among them from the bottom (0, 1, …), its origin its offset. A
  drawn layer's opacity is 1, its cells are its tiles' size, and each of
  its tiles is opaque, on a corner of one of its cells, and alone in its
  cell.
- **Tall tiles:** a tile whose custom data is `{"tall": [C, R]}`, the
  top-left tile of its block, is a tall sprite of sort layer 1, not
  flipped, its feet the bottom middle of the block whose bottom-left cell
  is the tile's, with its layer's offset, in pixels: a block an odd number
  of pixels wide is an error. The block's other tiles, placed in a cell,
  are cells as any tile is. Tall sprites go row by row in each layer,
  layer by layer from the bottom.
- **The grid:** the layer `Collision`, an IntGrid with no offset, one a
  level: each cell that is not 0 is solid, in the collision layer
  `"World"`. A level without one has no grid.
- **Entities:** each entity, layer by layer from the bottom, in LDtk's
  order, spawns the prefab of its identifier in snake_case (`BigRock` →
  `big_rock`), which must be a `data/prefabs/<name>.jsonc` the cook found;
  its position is its pivot's, `px`, with its layer's offset. Its fields
  named `Component__field`, with more `__` for nested fields, are its
  overrides, `{"Component": {"field": value}}`, components and fields in
  name order. Int, Float, Bool, String, Multilines, FilePath and enum
  fields are written as their values, and one with no value (`null`) is
  left out; any other type, and a field named otherwise, are errors even
  with no value, as are two fields of which one overrides what the other
  does (`Health__max` and `Health__max__x`).
- **Refused too:** a project that saves backups, a simplified export,
  images of its layers or levels, or a Tiled export beside itself, which
  the cook would find among the sources; and a level whose tile layers and
  grid hold more than 2^23 cells together, before any is made, as the
  reader bounds each layer but not their sum. `worldX` and `worldY` are
  not used; a level's own fields are not cooked, with a warning.
- **Errors** name the file, as found, and the member:
  ``levels/field.ldtk`: at `levels[0].layerInstances[3].gridTiles[5].a`:
  a tile's opacity must be 1: the cook draws every tile opaque``. What the
  LDtk reader refuses is its error (above, "LDtk subset"), with the file.
- **Its files** are the project, its level files, the image, sidecar and
  folder's `atlas.jsonc` of each tile set it draws, and the prefab of each
  name its entities spawn, so that an edit to any, or a prefab that goes,
  cooks it again. Each image is read, and decoded, once however many tile
  sets take it.
- **`Counts::Levels`**, a project's levels and their entities, is what the
  job reports; `Counts` became `#[non_exhaustive]` for it in M5.12, a
  break (ADR-0038).

### Level file `levels/<name>.klvl` (version 1)

KBJ, its members in this order (ADR-0038):

```
{ "version": 1,
  "size": [1024, 768],                       // pixels, 1 to 2^24 a side
  "background": [64, 70, 91],                // red, green, blue: 0 to 255
  "assets": ["atlas/sprites.kbj"],           // what it uses, in byte order, no two the same
  "tilesets": [ { "atlas": "atlas/sprites.kbj", "sprite": "sprites/tiles",   // a tile set of an atlas among the assets
                  "columns": 8, "rows": 4,                                   // 1 or more, at most 65535 tiles
                  "animations": [ { "tile": 8, "frames": 4, "ms": 250 } ] } ],  // by first tile, apart; 2+ frames, 1+ ms
  "layers": [ { "tileset": 0, "size": [64, 48], "cell": 16, "origin": [0, 0],  // cells; a cell's pixels, 1 to 4096
                "layer": 0, "depth": 0,                                      // sort layer 0 to 255; a fixed depth
                "cells": [4, 4, 9, …] } ],          // row by row: 0 none, else 1 + tile + 65536 × flip (1 x, 2 y)
  "grid": { "size": [64, 48], "cell": 16, "origin": [0, 0], "layer": "World",   // optional
            "solid": [1, 1, 0, …] },                                          // row by row: 0 or 1
  "tall": [ { "tileset": 0, "tile": 4, "size": [1, 2], "feet": [104, 336], "layer": 1 } ],  // a block from its top-left tile
  "entities": [ { "prefab": "tree", "position": [392, 112],
                  "overrides": { "Health": { "max": 3 } } } ],              // component names; no null
  "music": "audio/field.qoa", "ambience": "audio/wind.qoa",                 // optional asset paths
  "camera": [0, 0, 1024, 768] }                                             // optional bounds: x, y, w, h
```

- Layers are drawn in their order, the first beneath; a cell that starts
  one of its set's animations is animated (`render::Tile::animated`).
- A tall tile is a sprite of the block (`TileSet::block_uv`), its feet the
  bottom middle of the block.
- Positions are whole pixels that fit an `i32`.
- The game reads it with `nadir_codec::level::parse` (above, "Level file
  reader"), through nadir-scene's `Level` asset, and the facade's
  `load_level` loads it into the world (`docs/subsystems/scene.md`,
  "Levels"); the cook writes it (above, "LDtk levels"), and reads each it
  writes the same way, so that it never writes one that reader refuses.
  The version is the reader's `level::VERSION`.

### Prefab file `data/prefabs/<name>.kbj`

A prefab's source, `assets/data/prefabs/<name>.jsonc`, cooks to KBJ as any
data file does (above, "Data"):

```
{ "name": "slime",                 // lowercase letters, digits and `_`: its file's stem
  "extends": "enemy_base",         // optional: another prefab
  "components": { "Health": { "current": 12, "max": 12 } } }   // type names; no null
```

The cook reads each prefab directly in `data/prefabs/` with
`nadir_codec::prefab::read` before it writes it, and checks that its name
is its file's stem; a `.jsonc` in a folder below is data, not a prefab. The
game reads it with `nadir_codec::prefab::parse` (above, "Prefab file
reader"); what the values mean, and the chain of bases, nadir-scene checks
(M5.11).

### Incremental cache `OUT/.cache.kbj` (version 1)

```
{ "version": 1, "cooker": "nadir-cook 0.3.0 <fnv1a64 of its executable>", "options": "",
  "jobs": [ { "job": "atlas/sprites",                        // by name
              "inputs": [["sprites/person.png", "<fnv1a64, 16 hex digits>"], …],  // its images, sidecars, atlas.jsonc
              "outputs": [["atlas/sprites.kbj", "…"], …] } ] }                    // by path
```

- Each atlas group is a job, and so is each sound, each data file and each
  LDtk project. A job is skipped
  when its files' hashes, the cooker and the options are the cache's, and
  its outputs are there as written; `--force` skips none. A cook built
  anew cooks everything once. The options string holds the options that
  change output: none yet.
- An atlas job's files are every file it read: its images, its exports
  and their sheets, its fonts and their pages, the sidecars, and its
  folders' `atlas.jsonc`. A sound's are its WAV and its sidecar. An LDtk
  job's are as above, "LDtk levels".
- Each file is read once, so its hash and what is cooked come from the same
  bytes.
- A cache that cannot be read, has another version, or names a path the
  cook would not write (not an asset path as written, or an output not a
  `.qoi` or `.kbj` below `atlas/`, a `.kpcm` or `.qoa` below `audio/`, a
  `.kbj` below `data/`, or a `.klvl` below `levels/`) is not used, with a
  warning.
- Each output is written through a `.tmp` and a rename, then the pack,
  and the cache last, each only when its bytes change, so that a file
  keeps its time of change unless they do; within an atlas job, the
  pages come before the atlas file, which names them, and the fonts'
  files after it (M5.15, ADR-0042). The cook owns `OUT/atlas/`, `OUT/audio/`,
  `OUT/data/` and `OUT/levels/`: every file there that no job wrote is removed; and it
  writes `OUT/game.kpak`, above, over whatever is there. The output folder and the source folder may not hold
  each other. Two outputs of one name, or of names that fold alike, are
  an error.

### CLI

- `nadir-cook [--force] SOURCE OUT`: exit 0, 1 for a failed cook, 2 for a
  command line not understood, or with an argument that is not valid
  Unicode. It prints each job, cooked (an atlas group's sprites, pages,
  clips and fonts; a sound's encoding, length and rate, as `PCM16, 0.25 s
  at 48000 Hz`; a project's levels and their entities) or up to date,
  then the pack's files and size, read from its header, and warns on
  standard error.
- `cargo xtask cook [--force]` runs it, built in release mode, on
  `assets/` and `target/cooked/`; with `--watch`, it cooks again whenever
  they change (above, "Hot reload (dev)").

### Determinism

- inputs are sorted
- there are no timestamps
- map iteration uses ordered maps or the deterministic hasher
- the same inputs always produce byte-identical output, and so a
  byte-identical `game.kpak`. Checked by cooking into two folders,
  and an incremental cook against a clean one, in `cargo test -p
  nadir-cook`, and by `cargo xtask cook --force` against the cook before
  it.

### LDtk version

- **LDtk 1.5.3** (ADR-0038), recorded here and in the reader's
  `ldtk::VERSION`, not in `assets/README.md`, which the cook would list as
  a file it skips.
- The reader takes a `jsonVersion` of 1.5.x only; any other is an error
  that names the version to save the project with.

## Limits (runtime defaults, configurable for tools)

| Limit | Default |
| :- | :- |
| Max texture dimension (`max_texture_dimension`) | 8192 |
| Max decoded bytes per asset (`max_decoded_bytes`) | 256 MiB |
| Max JSON/KBJ nesting depth (`max_depth`) | 64 |
| Max string length | 1 MiB |
| Max array or object entries | 1,000,000 |
| Max pack entries (`max_entries`, as for an array) | 1,000,000 |

The cook raises `max_texture_dimension` for its source PNGs, and
`max_decoded_bytes` to 512 MiB for its WAVs and their resampled samples; a
job of a sound that size holds at most about 1.5 GiB at once (the WAV's
bytes, its samples, the resampled ones, and the file).

## Tests

- **Test vectors:**
  - inflate: a hand-built stored block, fixed Huffman, dynamic Huffman, and
    edge cases (empty, max distance, length 258), in `src/inflate.rs`
  - PNG: a small hand-made image per color type, bit depth, interlace mode
    and filter, committed under `crates/codec/tests/png/` with the script
    that made them
  - QOI and QOA: round-trip; QOI byte-exact against hand-computed small
    images; QOA's vectors from a separate port of the reference encoder,
    each decoded sample for sample and encoded byte for byte, and the
    round trip's SNR asserted for seven signals, in
    `crates/codec/tests/qoa/` with the script that wrote them
  - KLZ: hand-assembled streams and the encoder's own, in
    `crates/codec/tests/klz/`; round-trip on random and repetitive data;
    malformed offsets rejected
  - KBJ: hand-computed bytes for every tag and number edge; round-trip of
    random values and the JSON documents
  - WAV: 21 files from a separate RIFF writer, which libsndfile decodes to
    the same samples, in `crates/codec/tests/wav/`
  - The sound file: files assembled field by field, with their samples
    written apart, and each rule broken once, in `crates/codec/tests/kpcm/`
    with the script that wrote them
  - The resampler: tones against themselves, above the new Nyquist
    frequency, a loop's seam, stereo channels kept apart, a constant kept,
    `sin(πx)` at exact values, I₀, and the hash of two resampled signals
    pinned, in `crates/codec/src/pcm.rs`
  - BMFont: hand-written files in each generator's dialect, in
    `crates/codec/tests/bmfont/`
  - LDtk: projects and level files of LDtk 1.5.3, from its JSON schema, in
    `crates/codec/tests/ldtk/` with the script that wrote them
  - The pack file: packs assembled field by field, each rule broken once,
    in `crates/codec/tests/kpak/` with the script that wrote them
  - The font file: four files as the cook wrote them from BMFonts made for
    the purpose, in `crates/codec/tests/font/` with the script that made
    them
  - JSON: RFC 8259 examples plus the JSONC extensions; deep nesting hits the
    limit and errors, it doesn't overflow the stack
- **Fuzz targets** (fuzz smoke): every decoder above, the atlas file reader
  (`atlas`, M5.9), the Aseprite reader (`aseprite`, M5.5), the LDtk reader
  (`ldtk`), the level file reader (`level`) and the prefab file reader
  (`prefab`, all three M5.10), the pack file reader (`kpak`, M5.13), the
  sound file reader (`kpcm`, M6.8), the font file reader (`font`, M7.7)
  and, from M9.4, the save reader.
- **Integration:** cook the sandbox assets twice and compare the hashes;
  read every cooked file through `PackSource` and `LooseSource`, and find
  the same bytes (`tools/cook/tests/cook/workspace.rs`, M5.14).
- **The cook** (`tools/cook`): the scan's rules on lists of names made by
  hand, so that names differing only in case are tested on every system,
  and on disk where the file system can hold both; MaxRects over random
  sizes (inside the page, apart); trimming and extrusion pixel by pixel;
  the sidecar's and the cache's errors; and on real folders, the atlas
  file, a group's pages, a cook into two folders byte for byte, an
  incremental cook against a clean one, what each change cooks again,
  stale files removed, and the workspace's `assets/` cooked to its PNGs'
  pixels, cell by cell, into a file the game's reader takes, with the
  person's clips, and its sounds, the sound files sample for sample as
  their WAVs and the tunes QOA that the game's reader takes (M6.6);
  Aseprite exports placed in their canvas, their clips,
  their sidecars and their mistakes; fonts' frames and files, their
  fallbacks and their mistakes; sounds, by their folders and sidecars,
  resampled, as loops, their warnings and their mistakes
  (`tests/cook/sounds.rs`); data files and prefabs; and edits to an
  export, a font or a page, each cooking its group again to what a clean
  cook writes. LDtk projects made by hand (`tests/cook/ldtk.rs`): each
  layer type, tall and animated tiles, entities and their fields, a level
  saved apart, two cooks byte for byte, an edit to a level file, a tile
  set's image, sidecar or group cooking the level again to what a clean
  cook writes, and each mistake ADR-0038 names refused with its file and
  member. The pack (`tests/cook/pack.rs`): every file the cook wrote, and
  no other, read through `PackSource` as `LooseSource` reads it; outputs
  of one value stored once and KLZ where smaller; a damaged entry named;
  the pack kept as it was by a cook that changes nothing; an empty pack
  for a cook of nothing; and, beside the writer, two paths of one hash
  refused. The sandbox's tests read the cooked field, `levels/field.klvl`,
  and find `games/sandbox/src/map.rs`'s map in it, cell for cell, and its
  prefabs the trees and rocks it makes by hand.
- **`PackSource`** (`crates/assets/src/source.rs`): a pack's files read
  and no others, a damaged one named, each way `open` fails, a pack cut
  short after it was opened, and four threads reading at once.
- **The asset server** (`crates/assets/tests`): loads that never wait for a
  source held shut, order, budgets and turns, panics, scopes. The load
  storm (`storm.rs`) asks for 1,000 images in one frame, and holds every
  frame's finish under 4 ms in every build, and the whole first frame, the
  1,000 calls included, only when optimized:
  `cargo test --release -p nadir-assets --test storm` (ADR-0030). One of
  five runs, 2 s apart, must keep to the limit, so that another process
  taking a core for a time slice, or a shared machine busy for seconds,
  does not fail it (ADR-0034).
