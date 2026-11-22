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

//! Development builds: a debug window and a console command of the game's own.
//!
//! An example of the instructions page, `site/instructions/index.html`,
//! which `cargo xtask site` copies into the page without these comments.

use nadir::prelude::*;

/// The score, which the debug window shows and the console sets.
#[derive(Default)]
struct Score(u32);

fn main() -> Result<(), nadir::Error> {
    let app = App::new(Config::default())
        .insert_resource(Score::default())
        .add_fixed_system(FixedStage::Gameplay, "score", score);
    #[cfg(feature = "dev")]
    let app = debug::add(app);
    app.run()
}

/// A point a second.
fn score(world: &World, tick: &Tick) {
    if tick.index % 60 == 59 {
        world.resource_mut::<Score>().0 += 1;
    }
}

/// The game's debug tools, in development builds only: `nadir::debug`
/// and the methods that take its windows exist only with `dev`.
#[cfg(feature = "dev")]
mod debug {
    use nadir::debug::{Args, ConsoleError, ConsoleOut, Ui};
    use nadir::prelude::*;

    use super::Score;

    pub(super) fn add(app: App) -> App {
        let first = Rect::new(Vec2::new(8.0, 8.0), Vec2::new(168.0, 98.0));
        app.add_debug_window(Key::F8, "My game", first, score_window)
            .add_console_command("score <points>", "sets the score", set_score)
    }

    /// A window of the game's own, shown and hidden by F8.
    fn score_window(world: &World, ui: &mut Ui<'_>) {
        ui.label(format_args!("score: {}", world.resource::<Score>().0));
    }

    /// `score 120`, typed in the console, sets the score to 120.
    fn set_score(
        world: &World,
        args: &Args<'_>,
        out: &mut ConsoleOut<'_>,
    ) -> Result<(), ConsoleError> {
        let text = args.get(0).unwrap_or_default();
        let points = text
            .parse()
            .map_err(|_| ConsoleError::Failed(format!("`{text}` is not a whole number")))?;
        world.resource_mut::<Score>().0 = points;
        out.print(format_args!("the score is {points}"));
        Ok(())
    }
}
