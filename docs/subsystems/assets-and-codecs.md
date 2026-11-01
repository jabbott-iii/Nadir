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

- takes `&[u8]` plus a `Limits` struct
- returns `Result<_, CodecError>` and **never panics** on any input
- checks every length and offset against the input size before use
- checks every allocation size against `Limits` before allocating

| Module | Spec | Encode | Decode | Shipped in game? | Notes |
| :- | :- | :- | :- | :- | :- |
| `adler32` | RFC 1950 | ✓ | ✓ | no | zlib trailer: `adler32::checksum` |
| `inflate` | RFC 1950/1951 | — | ✓ | no (cook only) | Stored, fixed and dynamic Huffman blocks; output cap from `Limits`. See below |
| `png` (feature `png`) | PNG 1.2 / ISO 15948 | — | ✓ | no | See subset below |
| `qoi` | qoiformat.org spec | ✓ | ✓ | yes | Textures, golden references |
| `qoa` | qoaformat.org spec | ✓ | ✓ | yes | Music and long SFX; LMS predictor, 20-sample slices |
| `wav` | RIFF/WAVE | — | ✓ | no | PCM 16/24-bit int, 32-bit float; mono/stereo. See subset below |
| `klz` | Below | ✓ | ✓ | yes | Pack entry compression. See below |
| `json` | RFC 8259 + JSONC (`//`, `/* */`, trailing commas) | ✓ | ✓ | yes (settings, dev data) | Order-preserving objects: `Vec<(String, Value)>` |
| `kbj` | Below | ✓ | ✓ | yes | Binary form of the JSON value tree for cooked data. See below |
| `bmfont` | AngelCode BMFont text format | — | ✓ | no | `info`, `common`, `page`, `char`, `kerning` lines. See subset below |
| `atlas` | The cook's atlas file, version 1, below | — | ✓ | yes | KBJ, then checked strictly; read by render's `Atlas` asset (ADR-0033). See below |

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
interleaved. The cook tool turns them into PCM16 or QOA (M6.6).

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
  supported ones; more than two channels, or none; a sample rate of 0; RF64
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
  48 kHz. The cook raises it for music (M6.6).
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

### BMFont subset (documented, tested)

`bmfont::parse(&str, Limits)` reads a font in BMFont's text format into a
`Font`: its face, size, line height and base, its pages' size and files,
its glyphs, its kerning pairs, what each channel of the pages holds, and
its fallback glyph, if it has one. The cook tool turns it into the game's
font asset (M5.5).

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
  `scaleW × scaleH`, and have safe paths; the cook does that (M5.5).
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

### Atlas file reader (documented, tested)

`atlas::parse(bytes, limits)` reads the cook's `atlas/<group>.kbj`
(below, "Atlas file") into an `AtlasFile` of `Page`s and `SpriteEntry`s,
each sprite a `SpriteKind::Pivot` or `SpriteKind::Tiles` with its `Frame`s.
The types are `#[non_exhaustive]`: only the reader makes them. As built in
M5.9 (ADR-0033):

- **KBJ first**, with the KBJ decoder and its limits; a file that is not
  KBJ is a `CodecError::Kbj`.
- **The version first:** any whole number other than 1 is
  `Version { found }`, "not supported", whatever else the file holds, so a
  later file's new members never read as wrong.
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
  number of tiles across and down.
- **Errors** name where in the file: `CodecError::Atlas { at, kind }`, with
  `at` a member path such as `sprites[2].frames[0].w`, since a KBJ value
  carries no byte offset; empty for the file as a whole.
- **Tests:** a rule a test, from hand-written JSON text encoded to KBJ, in
  `crates/codec/src/atlas.rs`. **Fuzz target:** `atlas`, with seeds in
  `fuzz/corpus/atlas/` (the sandbox's cooked file, the least a file can be,
  a tile set, a sheet with an empty frame); what it accepts must keep the
  promises above, which the target asserts.

## `nadir-assets`

### Handles and storage

As built in M3.2 and M5.3 (ADR-0030): loading on worker threads from a
`Source`, finalizing on the main thread within a frame budget, and freeing
by scope. `reload` comes in M5.8.

```rust
pub use nadir_core::Handle;                 // { id: GenIndex, _t: PhantomData<fn() -> T> }: Copy, Eq, Hash
pub struct Assets<T> { arena: GenArena<Entry<T>>, by_path, pending: Vec<Handle<T>>, /* result channel, released */ }
enum Slot<T> { Decoding, Pending(Result<T::Decoded, AssetError>), Loaded(T), Failed { error: AssetError, placeholder: Option<T> } }
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
    pub fn has_pending(&self) -> bool;                             // loads out, or assets to free
    pub fn set_finish_budget(&mut self, budget: Duration);         // 2 ms unless set
    pub fn finish_completed_loads(&mut self, ctx: &mut FinalizeCtx<'_>); // main thread, once a frame; never waits
    pub fn finish_all_loads(&mut self, ctx: &mut FinalizeCtx<'_>);       // waits for every load: tools, tests, offscreen frames
    // M5.8: pub fn reload(&self, path: &str);                    // dev hot reload
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
  - No control character, `\`, `:` or `*?"<>|` is allowed. A path that
    fails is refused, not repaired.
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
    ADR-0024): `load_level` (M5.6) removes a level's own.
  - This is explicit, predictable, and has no hidden `Drop` cost.
- **Placeholders in dev:**
  - The server's `OnFailure::Placeholder` shows them. It is a runtime
    setting, which the facade picks under its `dev` feature. The user chose
    this on 2026-10-19.
  - A failed texture becomes a magenta/black 8×8 checker. Its state stays
    `Failed`, and `get` gives the placeholder.
  - A failed sound becomes silence.
  - Failures log with the path and the error.
- **Release:** a missing asset is a hard error at level load, with a dialog
  (M5.6). Until then, with `OnFailure::Nothing`, it is `Failed` and logged.

### Virtual file system

```rust
pub trait Source: Send + Sync {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>;      // NotFound, or Read
    fn modified(&self, path: &AssetPath) -> Option<std::time::SystemTime> { None }  // loose files tell
}
```

- **`LooseSource`** reads files under a root directory: in development,
  `target/cooked/`. It is in every build until release builds read the
  pack (M5.7, ADR-0030).
- **`PackSource`** (release, M5.7) reads from `game.kpak`.
- **Paths are normalized** as `AssetPath`s, above: lowercase, forward
  slashes, no `..`. The cook tool rejects source paths that differ only in
  case, so Windows/macOS (case-insensitive) and Linux (case-sensitive)
  behave the same.

### Pack file `.kpak` (version 1)

```
header (32 bytes):
  magic "KPAK" | version u16 | flags u16 | entry_count u32 | toc_offset u64 | toc_size u32 | toc_crc32 u32 | reserved u32
data region: entries, each 16-byte aligned, identical blobs stored once (dedup by content hash)
toc (sorted by path_hash for binary search):
  path_hash u64 (fnv1a64 of normalized path) | offset u64 | stored_size u32 | raw_size u32 | codec u8 (0 none, 1 klz) | reserved [u8;3] | crc32 u32
string table (optional, dev builds and debugging): path strings in TOC order
```

- Hash collisions between distinct paths are detected at cook time, which
  fails the build.
- At runtime, an entry's CRC is verified on first read. A mismatch is a
  corruption error naming the path, if the string table is present.

### Hot reload (dev)

- The watcher thread polls the mtimes of the loose cooked files every 250 ms.
- `cargo xtask cook --watch` re-cooks sources into `target/cooked/` when
  they change. The game reloads the cooked file. The engine never links the
  cook tool, which keeps the layering clean.
- Reload swaps the data behind an existing handle, so references stay valid.
  GPU textures are re-uploaded in `finalize`.

## `nadir-cook` (library + CLI)

As built in M5.4 (ADR-0031): the atlas pipeline, the cache, and the CLI.
The other pipelines below come with M5.5, M5.6 and M6.6.

### Pipelines

| Source | Pipeline | Output |
| :- | :- | :- |
| `*.png` (+ optional `*.sprite.jsonc` sidecar: sheet frames, pivot, trim, tile set) | Decode → cut into frames → trim → atlas pack → QOI | Atlas pages `atlas/<group>_<n>.qoi`, tile sets' pages `atlas/<sprite>.tiles.qoi`, and `atlas/<group>.kbj` (where each frame went) |
| Aseprite export (`aseprite -b in.aseprite --sheet out.png --data out.json --format json-array --list-tags`) | Parse frames and tags → animation clips → join the atlas group. Per clip: a name, loop or once, and frames of an atlas page path, a region and milliseconds; a tag's direction and repeat count expand to the frames Aseprite plays. The facade's loader turns them into `Clip`s with texture handles, in its `Clips` (ADR-0024) | Clips in `anim/<name>.kbj` |
| `*.ldtk` (LDtk project, optionally with external level files) | Tile layers → chunked tile data; IntGrid → collision grid; entities → prefab references + field overrides; tall tiles → sprite list | `levels/<name>.klvl` (KBJ-based) |
| `*.wav` | Resample policy from the sidecar; long or music → QOA, short SFX → PCM16 | `.qoa` / `.kpcm` |
| `*.fnt` + pages (BMFont text) | Parse, pack pages into the atlas | `fonts/<name>.kbj` + atlas regions |
| `*.jsonc` data (prefabs, items, input defaults) | Validate syntax → KBJ (semantic validation happens at game load via reflection) | `data/<name>.kbj` |

A file no pipeline cooks yet is listed once a run, with why, and left
alone. So are, rather than failing the cook, such files' names that are
not asset paths, differ from another only in case, are not UTF-8, or are
symbolic links. Names starting with `.` are left out.

### Sources and names

- A file the cook reads (an image, a sidecar, an `atlas.jsonc`) must have
  an asset path (`AssetPath`: lowercased, then checked) and a name that is
  UTF-8, and not be a symbolic link. No other name in a folder above or
  beside it may equal its own once folded: each character uppercased, then
  lowercased, so that `Σ`, `σ` and `ς` are one, as Windows and macOS see
  them. Each is an error naming the file.
- An image's folder names its **atlas group**: `sprites/hero.png` is in
  group `sprites`. An image directly in the source folder is an error.
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

### Atlas groups

- **Frames:** an image is one frame, or a sheet's cells row by row. Each
  is trimmed to the box of its pixels whose alpha is above 0, unless its
  sidecar says `"trim": false`. A frame with no such pixel is empty: it
  takes no space.
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

### Atlas file `atlas/<group>.kbj` (version 1)

KBJ, its members in this order:

```
{ "version": 1,
  "pages": [ { "path": "atlas/sprites_0.qoi", "w": 128, "h": 128 }, … ],   // packed pages, then tile sets'
  "sprites": [                                                         // by name
    { "name": "sprites/person",          // the image's path without .png
      "frames": [ { "page": 0, "x": 33, "y": 85, "w": 10, "h": 20,      // its pixels in the page
                    "ox": 1, "oy": 0, "sw": 12, "sh": 20 }, … ],      // their offset in the untrimmed frame, its size
      "pivot": [6.0, 20.0] },                                          // floats
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

### Incremental cache `OUT/.cache.kbj` (version 1)

```
{ "version": 1, "cooker": "nadir-cook 0.1.0 <fnv1a64 of its executable>", "options": "",
  "jobs": [ { "job": "atlas/sprites",                        // by name
              "inputs": [["sprites/person.png", "<fnv1a64, 16 hex digits>"], …],  // its images, sidecars, atlas.jsonc
              "outputs": [["atlas/sprites.kbj", "…"], …] } ] }                    // by path
```

- Each atlas group is a job. It is skipped when its files' hashes, the
  cooker and the options are the cache's, and its outputs are there as
  written; `--force` skips none. A cook built anew cooks everything once.
  The options string holds the options that change output: none yet.
- Each file is read once, so its hash and what is cooked come from the same
  bytes.
- A cache that cannot be read, has another version, or names a path the
  cook would not write (not an asset path as written, or an output not a
  `.qoi` or `.kbj` below `atlas/`) is not used, with a warning.
- Each output is written through a `.tmp` and a rename, and the cache last.
  The cook owns `OUT/atlas/`: every file there that no job wrote is
  removed. The output folder and the source folder may not hold each
  other.

### CLI

- `nadir-cook [--force] SOURCE OUT`: exit 0, 1 for a failed cook, 2 for a
  command line not understood, or with an argument that is not valid
  Unicode. It prints each job, cooked or up to date, and warns on standard
  error.
- `cargo xtask cook [--force]` runs it, built in release mode, on
  `assets/` and `target/cooked/`.

### Determinism

- inputs are sorted
- there are no timestamps
- map iteration uses ordered maps or the deterministic hasher
- the same inputs always produce byte-identical output, and so a
  byte-identical `game.kpak` (M5.7). Checked by cooking into two folders,
  and an incremental cook against a clean one, in `cargo test -p
  nadir-cook`, and by `cargo xtask cook --force` against the cook before
  it.

### LDtk version

- pin the LDtk version the team uses and record it in `assets/README.md`
- the importer checks the project's `jsonVersion`; an unknown major
  version is an error with an upgrade note

## Limits (runtime defaults, configurable for tools)

| Limit | Default |
| :- | :- |
| Max texture dimension (`max_texture_dimension`) | 8192 |
| Max decoded bytes per asset (`max_decoded_bytes`) | 256 MiB |
| Max JSON/KBJ nesting depth (`max_depth`) | 64 |
| Max string length | 1 MiB |
| Max array or object entries | 1,000,000 |
| Max pack entries | 1,000,000 |

## Tests

- **Test vectors:**
  - inflate: a hand-built stored block, fixed Huffman, dynamic Huffman, and
    edge cases (empty, max distance, length 258), in `src/inflate.rs`
  - PNG: a small hand-made image per color type, bit depth, interlace mode
    and filter, committed under `crates/codec/tests/png/` with the script
    that made them
  - QOI and QOA: round-trip; QOA SNR threshold; QOI byte-exact against
    hand-computed small images
  - KLZ: hand-assembled streams and the encoder's own, in
    `crates/codec/tests/klz/`; round-trip on random and repetitive data;
    malformed offsets rejected
  - KBJ: hand-computed bytes for every tag and number edge; round-trip of
    random values and the JSON documents
  - WAV: 21 files from a separate RIFF writer, which libsndfile decodes to
    the same samples, in `crates/codec/tests/wav/`
  - BMFont: hand-written files in each generator's dialect, in
    `crates/codec/tests/bmfont/`
  - JSON: RFC 8259 examples plus the JSONC extensions; deep nesting hits the
    limit and errors, it doesn't overflow the stack
- **Fuzz targets** (fuzz smoke): every decoder above, the atlas file reader
  (`atlas`, M5.9), the pack TOC reader, the level reader and the save
  reader.
- **Integration:** cook the sandbox assets twice and compare the hashes;
  load every cooked asset through `PackSource` and `LooseSource`.
- **The cook** (`tools/cook`): the scan's rules on lists of names made by
  hand, so that names differing only in case are tested on every system,
  and on disk where the file system can hold both; MaxRects over random
  sizes (inside the page, apart); trimming and extrusion pixel by pixel;
  the sidecar's and the cache's errors; and on real folders, the atlas
  file, a group's pages, a cook into two folders byte for byte, an
  incremental cook against a clean one, what each change cooks again,
  stale files removed, and the workspace's `assets/` cooked to its PNGs'
  pixels, cell by cell, into a file the game's reader takes.
- **The asset server** (`crates/assets/tests`): loads that never wait for a
  source held shut, order, budgets and turns, panics, scopes. The load
  storm (`storm.rs`) asks for 1,000 images in one frame, and holds every
  frame's finish under 4 ms in every build, and the whole first frame, the
  1,000 calls included, only when optimized:
  `cargo test --release -p nadir-assets --test storm` (ADR-0030). One of
  three runs must keep to the limit, so that another process taking a core
  for a time slice does not fail it.
