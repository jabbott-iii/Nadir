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

//! Text and sound: a font and a sound loaded at setup, a jump that plays the sound, and a count drawn on the window.
//!
//! An example of the instructions page, `site/instructions/index.html`,
//! which `cargo xtask site` copies into the page without these comments.

use std::error::Error;

use nadir::assets::Scope;
use nadir::prelude::*;
use nadir::render::{Font, TextDraw};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Jump,
}

impl ActionSet for Act {
    const ALL: &'static [Self] = &[Act::Jump];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        "jump"
    }

    fn kind(self) -> ActionKind {
        ActionKind::Button
    }
}

/// What the game loaded for its interface and its sound.
struct Hud {
    font: Handle<Font>,
    jump: Handle<Sound>,
    jumps: u32,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut map = ActionMap::<Act>::new()?;
    map.bind(Act::Jump, 0, Binding::Key(Key::Space))?;
    let mut app = App::new(Config {
        assets: "cooked".into(),
        ..Config::default()
    })
    .add_actions(map)
    .add_fixed_system(FixedStage::Gameplay, "jump", jump)
    .add_frame_system(FrameStage::Ui, "hud", draw_hud);

    // A font and a sound the game needs from its first frame, loaded now.
    let hud = {
        let mut server = app.world().resource_mut::<AssetServer>();
        let mut ctx = FinalizeCtx::new();
        let font = Font::load_now_in(
            &mut server,
            "atlas/fonts/pixel.font.kbj",
            Scope::Global,
            &mut ctx,
        )?;
        let jump = server.load_now::<Sound>("audio/sfx/jump.kpcm", &mut ctx)?;
        Hud {
            font,
            jump,
            jumps: 0,
        }
    };
    app.world_mut().insert_resource(hud);
    app.run()?;
    Ok(())
}

/// Plays the jump's sound on Space, once a press.
fn jump(world: &World, _tick: &Tick) {
    if world.resource::<ActionState<Act>>().just_pressed(Act::Jump) {
        let mut hud = world.resource_mut::<Hud>();
        hud.jumps += 1;
        let assets = world.resource::<AssetServer>();
        world
            .resource::<Audio>()
            .play(&assets, hud.jump, PlayParams::default());
    }
}

/// Writes the count in the top-left corner of the window, over the world.
fn draw_hud(world: &World, _frame: &Frame) {
    let hud = world.resource::<Hud>();
    let mut frame = world.resource_mut::<RenderFrame>();
    frame.ui.write(
        TextDraw::new(hud.font, Vec2::new(8.0, 8.0)),
        format_args!("Jumps: {}", hud.jumps),
    );
}
