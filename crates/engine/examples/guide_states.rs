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

//! Game states: a main menu, a level loaded behind the loading screen, and a pause.
//!
//! An example of the instructions page, `site/instructions/index.html`,
//! which `cargo xtask site` copies into the page without these comments.

use nadir::log::info;
use nadir::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Start,
    Pause,
}

impl ActionSet for Act {
    const ALL: &'static [Self] = &[Act::Start, Act::Pause];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Act::Start => "start",
            Act::Pause => "pause",
        }
    }

    fn kind(self) -> ActionKind {
        ActionKind::Button
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut map = ActionMap::<Act>::new()?;
    map.bind(Act::Start, 0, Binding::Key(Key::Enter))?;
    map.bind(Act::Pause, 0, Binding::Key(Key::Escape))?;
    App::new(Config::default())
        .insert_resource(StateMachine::new(GameState::MainMenu))
        .add_actions(map)
        .add_fixed_system_in(StateKind::MainMenu, FixedStage::Gameplay, "menu", menu)
        .add_fixed_system_in(StateKind::InGame, FixedStage::Gameplay, "pause", pause)
        .add_fixed_system_in(StateKind::Paused, FixedStage::Gameplay, "resume", pause)
        .on_enter(StateKind::InGame, "log_start", |_, transition| {
            info!("playing, from {:?}", transition.from);
        })
        .run()?;
    Ok(())
}

/// Enter loads the level `field`, behind the loading screen, then plays it.
fn menu(world: &World, _tick: &Tick) {
    if world
        .resource::<ActionState<Act>>()
        .just_pressed(Act::Start)
    {
        let level = LevelId::new("field");
        world
            .resource_mut::<StateMachine>()
            .set(GameState::Loading(level));
    }
}

/// Escape pauses the game, and resumes it.
fn pause(world: &World, _tick: &Tick) {
    if world
        .resource::<ActionState<Act>>()
        .just_pressed(Act::Pause)
    {
        let mut states = world.resource_mut::<StateMachine>();
        let next = if states.is(StateKind::Paused) {
            GameState::InGame
        } else {
            GameState::Paused
        };
        states.set(next);
    }
}
