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

//! A test of a game: one frame run with no window, and where it left the player.
//!
//! An example of the instructions page, `site/instructions/index.html`,
//! which `cargo xtask site` copies into the page without these comments.

use std::time::Duration;

use nadir::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Move,
}

impl ActionSet for Act {
    const ALL: &'static [Self] = &[Act::Move];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        "move"
    }

    fn kind(self) -> ActionKind {
        ActionKind::Axis2
    }
}

fn walk(world: &World, _tick: &Tick) {
    let direction = world.resource::<ActionState<Act>>().axis2(Act::Move);
    let players = world.read::<Player>();
    let mut velocities = world.write::<Velocity>();
    for (player, _) in players.iter() {
        if let Some(velocity) = velocities.get_mut(player) {
            velocity.0 = direction * 120.0;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut map = ActionMap::<Act>::new()?;
    let wasd = Binding::KeyComposite2 {
        up: Key::W,
        down: Key::S,
        left: Key::A,
        right: Key::D,
    };
    map.bind(Act::Move, 0, wasd)?;
    let mut app = App::new(Config::default())
        .add_actions(map)
        .add_fixed_system(FixedStage::Gameplay, "walk", walk);
    let player = app
        .world_mut()
        .spawn()
        .with(Player)
        .with(Transform::at(Vec2::ZERO))
        .with(Velocity::default())
        .id();

    // D goes down, and a frame of one tick runs: 120 pixels a second is 2
    // a tick.
    let tick = Duration::from_nanos(16_666_667);
    let d = nadir::Event::Key {
        key: Key::D,
        pressed: true,
        repeat: false,
    };
    app.update(tick, &[d]);
    let at = app
        .world()
        .read::<Transform>()
        .get(player)
        .map(|it| it.position);
    assert_eq!(at, Some(Vec2::new(2.0, 0.0)));
    Ok(())
}
