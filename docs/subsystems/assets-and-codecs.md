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
| `wav` | RIFF/WAVE | — | ✓ | no | PCM 16/24-bit int, 32-bit float; mono/stereo |
| `klz` | Below | ✓ | ✓ | yes | Pack entry compression |
| `json` | RFC 8259 + JSONC (`//`, `/* */`, trailing commas) | ✓ | ✓ | yes (settings, dev data) | Order-preserving objects: `Vec<(String, Value)>` |
| `kbj` | Below | ✓ | ✓ | yes | Binary form of the JSON value tree for cooked data |
| `bmfont` | AngelCode BMFont text format | — | ✓ | no | `info`, `common`, `page`, `char`, `kerning` lines |

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
  warns once for a file that names any (M5.4). Other ancillary chunks are
  ignored without a word.
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
  the vectors). It decodes each input, then a repaired copy, so that
  mutations get past the checksums: the signature restored; the first
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

### KLZ block format (our own, LZ77, byte-oriented)

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

- The final block has literals only.
- The decoder checks that `offset ≤ bytes produced so far` and that the
  output doesn't exceed the declared raw size.
- The encoder uses a greedy hash table of 4-byte sequences (2^16 entries).
- The format is deliberately LZ4-like in spirit, but it is our own
  specification. Its exact bytes are defined by this document and the test
  vectors in `crates/codec/tests/klz/`.

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

### KBJ (binary JSON value tree)

```
value := tag:u8 payload
  0x00 null | 0x01 false | 0x02 true
  0x03 i64 (zigzag LEB128) | 0x04 f64 (8 bytes LE)
  0x05 string: len:LEB128 bytes(UTF-8)
  0x06 array:  count:LEB128 value*
  0x07 object: count:LEB128 (key:string-payload value)*
file  := "KBJ1" value
```

## `nadir-assets`

### Handles and storage

As built in M3.2: loading is synchronous, from loose files under a root
directory. Workers, scopes and `Source` come in M5.3, and `reload` in M5.8.

```rust
pub use nadir_core::Handle;                 // { id: GenIndex, _t: PhantomData<fn() -> T> }: Copy, Eq, Hash
pub struct Assets<T> { arena: GenArena<Entry<T>>, by_path: FastHashMap<String, Handle<T>>, pending: Vec<Handle<T>> }
enum Slot<T> { Pending(Result<T::Decoded, AssetError>), Loaded(T), Failed { error: AssetError, placeholder: Option<T> } }
pub enum LoadState { Loading, Loaded, Failed }
pub enum OnFailure { Placeholder, Nothing }

impl AssetServer {
    pub fn new(root: impl Into<PathBuf>, on_failure: OnFailure) -> Self;
    pub fn load<T: Asset>(&mut self, path: &str) -> Handle<T>;     // dedups by path; reads and decodes now
    pub fn get<T: Asset>(&self, h: Handle<T>) -> Option<&T>;        // loaded, or a failed one's placeholder
    pub fn state<T: Asset>(&self, h: Handle<T>) -> Option<LoadState>;  // None: nothing under that handle
    pub fn error<T: Asset>(&self, h: Handle<T>) -> Option<&AssetError>;
    pub fn path<T: Asset>(&self, h: Handle<T>) -> Option<&str>;
    pub fn assets<T: Asset>(&self) -> Option<&Assets<T>>;
    pub fn has_pending(&self) -> bool;
    pub fn finish_completed_loads(&mut self, ctx: &mut FinalizeCtx<'_>); // main thread, once per frame
    // M5.8: pub fn reload(&self, path: &str);                    // dev hot reload
}

pub trait Asset: Sized + Send + 'static {
    type Decoded: Send + 'static;                                  // produced on a worker from M5.3
    const EXTENSIONS: &'static [&'static str];
    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<Self::Decoded, AssetError>;
    fn finalize(decoded: Self::Decoded, ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError>; // main thread (GPU upload)
    fn placeholder(ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError>;   // default: NoPlaceholder
}

pub struct DecodeCtx<'a> { /* path, limits */ }
pub struct FinalizeCtx<'a> { /* up to 4 `&mut dyn Any` resources, by type: no allocation */ }
```

- **Loading:**
  - `load` gives the same handle for the same path, whatever happened to
    its load. Otherwise it reads and decodes the file at once.
  - The asset is made by the next `finish_completed_loads`, and stays
    `Loading` until then.
  - A failure shows only then, and is logged with the path: a missing file
    is `NotFound`.
- **Paths:**
  - ASCII letters are lowercased.
  - The rest is checked as one byte grammar on every system: `/` between
    parts, no empty, `.` or `..` part, and no part ending in a dot or a
    space or named like a Windows device.
  - No control character, `\`, `:` or `*?"<>|` is allowed.
  - The last part must end in one of `T::EXTENSIONS`.
- **`FinalizeCtx`** holds the resources `finalize` needs, by type, such as
  render's `gfx::Device`. A missing one is `AssetError::MissingContext`.
- **`AssetError`** holds no path: the server logs each failure with its
  path. Its variants are `BadPath`, `Extension`, `NotFound`, `Read`,
  `Decode(CodecError)`, `Invalid(String)` (an asset type's own reason),
  `MissingContext` and `NoPlaceholder`.
- **Reference handling:** handles are plain IDs; there is no reference
  counting.
  - Assets live until their owning *scope* (global, level, transient) is
    unloaded (M5.3).
  - Levels declare their asset list. Unloading a level frees every asset
    scoped to it.
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
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>;
    fn modified(&self, path: &AssetPath) -> Option<std::time::SystemTime>;  // loose only
}
```

- **`LooseSource`** (dev) reads from `target/cooked/`.
- **`PackSource`** (release) reads from `game.kpak`.
- **Paths are normalized:** lowercase, forward slashes, no `..`. The cook
  tool rejects source paths that differ only in case, so Windows/macOS
  (case-insensitive) and Linux (case-sensitive) behave the same.

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

### Pipelines

| Source | Pipeline | Output |
| :- | :- | :- |
| `*.png` (+ optional `*.sprite.jsonc` sidecar: pivot, slicing) | Decode → trim → atlas pack → QOI | Atlas pages `atlas/<group>_<n>.qoi` + `atlas/<group>.kbj` (regions) |
| Aseprite export (`aseprite -b in.aseprite --sheet out.png --data out.json --format json-array --list-tags`) | Parse frames and tags → animation clips → join the atlas group. Per clip: a name, loop or once, and frames of an atlas page path, a region and milliseconds; a tag's direction and repeat count expand to the frames Aseprite plays. The facade's loader turns them into `Clip`s with texture handles, in its `Clips` (ADR-0024) | Clips in `anim/<name>.kbj` |
| `*.ldtk` (LDtk project, optionally with external level files) | Tile layers → chunked tile data; IntGrid → collision grid; entities → prefab references + field overrides; tall tiles → sprite list | `levels/<name>.klvl` (KBJ-based) |
| `*.wav` | Resample policy from the sidecar; long or music → QOA, short SFX → PCM16 | `.qoa` / `.kpcm` |
| `*.fnt` + pages (BMFont text) | Parse, pack pages into the atlas | `fonts/<name>.kbj` + atlas regions |
| `*.jsonc` data (prefabs, items, input defaults) | Validate syntax → KBJ (semantic validation happens at game load via reflection) | `data/<name>.kbj` |

- **Atlas packing:**
  - MaxRects (best short-side fit), max page 4096², 2 px padding
  - 1 px edge extrusion, which prevents bleeding under filtering and
    sub-pixel camera offsets
  - groups set by folder, with `atlas.jsonc` overrides
- **Incremental cache:** `target/cooked/.cache.kbj` maps each source path to
  (source content hash, cooker version, options hash, output hashes).
  Unchanged entries are skipped.
- **Determinism:**
  - inputs are sorted
  - there are no timestamps
  - map iteration uses the deterministic hasher
  - the same inputs always produce a byte-identical `game.kpak`, and CI
    checks this by cooking twice
- **LDtk version:**
  - pin the LDtk version the team uses and record it in `assets/README.md`
  - the importer checks the project's `jsonVersion`; an unknown major
    version is an error with an upgrade note

## Limits (runtime defaults, configurable for tools)

| Limit | Default |
| :- | :- |
| Max texture dimension (`max_texture_dimension`) | 8192 |
| Max decoded bytes per asset (`max_decoded_bytes`) | 256 MiB |
| Max JSON/KBJ nesting depth | 64 |
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
  - KLZ: round-trip on random and repetitive data; malformed offsets
    rejected
  - JSON: RFC 8259 examples plus the JSONC extensions; deep nesting hits the
    limit and errors, it doesn't overflow the stack
- **Fuzz targets** (fuzz smoke): every decoder above, the pack TOC reader,
  the level reader and the save reader.
- **Integration:** cook the sandbox assets twice and compare the hashes;
  load every cooked asset through `PackSource` and `LooseSource`.
