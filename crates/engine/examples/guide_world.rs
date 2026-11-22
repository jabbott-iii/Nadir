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

//! The world: a component, a resource, an event, and commands.
//!
//! An example of the instructions page, `site/instructions/index.html`,
//! which `cargo xtask site` copies into the page without these comments.

use nadir::prelude::*;

/// Hit points: a component of the game's own.
struct Health(u32);
impl Component for Health {}

/// The score: a resource, the world's one value of its type.
#[derive(Default)]
struct Score(u32);

/// An event: one system sends it, and others read it.
#[derive(Clone)]
struct Defeated {
    points: u32,
}

fn main() -> Result<(), nadir::Error> {
    let mut app = App::new(Config::default())
        .insert_resource(Score::default())
        .add_event::<Defeated>()
        .add_fixed_system(FixedStage::Gameplay, "wear_down", wear_down)
        .add_fixed_system(FixedStage::Cleanup, "keep_score", keep_score);
    let world = app.world_mut();
    world.register::<Health>();
    for hit_points in [30, 60, 90] {
        world.spawn().with(Health(hit_points));
    }
    app.run()
}

/// Takes a hit point from each entity every tick. One that reaches 0 is
/// despawned after the tick, and worth 10 points.
fn wear_down(world: &World, _tick: &Tick) {
    let mut healths = world.write::<Health>();
    let mut commands = world.commands();
    let mut defeated = world.resource_mut::<Events<Defeated>>();
    for (entity, health) in healths.iter_mut() {
        health.0 = health.0.saturating_sub(1);
        if health.0 == 0 {
            commands.despawn(entity);
            defeated.send(Defeated { points: 10 });
        }
    }
}

/// Adds up this tick's points.
fn keep_score(world: &World, _tick: &Tick) {
    let points: u32 = world
        .resource::<Events<Defeated>>()
        .tick_events()
        .iter()
        .map(|event| event.points)
        .sum();
    world.resource_mut::<Score>().0 += points;
}
