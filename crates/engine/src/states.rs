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

//! The game states in the main loop (ADR-0067): nadir-scene's
//! [`StateMachine`] made between ticks, the loading screen of
//! [`GameState::Loading`], and the world held still in
//! [`GameState::Paused`].

use std::time::{Duration, Instant};

use nadir_assets::AssetServer;
use nadir_core::log::{error, info};
use nadir_ecs::World;
use nadir_scene::{GameState, LevelId, StateKind, StateMachine, StateSystems};

use crate::level::failed_texture;
use crate::{Error, LevelError, load_level};

/// Where the loading of [`GameState::Loading`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Entered at `since`: the level loads once a frame's stages have run
    /// in the state (`shown`), so that the loading screen shows first.
    Entered { shown: bool, since: Instant },
    /// The level loaded: its assets load, until none is pending.
    Waiting { since: Instant },
    /// It failed: the game stays in the state, and runs no ticks.
    Failed,
}

/// The game states' side of an [`App`](crate::App): the enter and exit
/// systems, the loading, and the time held while paused.
pub(crate) struct States {
    systems: StateSystems,
    loading: Option<Step>,
    /// Whether the frames finish the asset server's loads: they are drawn,
    /// on a device. A headless run finishes none, so the loading does not
    /// wait for them.
    finishes_loads: bool,
    /// Why the level of the loading state did not load, until the loop or
    /// the offscreen frame takes it.
    failure: Option<Error>,
    time: HeldTime,
}

impl States {
    pub(crate) fn new() -> Self {
        Self {
            systems: StateSystems::new(),
            loading: None,
            finishes_loads: false,
            failure: None,
            time: HeldTime::default(),
        }
    }

    /// The enter and exit systems.
    pub(crate) fn systems(&self) -> &StateSystems {
        &self.systems
    }

    /// The enter and exit systems, to add to.
    pub(crate) fn systems_mut(&mut self) -> &mut StateSystems {
        &mut self.systems
    }

    /// Says that the frames are drawn on a device, which finishes the
    /// asset server's loads, so that the loading waits for them.
    pub(crate) fn finish_loads(&mut self) {
        self.finishes_loads = true;
    }

    /// Why the level of the loading state did not load, once.
    pub(crate) fn take_failure(&mut self) -> Option<Error> {
        self.failure.take()
    }

    /// Makes the transition queued, if one is due: between ticks only.
    /// After its exit and enter systems, a load that waits for its level is
    /// restored, if this was the level's loading ending in
    /// [`GameState::InGame`] (`saves.rs`).
    pub(crate) fn apply(&mut self, world: &mut World) {
        if let Some(transition) = self.systems.apply(world) {
            self.loading = matches!(transition.to, GameState::Loading(_)).then(|| Step::Entered {
                shown: false,
                since: Instant::now(),
            });
            crate::saves::transitioned(world, &transition);
        }
    }

    /// Before a frame's ticks: makes the transition due, then takes the
    /// loading a step on. A loading state whose screen has shown loads its
    /// level; one whose level has loaded goes on to [`GameState::InGame`]
    /// once no asset load is pending, or at once where the frames finish
    /// none.
    pub(crate) fn start_frame(&mut self, world: &mut World) {
        self.apply(world);
        let Some(step) = self.loading else {
            return;
        };
        let GameState::Loading(level) = world.resource::<StateMachine>().state().clone() else {
            self.loading = None;
            return;
        };
        let since = match step {
            Step::Entered { shown: true, since } => {
                if let Err(error) = load_level(world, level.as_str()) {
                    return self.fail(&level, error);
                }
                self.loading = Some(Step::Waiting { since });
                since
            }
            Step::Waiting { since } => since,
            Step::Entered { shown: false, .. } | Step::Failed => return,
        };
        if self.finishes_loads && world.resource::<AssetServer>().has_pending() {
            return;
        }
        // Without `dev`, a texture that failed shows nothing: the level
        // cannot be played. With it, it shows its placeholder.
        if !cfg!(feature = "dev")
            && let Some(error) = failed_texture(world)
        {
            return self.fail(&level, error);
        }
        let ms = since.elapsed().as_secs_f64() * 1000.0;
        info!("the level `{level}` is ready, {ms:.1} ms after the loading started");
        world.resource_mut::<StateMachine>().set(GameState::InGame);
        self.apply(world);
    }

    /// After the frame stages: notes that they ran in the loading state,
    /// so that its screen has shown, then makes the transition due.
    pub(crate) fn end_frame(&mut self, world: &mut World) {
        if let Some(Step::Entered { shown, .. }) = &mut self.loading {
            *shown = true;
        }
        self.apply(world);
    }

    /// The `RenderFrame::time` of a frame whose simulation has run
    /// `simulated`: the same, less the time spent in
    /// [`GameState::Paused`], which holds it.
    pub(crate) fn frame_time(&mut self, world: &World, simulated: Duration) -> Duration {
        self.time.frame(simulated, paused(world))
    }

    /// The level of the loading state did not load: logged, kept for the
    /// loop, and the state stays.
    fn fail(&mut self, level: &LevelId, cause: LevelError) {
        error!("the level `{level}` cannot be loaded, so the game stays in Loading: {cause}");
        self.loading = Some(Step::Failed);
        self.failure = Some(Error::Level {
            name: level.to_string(),
            error: Box::new(cause),
        });
    }
}

/// A frame's time, held while the game is paused.
#[derive(Clone, Copy, Debug, Default)]
struct HeldTime {
    /// The time spent paused, left out.
    left_out: Duration,
    /// The last frame's time.
    shown: Duration,
}

impl HeldTime {
    /// The time of a frame whose simulation has run `simulated`: held at the
    /// last while `held`, and going on from there after.
    fn frame(&mut self, simulated: Duration, held: bool) -> Duration {
        if held {
            self.left_out = simulated.saturating_sub(self.shown);
        } else {
            self.shown = simulated.saturating_sub(self.left_out);
        }
        self.shown
    }
}

/// Whether the ticks run: not while loading.
pub(crate) fn ticks_run(world: &World) -> bool {
    !world.resource::<StateMachine>().is(StateKind::Loading)
}

/// Whether the game is paused.
fn paused(world: &World) -> bool {
    world.resource::<StateMachine>().is(StateKind::Paused)
}

/// The run condition of the engine's systems that move the world: not
/// while paused.
pub(crate) fn not_paused(world: &World) -> bool {
    !paused(world)
}

/// The run condition of the engine's systems that draw the world: not
/// while loading, so that the loading screen hides it.
pub(crate) fn not_loading(world: &World) -> bool {
    ticks_run(world)
}

#[cfg(test)]
mod tests {
    use std::env;

    use nadir_assets::{AssetError, OnFailure, Scope};
    use nadir_core::time::Tick;
    use nadir_core::{UVec2, Vec2};
    use nadir_ecs::{FixedStage, FrameStage};
    use nadir_gfx::GfxError;
    use nadir_render::{RenderError, RenderFrame, Texture};
    use nadir_scene::Transition;

    use super::*;
    use crate::level::tests::{Memory, app as level_app, files};
    use crate::{App, Config, CurrentLevel, Offscreen, TickStats, Transform, Velocity};

    /// A frame of one tick.
    const TICK: Duration = Duration::from_nanos(16_666_667);

    /// What ran, in order, as `name: the state the game was in`.
    #[derive(Default)]
    struct Log(Vec<String>);

    fn note(world: &World, name: &str) {
        let state = world.resource::<StateMachine>().state().to_string();
        world
            .resource_mut::<Log>()
            .0
            .push(format!("{name}: {state}"));
    }

    fn log(app: &App) -> Vec<String> {
        app.world().resource::<Log>().0.clone()
    }

    fn set(app: &App, next: GameState) {
        app.world().resource_mut::<StateMachine>().set(next);
    }

    fn state(app: &App) -> GameState {
        app.world().resource::<StateMachine>().state().clone()
    }

    fn noted_tick(world: &World, tick: &Tick) {
        note(world, &format!("tick {}", tick.index));
    }

    #[test]
    fn transitions_happen_between_ticks_even_inside_a_frame_of_several() {
        let mut app = App::new(Config::default())
            .insert_resource(Log::default())
            .insert_resource(StateMachine::new(GameState::InGame))
            .add_fixed_system_in(
                StateKind::InGame,
                FixedStage::Gameplay,
                "pause",
                |world, tick| {
                    if tick.index == 1 {
                        world.resource_mut::<StateMachine>().set(GameState::Paused);
                    }
                },
            )
            .add_fixed_system_in(
                StateKind::InGame,
                FixedStage::Cleanup,
                "playing",
                |world, tick| {
                    note(world, &format!("playing {}", tick.index));
                },
            )
            .add_fixed_system_in(
                StateKind::Paused,
                FixedStage::Cleanup,
                "paused",
                |world, tick| {
                    note(world, &format!("paused {}", tick.index));
                },
            )
            .add_frame_system(FrameStage::Ui, "frame", |world, _| note(world, "frame"))
            .on_enter(StateKind::InGame, "enter_game", |world, transition| {
                assert_eq!(transition.from, None, "the start");
                note(world, "enter_game");
            })
            .on_exit(StateKind::InGame, "leave_game", |world, _| {
                note(world, "leave_game")
            })
            .on_enter(StateKind::Paused, "pause_menu", |world, _| {
                note(world, "pause_menu")
            });

        // One frame of three ticks: the game pauses in the second, which runs
        // to its end as play, and the third is paused.
        app.update(TICK * 3, &[]);
        assert_eq!(app.world().resource::<TickStats>().ticks, 3);
        assert_eq!(
            log(&app),
            [
                "enter_game: InGame",
                "playing 0: InGame",
                "playing 1: InGame",
                "leave_game: InGame",
                "pause_menu: Paused",
                "paused 2: Paused",
                "frame: Paused",
            ]
        );
    }

    #[test]
    fn a_transition_the_frame_stages_queue_comes_before_the_next_tick() {
        let mut app = App::new(Config::default())
            .insert_resource(Log::default())
            .add_fixed_system(FixedStage::Gameplay, "tick", noted_tick)
            .add_frame_system_in(StateKind::Boot, FrameStage::Ui, "menu", |world, _| {
                note(world, "menu");
                world
                    .resource_mut::<StateMachine>()
                    .set(GameState::MainMenu);
            })
            .on_enter(StateKind::MainMenu, "show_menu", |world, _| {
                note(world, "show_menu")
            });
        app.update(TICK, &[]);
        assert_eq!(
            state(&app),
            GameState::MainMenu,
            "made after the frame stages"
        );
        app.update(TICK, &[]);
        assert_eq!(
            log(&app),
            [
                "tick 0: Boot",
                "menu: Boot",
                "show_menu: MainMenu",
                "tick 1: MainMenu"
            ]
        );
    }

    #[test]
    fn paused_the_world_and_its_time_stand_still_while_the_ticks_come() {
        let mut app = App::new(Config::default())
            .insert_resource(Log::default())
            .add_fixed_system_in(StateKind::Paused, FixedStage::Gameplay, "menu", noted_tick);
        let ball = app
            .world_mut()
            .spawn()
            .with(Transform::at(Vec2::ZERO))
            .with(Velocity(Vec2::new(60.0, 0.0)))
            .id();
        let x = |app: &App| {
            app.world()
                .read::<Transform>()
                .get(ball)
                .map_or(f32::NAN, |it| it.position.x)
        };
        let time = |app: &App| app.world().resource::<RenderFrame>().time;
        for _ in 0..2 {
            app.update(TICK, &[]);
        }
        assert_eq!(x(&app), 2.0);
        let before = time(&app);

        set(&app, GameState::Paused);
        for _ in 0..3 {
            app.update(TICK, &[]);
            assert_eq!(x(&app), 2.0, "the world stands still");
            assert_eq!(time(&app), before, "and so does the frames' time");
        }
        assert_eq!(
            log(&app),
            ["tick 2: Paused", "tick 3: Paused", "tick 4: Paused"],
            "the ticks still come"
        );

        // Played again, it goes on from where it stood: a tick more.
        set(&app, GameState::InGame);
        app.update(TICK, &[]);
        assert_eq!(x(&app), 3.0);
        let step = time(&app) - before;
        assert!(
            step >= Duration::from_micros(16_666) && step <= Duration::from_micros(16_667),
            "{step:?}"
        );
    }

    #[test]
    fn loading_shows_its_screen_then_replaces_the_level_and_takes_no_tick() {
        let mut app = level_app(true)
            .insert_resource(Log::default())
            .add_fixed_system(FixedStage::Gameplay, "tick", noted_tick)
            .add_frame_system_in(StateKind::Loading, FrameStage::Ui, "screen", |world, _| {
                note(world, "screen");
            })
            .on_enter(
                StateKind::InGame,
                "play",
                |world, transition: &Transition| {
                    let from = transition.from.as_ref().map(ToString::to_string);
                    note(world, &format!("play from {}", from.unwrap_or_default()));
                },
            );
        app.load_level("meadow").expect("the meadow");
        let meadow = app.world().resource::<CurrentLevel>().entities().to_vec();
        let held = app
            .world()
            .resource_mut::<AssetServer>()
            .load_in::<Texture>("atlas/tiles_0.qoi", Scope::Level);
        app.update(TICK, &[]);

        set(&app, GameState::Loading("pond".into()));
        // The screen shows, in front of nothing: the world is not drawn, and
        // the last level is still there.
        app.update(TICK, &[]);
        assert_eq!(app.world().resource::<TickStats>().ticks, 0);
        assert!(app.world().resource::<RenderFrame>().tile_layers.is_empty());
        assert_eq!(
            app.world().resource::<CurrentLevel>().name(),
            Some("meadow")
        );

        // Then the level loads, in place of the last; with no frames to
        // finish its loads, the game plays at once, and the next tick is the
        // one after the last before the loading.
        app.update(TICK, &[]);
        assert_eq!(state(&app), GameState::InGame);
        assert_eq!(app.world().resource::<CurrentLevel>().name(), Some("pond"));
        assert!(meadow.iter().all(|&entity| !app.world().is_alive(entity)));
        assert_eq!(
            app.world().resource::<AssetServer>().state(held),
            None,
            "the last level's scope is unloaded"
        );
        assert_eq!(
            log(&app),
            [
                "tick 0: Boot",
                "screen: Loading(pond)",
                "play from Loading(pond): InGame",
                "tick 1: InGame",
            ]
        );
    }

    #[test]
    fn a_level_that_does_not_load_keeps_the_game_loading_and_ends_the_run() {
        let mut app = level_app(true)
            .insert_resource(Log::default())
            .insert_resource(StateMachine::new(GameState::Loading("nowhere".into())))
            .add_fixed_system(FixedStage::Gameplay, "tick", noted_tick);
        app.update(TICK, &[]);
        assert_eq!(app.states().take_failure(), None, "the screen shows first");
        app.update(TICK, &[]);
        let failure = app.states().take_failure().expect("the failure");
        assert_eq!(
            failure,
            Error::Level {
                name: "nowhere".to_string(),
                error: Box::new(LevelError::Load {
                    path: "levels/nowhere.klvl".to_string(),
                    error: AssetError::NotFound,
                }),
            }
        );
        for _ in 0..3 {
            app.update(TICK, &[]);
        }
        assert_eq!(state(&app), GameState::Loading("nowhere".into()));
        assert_eq!(app.states().take_failure(), None, "once");
        assert!(log(&app).is_empty(), "and no tick ran: {:?}", log(&app));
    }

    /// `app` drawn offscreen, or `None` where this system has no offscreen
    /// context yet (Windows and macOS, until M2.6 and M2.7) and
    /// `NADIR_EXPECT_GL_RENDERER` asks for none.
    fn offscreen(app: App) -> Option<Offscreen> {
        let expected = env::var("NADIR_EXPECT_GL_RENDERER").unwrap_or_default();
        match app.offscreen(UVec2::new(32, 18)) {
            Ok(offscreen) => {
                let renderer = offscreen.renderer();
                assert!(renderer.contains(&expected), "{renderer} is not {expected}");
                Some(offscreen)
            }
            Err(Error::Render(RenderError::Gfx(GfxError::NoOffscreenContext)))
                if expected.is_empty() =>
            {
                None
            }
            Err(error) => panic!("no offscreen OpenGL context: {error}"),
        }
    }

    /// An app of the level files, the meadow's ground texture among them
    /// unless `missing`, that starts by loading the meadow.
    fn loading_meadow(missing: bool) -> App {
        let mut files = files();
        if missing {
            files.remove("atlas/tiles/ground.tiles.qoi");
        }
        App::new(Config::default())
            .insert_resource(AssetServer::with_source(Memory(files), OnFailure::Nothing))
            .collision_layers(&["World"])
            .insert_resource(StateMachine::new(GameState::Loading("meadow".into())))
    }

    #[test]
    fn drawn_the_loading_waits_for_the_levels_textures() {
        let Some(mut offscreen) = offscreen(loading_meadow(false)) else {
            return;
        };
        let state = |offscreen: &Offscreen| {
            offscreen
                .app()
                .world()
                .resource::<StateMachine>()
                .state()
                .clone()
        };
        offscreen.frame(TICK, &[]).expect("the screen");
        // The level loads, and its textures are still loading as it draws.
        offscreen.frame(TICK, &[]).expect("the level loads");
        assert_eq!(state(&offscreen), GameState::Loading("meadow".into()));
        assert_eq!(
            offscreen.app().world().resource::<CurrentLevel>().name(),
            Some("meadow")
        );
        // The offscreen frame waits for them: the next plays.
        offscreen.frame(TICK, &[]).expect("in game");
        assert_eq!(state(&offscreen), GameState::InGame);
        assert_eq!(offscreen.app().world().resource::<TickStats>().ticks, 1);
    }

    #[test]
    fn drawn_a_texture_that_fails_ends_the_run_without_dev_and_shows_with_it() {
        let Some(mut offscreen) = offscreen(loading_meadow(true)) else {
            return;
        };
        offscreen.frame(TICK, &[]).expect("the screen");
        offscreen.frame(TICK, &[]).expect("the level loads");
        let third = offscreen.frame(TICK, &[]);
        let state = offscreen
            .app()
            .world()
            .resource::<StateMachine>()
            .state()
            .clone();
        if cfg!(feature = "dev") {
            // Its placeholder shows, and the game plays.
            assert!(third.is_ok(), "{third:?}");
            assert_eq!(state, GameState::InGame);
        } else {
            assert_eq!(
                third,
                Err(Error::Level {
                    name: "meadow".to_string(),
                    error: Box::new(LevelError::Load {
                        path: "atlas/tiles/ground.tiles.qoi".to_string(),
                        error: AssetError::NotFound,
                    }),
                })
            );
            assert_eq!(state, GameState::Loading("meadow".into()));
        }
    }
}
