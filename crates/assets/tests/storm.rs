// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A load storm: 1,000 images asked for in one frame, made over the frames
//! that follow (roadmap M5.3). In every build, no frame spends over 4 ms in
//! `finish_completed_loads`. In an optimized build, the first frame's 1,000
//! calls to `load` and its finish together keep to 4 ms as well:
//! `cargo test --release -p nadir-assets --test storm` (ADR-0030). It is a
//! test binary of its own, so that no other test runs beside it.

use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use nadir_assets::{
    Asset, AssetError, AssetServer, DecodeCtx, FinalizeCtx, Handle, LoadState, OnFailure,
};
use nadir_codec::qoi::{self, Channels, ColorSpace, Header, Image};

const ASSETS: usize = 1000;
/// Each image's width and height.
const SIDE: u32 = 64;
/// The most a frame may spend on assets on the main thread.
const STALL: Duration = Duration::from_millis(4);
/// What else a frame does, in the test's place.
const FRAME: Duration = Duration::from_millis(1);
/// How many times the storm may run: one run must keep to [`STALL`].
const ATTEMPTS: usize = 5;
/// How long the test waits before it runs the storm again.
const PAUSE: Duration = Duration::from_secs(2);

/// An image, made on the main thread from its decoded pixels, as a
/// texture is uploaded.
struct Picture {
    checksum: u64,
}

impl Asset for Picture {
    type Decoded = Image;
    const EXTENSIONS: &'static [&'static str] = &["qoi"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<Image, AssetError> {
        qoi::decode(bytes, ctx.limits()).map_err(AssetError::Decode)
    }

    fn finalize(image: Image, _: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        // A pass over the pixels, as an upload copies them.
        let checksum = image.pixels.iter().map(|&byte| u64::from(byte)).sum();
        Ok(Self { checksum })
    }
}

/// Image `index`'s pixels, different from the others'.
fn pixels(index: usize) -> Vec<u8> {
    (0..SIDE * SIDE)
        .flat_map(|i| {
            let value = (i as usize * 7 + index) as u8;
            [value, value ^ 0x55, index as u8, 255]
        })
        .collect()
}

fn checksum(pixels: &[u8]) -> u64 {
    pixels.iter().map(|&byte| u64::from(byte)).sum()
}

/// Runs the storm once over the images under `root`, and returns its
/// report: `Err` if a frame went over [`STALL`].
fn storm(root: &Path) -> Result<String, String> {
    let mut server = AssetServer::new(root, OnFailure::Nothing);
    let mut ctx = FinalizeCtx::new();
    let started = Instant::now();
    // The first frame asks for every image, then finishes what is ready, as
    // the engine's frame does after its systems.
    let handles: Vec<Handle<Picture>> = (0..ASSETS)
        .map(|index| server.load(&format!("images/{index:04}.qoi")))
        .collect();
    let asked = started.elapsed();
    let finish = |server: &mut AssetServer, ctx: &mut FinalizeCtx<'_>| {
        let frame = Instant::now();
        server.finish_completed_loads(ctx);
        frame.elapsed()
    };
    let mut finishes = vec![finish(&mut server, &mut ctx)];
    while server.has_pending() {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the storm did not finish in a minute"
        );
        thread::sleep(FRAME);
        finishes.push(finish(&mut server, &mut ctx));
    }
    let total = started.elapsed();

    for (index, &handle) in handles.iter().enumerate() {
        assert_eq!(
            server.state(handle),
            Some(LoadState::Loaded),
            "image {index}"
        );
        let picture = server.get(handle).expect("loaded");
        assert_eq!(picture.checksum, checksum(&pixels(index)), "image {index}");
    }
    let (worst, at) = finishes
        .iter()
        .enumerate()
        .map(|(index, &time)| (time, index))
        .max()
        .expect("a frame at least");
    let report = format!(
        "{} frames; the first asked for {ASSETS} in {asked:?} and finished in {:?}; \
         the slowest finish was frame {at}'s, {worst:?}; the storm took {total:?}",
        finishes.len(),
        finishes[0],
    );
    // What the server does in a frame keeps to the budget, whatever the
    // build. The game's calls to `load` are in the first frame too. They
    // read and decode nothing, but an unoptimized build spends about 3 µs
    // on each, so only an optimized build holds the whole first frame to
    // the limit.
    let first = if cfg!(debug_assertions) {
        Duration::ZERO
    } else {
        asked + finishes[0]
    };
    if worst < STALL && first < STALL {
        Ok(report)
    } else {
        Err(report)
    }
}

#[test]
fn a_storm_of_1000_loads_finishes_with_no_frame_stalled_over_4_ms() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("nadir-assets-storm-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).expect("remove an earlier run's directory");
    }
    fs::create_dir_all(root.join("images")).expect("create the test directory");
    let header = Header {
        width: SIDE,
        height: SIDE,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    for index in 0..ASSETS {
        let bytes = qoi::encode(&header, &pixels(index)).expect("an image");
        fs::write(root.join(format!("images/{index:04}.qoi")), bytes).expect("write an image");
    }

    // The main thread and the workers fill every core, so another process
    // that wants one can take the main thread's for a time slice, which is
    // longer than the margin the budget leaves. A run held that way is run
    // again after a pause, since on a shared machine a busy spell can last
    // longer than several runs back to back: one of five must keep to the
    // limit (ADR-0034). The server itself never waits in a frame
    // (`loading_never_waits_for_the_file` in server.rs).
    let mut reports = Vec::new();
    let passed = (0..ATTEMPTS).any(|attempt| {
        if attempt > 0 {
            thread::sleep(PAUSE);
        }
        match storm(&root) {
            Ok(_) => true,
            Err(report) => {
                reports.push(report);
                false
            }
        }
    });
    fs::remove_dir_all(&root).expect("remove the test directory");
    assert!(
        passed,
        "every run went over {STALL:?}:\n{}",
        reports.join("\n")
    );
}
