//! Opens a window and prints every event it reports.
//!
//! ```text
//! cargo run -p nadir-platform --example events -- [options]
//! ```
//!
//! Options:
//!
//! - `--seconds N`: close after N seconds.
//! - `--fullscreen`: start fullscreen.
//! - `--fixed`: do not let the user resize the window.
//! - `--no-vsync`: ask for a swap interval of 0.
//!
//! Keys, named by their place on a US keyboard:
//!
//! - `F`: enter or leave fullscreen.
//! - `C`: hide or show the pointer.
//! - `T`: change the title.
//! - `Escape`: quit. So does closing the window.
//!
//! While the system holds the event loop (on Windows, while the window is
//! dragged or resized), it counts modal frames, and says how many ran once
//! the loop is back.

use std::cell::Cell;
use std::error::Error;
use std::io::{self, Write};
use std::process::ExitCode;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use nadir_core::cli::Args;
use nadir_core::{Key, UVec2};
use nadir_platform::{Event, GpuSurface, Platform, WindowConfig};

/// How long to wait between looks at the event queue.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

fn main() -> ExitCode {
    match run(Args::from_env(), &mut io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // If standard error is closed too, there is nobody left to tell.
            let _ = writeln!(io::stderr(), "events: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: Args, out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let seconds: Option<f64> = args.parsed("--seconds")?;
    let config = WindowConfig {
        title: "Nadir events".to_string(),
        size: UVec2::new(640, 360),
        min_size: UVec2::new(320, 180),
        resizable: !args.flag("--fixed"),
        fullscreen: args.flag("--fullscreen"),
        vsync: !args.flag("--no-vsync"),
    };
    if let Some(extra) = args.finish()?.first() {
        return Err(format!("unexpected argument `{extra}`").into());
    }
    let deadline = seconds.map(Duration::try_from_secs_f64).transpose()?;

    let mut platform = Platform::new(&config)?;
    let size = platform.window_size();
    writeln!(
        out,
        "window: {}x{} physical pixels, scale factor {}",
        size.x,
        size.y,
        platform.scale_factor()
    )?;
    match platform.gpu_surface() {
        GpuSurface::Gl(gl) => {
            let drawable = gl.drawable_size();
            writeln!(out, "gpu: OpenGL, drawable {}x{}", drawable.x, drawable.y)?;
        }
        _ => writeln!(out, "gpu: not OpenGL")?,
    }
    writeln!(out, "keys: F fullscreen, C pointer, T title, Escape quit")?;

    let modal_frames = Rc::new(Cell::new(0u64));
    let counter = Rc::clone(&modal_frames);
    platform.set_modal_frame_callback(Box::new(move |_frame| counter.set(counter.get() + 1)));

    let start = Instant::now();
    let mut events = Vec::new();
    let mut fullscreen = config.fullscreen;
    let mut pointer_visible = true;
    let mut titles = 0;
    loop {
        platform.pump_events(&mut events);
        for &event in &events {
            writeln!(out, "{:9.3}  {event:?}", start.elapsed().as_secs_f64())?;
            match event {
                Event::CloseRequested => return Ok(()),
                Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                } => match key {
                    Key::Escape => return Ok(()),
                    Key::F => {
                        fullscreen = !fullscreen;
                        platform.set_fullscreen(fullscreen);
                    }
                    Key::C => {
                        pointer_visible = !pointer_visible;
                        platform.set_cursor_visible(pointer_visible);
                    }
                    Key::T => {
                        titles += 1;
                        platform.set_title(&format!("Nadir events — title {titles}"));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        let held = modal_frames.replace(0);
        if held > 0 {
            writeln!(
                out,
                "{:9.3}  {held} modal frames ran while the system held the loop",
                start.elapsed().as_secs_f64()
            )?;
        }
        // Show the events now, also when the output is a pipe.
        out.flush()?;
        events.clear();
        if deadline.is_some_and(|deadline| start.elapsed() >= deadline) {
            writeln!(out, "closing after {:.1} s", start.elapsed().as_secs_f64())?;
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }
}
