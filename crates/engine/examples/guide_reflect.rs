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

//! Reflected components: one that prefabs, levels, and saves can set, the
//! inspector can edit, and the world hash covers.
//!
//! An example of the instructions page, `site/instructions/index.html`,
//! which `cargo xtask site` copies into the page without these comments.

use nadir::prelude::*;
use nadir::scene::reflect_struct;

reflect_struct! {
    /// Hit points. In a prefab: `"Health": { "max": 12 }`, with
    /// `current` left at its default.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Health {
        current: i32,
        max: i32,
    }
}

impl Component for Health {}

fn main() -> Result<(), nadir::Error> {
    let mut app = App::new(Config::default()).register_component::<Health>();
    app.world_mut().spawn().with(Health {
        current: 12,
        max: 12,
    });
    app.run()
}
