# nadir-cook

The offline asset pipeline of Nadir: the `nadir-cook` command, and the
library it is built on. It cooks the files artists make (PNG images,
Aseprite exports, BMFont fonts and JSONC data) into the few formats the
game loads: QOI atlas pages and KBJ files.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine
for top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games do not depend on it: it runs before the game,
and a game built on [nadir_engine](https://crates.io/crates/nadir_engine)
loads what it wrote. Install the command to cook a game's assets, or
depend on the library to cook from a tool of your own.

## Status

Version 0.1: the API will change before 1.0, and so may the formats it
writes.

The cook reads PNG images with their sidecars, Aseprite exports, BMFont
fonts and JSONC data, and keeps a cache so that the next cook does only
what changed. Not yet:

- LDtk levels (roadmap M5.12);
- audio (roadmap M6.6);
- a reader for the font files in the game (roadmap M7.1): the cook writes
  them, but the game does not read them yet.

## Install

```bash
cargo install nadir-cook
```

## Run

```bash
nadir-cook assets target/cooked
```

It cooks the files below the source folder, `assets`, into the output
folder, `target/cooked`, which it makes if needed. Neither folder may hold
the other.

```text
Usage: nadir-cook [--force] SOURCE OUT

Options:
  --force    Cook every job, whatever the cache in OUT says
  --help     Show this
  --version  Show the version
```

It prints each job, cooked or up to date, to standard output:

```text
cook: atlas/sprites: 5 sprites on 1 page, 2 clips
cook: data/items: cooked
cook: 2 jobs cooked, 0 up to date, into target/cooked
```

Warnings, and the files it did not cook with why, go to standard error.
It exits with 0 when the cook is done, 1 when it failed, with the error on
standard error, and 2 when the command line was not understood or an
argument is not valid Unicode.

## What it cooks

Each folder of images is an **atlas group**: `sprites/rock.png` is in the
group `sprites`. A group's sprites are packed into its pages.

| Source | What it makes |
| :- | :- |
| `*.png` | A sprite of one frame, packed into its group's pages |
| `*.png` with a `<name>.sprite.jsonc` sidecar | A sheet of frames, or a tile set, as the sidecar says |
| `*.json`, an Aseprite export, and its sheet | A sprite of its frames, with each tag a clip |
| `*.fnt`, a BMFont font in the text format, and its pages | A sprite of its glyphs, and a font file |
| `data/**/*.jsonc` | The same value, in KBJ |

For example:

```text
assets/                          target/cooked/
  sprites/                         .cache.kbj
    person.json                    atlas/
    person.png                       sprites.kbj
    rock.png                         sprites_0.qoi
    tiles.png                        sprites/tiles.tiles.qoi
    tiles.sprite.jsonc               fonts.kbj
  fonts/                             fonts_0.qoi
    pixel.fnt                        fonts/pixel.font.kbj
    pixel_0.png                    data/
  data/                              items.kbj
    items.jsonc
```

- **Pages:** `atlas/<group>_<n>.qoi`, squares of 64 to 4096 pixels. A
  tile set gets a page of its own, `atlas/<sprite>.tiles.qoi`.
- **The atlas file** `atlas/<group>.kbj` says where each sprite's frames
  went, with their pivots and an Aseprite export's clips. The game reads
  it as an atlas, and the cook reads back each one it writes the same way.
- **A sidecar** `<name>.sprite.jsonc` beside an image cuts it into frames
  and sets their pivot, or makes it a tile set:

  ```jsonc
  { "frames": [12, 20], "pivot": [6, 20], "trim": false }
  ```

  ```jsonc
  { "tiles": [16, 16] }
  ```

  Frames are trimmed to their pixels unless `"trim"` is `false`, and their
  pivot is their bottom centre unless `"pivot"` says otherwise. Beside an
  Aseprite export, a sidecar takes `trim` and `pivot` only.
- **A folder's `atlas.jsonc`** puts its images in another group:
  `{ "group": "characters" }`.
- **Aseprite exports** are written by
  `aseprite -b in.aseprite --sheet out.png --data out.json --format json-array --list-tags`.
  Every `.json` file is read as one.
- **Data** below `data/` keeps its path: `data/items.jsonc` cooks to
  `data/items.kbj`. What the value means is checked where the game loads
  it.

Any other file is listed as not cooked and left alone. Names starting
with `.` are left out. A file the cook reads must have a path that is an
asset path and a UTF-8 name, and not be a symbolic link; neither its name
nor a folder's above it may differ only in case from another name beside
it. Each is an error naming the file. Paths are lowercased, so the
outputs' are lowercase.

## The output and the cache

- Each atlas group is a job, and so is each data file. A job is left as it
  is when its files have not changed since the last cook into the same
  output folder, by their hashes, and its outputs are still there as the
  cook wrote them. `--force` cooks every job.
- The cache is `.cache.kbj` in the output folder. A cook of another
  version, or built anew, cooks everything once.
- The cook owns the output's `atlas/` and `data/`: it removes every file
  there that no job wrote.
- Each file is written to a `.tmp` beside it and then renamed, and the
  cache last, so a cook stopped halfway leaves no file half written.
- The same source files cook to the same bytes on every run and every
  system.

## Usage as a library

```toml
[dependencies]
nadir-cook = "0.1"
```

`nadir_cook::cook` does what the command does, and gives a report in
place of printing one:

```rust,no_run
use std::path::Path;

use nadir_cook::{Options, cook};

fn main() -> Result<(), nadir_cook::CookError> {
    let options = Options {
        force: false,
        // What did the cooking: a cache another cooker wrote is not used.
        cooker: concat!("my-tool ", env!("CARGO_PKG_VERSION")).to_string(),
    };
    let report = cook(Path::new("assets"), Path::new("target/cooked"), &options)?;
    for job in &report.jobs {
        let done = if job.cooked { "cooked" } else { "up to date" };
        println!("{}: {done}", job.name);
    }
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    for (path, why) in &report.skipped {
        eprintln!("not cooked: {path}: {why}");
    }
    Ok(())
}
```

## Documentation

- API reference: https://docs.rs/nadir-cook
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/assets-and-codecs.md,
  its part "`nadir-cook` (library + CLI)", with the formats it writes
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
