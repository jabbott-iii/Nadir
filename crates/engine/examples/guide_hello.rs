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

//! Hello: a game of one system, which logs at its first tick.
//!
//! An example of the instructions page, `site/instructions/index.html`,
//! which `cargo xtask site` copies into the page without these comments.

use nadir::prelude::*;

fn main() -> Result<(), nadir::Error> {
    App::new(Config::default())
        .add_fixed_system(FixedStage::Gameplay, "hello", |_, tick| {
            if tick.index == 0 {
                nadir::log::info!("the first tick");
            }
        })
        .run()
}
