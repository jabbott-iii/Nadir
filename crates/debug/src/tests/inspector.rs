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

//! The inspector, driven as a person would drive it: clicks on what the
//! last frame drew, typed text and keys (`debug-tools.md`, "Tests": a
//! component edited through `Reflect` changes the world's value).
//!
//! Its window is `INSPECTOR`, in a screen tall enough for every row. A
//! control's label is 2 after its control's right end, so a point 6 left of
//! a label is inside its control.

use nadir_codec::Limits;
use nadir_codec::json::{self, Value};
use nadir_core::{Color, Key, Rect, UVec2, Vec2};
use nadir_ecs::{Component, Entity, World};
use nadir_scene::{
    FieldInfo, FromValue, Reflect, ReflectContext, ReflectError, ReflectErrorKind, ReflectKind,
    TypeRegistry,
};

use super::Harness;
use crate::{Inspector, InspectorResponse};

const SCREEN: UVec2 = UVec2::new(800, 1000);
const INSPECTOR: Rect = Rect::new(Vec2::ZERO, Vec2::new(420.0, 990.0));

nadir_scene::reflect_enum! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) enum Mood {
        #[default]
        Calm,
        Angry,
        Asleep,
    }
}

nadir_scene::reflect_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Health {
        pub(crate) current: i32,
        pub(crate) max: u8,
    }
}

nadir_scene::reflect_struct! {
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct Body {
        pub(crate) speed: f32,
        pub(crate) mood: Mood,
        pub(crate) awake: bool,
        pub(crate) at: Vec2,
        pub(crate) tint: Color,
        pub(crate) health: Health,
        pub(crate) name: String,
    }
}

impl Default for Body {
    fn default() -> Self {
        Self {
            speed: 0.0,
            mood: Mood::Calm,
            awake: false,
            at: Vec2::ZERO,
            tint: Color::WHITE,
            health: Health::default(),
            name: String::new(),
        }
    }
}

nadir_scene::reflect_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Player;
}

nadir_scene::reflect_newtype! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Score(pub(crate) u32);
}

/// A value reflected whole, as the facade's sprite is: an object of its
/// parts, which applying sets one by one, keeping the last value applied.
/// It needs a color to be made.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Glow {
    color: Color,
    sizes: Vec<f32>,
    last: Option<Value>,
}

impl Reflect for Glow {
    fn type_name(&self) -> &'static str {
        "Glow"
    }

    fn kind(&self) -> ReflectKind {
        ReflectKind::Primitive
    }

    fn fields(&self) -> &'static [FieldInfo] {
        &[]
    }

    fn field(&self, _index: usize) -> Option<&dyn Reflect> {
        None
    }

    fn field_mut(&mut self, _index: usize) -> Option<&mut dyn Reflect> {
        None
    }

    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        Value::Object(vec![
            ("color".to_string(), self.color.to_value(cx)),
            ("sizes".to_string(), self.sizes.to_value(cx)),
        ])
    }

    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        let members = value
            .as_object()
            .ok_or_else(|| cx.expected("an object", value))?;
        for (name, member) in members {
            match name.as_str() {
                "color" => self.color = cx.field("color", |cx| Color::from_value(member, cx))?,
                "sizes" => self.sizes = cx.field("sizes", |cx| Vec::from_value(member, cx))?,
                _ => cx.unknown_field("Glow", name),
            }
        }
        self.last = Some(value.clone());
        Ok(())
    }
}

impl FromValue for Glow {
    const TYPE_NAME: &'static str = "Glow";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        if value.get("color").is_none() {
            return Err(cx.error(ReflectErrorKind::MissingField { field: "color" }));
        }
        let mut glow = Glow {
            color: Color::BLACK,
            sizes: Vec::new(),
            last: None,
        };
        glow.apply_value(value, cx)?;
        glow.last = None;
        Ok(glow)
    }
}

impl Component for Health {}
impl Component for Body {}
impl Component for Player {}
impl Component for Glow {}
impl Component for Score {}

/// A world whose registry has `Health`, `Body`, `Player`, `Glow` and
/// `Score`.
fn world() -> World {
    let mut registry = TypeRegistry::new();
    registry.register_component::<Health>();
    registry.register_component::<Body>();
    registry.register_component::<Player>();
    registry.register_component::<Glow>();
    registry.register_component::<Score>();
    let mut world = World::new();
    registry.register_components(&mut world);
    world.insert_resource(registry);
    world
}

/// The inspector in its window, with a world, as frames go by.
struct Bench {
    harness: Harness,
    inspector: Inspector,
    world: World,
    response: InspectorResponse,
}

impl Bench {
    fn new(world: World) -> Self {
        let mut harness = Harness::new();
        harness.size = SCREEN;
        let mut bench = Self {
            harness,
            inspector: Inspector::new(),
            world,
            response: InspectorResponse::default(),
        };
        bench.frame();
        bench
    }

    /// A frame of the inspector, which takes the input queued since the
    /// last.
    fn frame(&mut self) {
        let (inspector, world) = (&mut self.inspector, &self.world);
        let mut response = InspectorResponse::default();
        self.harness.frame(|frame| {
            frame.window("Inspector", INSPECTOR, |ui| {
                let mut cx = ReflectContext::new("the inspector");
                response = inspector.show(world, ui, &mut cx);
            });
        });
        self.response = response;
    }

    /// Where the last frame drew `text`.
    fn at(&self, text: &str) -> Vec2 {
        self.harness
            .text_at(text)
            .unwrap_or_else(|| panic!("`{text}` is not drawn: {:?}", self.harness.texts()))
    }

    fn shows(&self, text: &str) -> bool {
        self.harness.shows(text)
    }

    /// Clicks on `text`, as the last frame drew it, and draws two frames:
    /// one that takes the click, and one that shows what it did, as a
    /// widget drawn before the one clicked shows it only then. What the
    /// first answered.
    fn click(&mut self, text: &str) -> InspectorResponse {
        let at = self.at(text) + Vec2::new(1.0, 2.0);
        self.harness.click(at);
        self.frame();
        let response = self.response;
        self.frame();
        response
    }

    /// A point inside the control whose label is `label`, near its end.
    fn control(&self, label: &str) -> Vec2 {
        self.at(label) + Vec2::new(-6.0, 2.0)
    }

    /// Clicks into the field labeled `label`, then replaces its text with
    /// `text`, and taps `key`: a frame each.
    fn type_into(&mut self, label: &str, text: &str, key: Key) {
        self.type_at(self.control(label), text, key);
    }

    /// Clicks into the field at `at`, then replaces its text with `text`,
    /// and taps `key`: a frame each.
    fn type_at(&mut self, at: Vec2, text: &str, key: Key) {
        self.harness.click(at);
        self.frame();
        self.harness.input.key(Key::LeftControl, true, false);
        self.harness.tap(Key::A);
        self.harness.input.key(Key::LeftControl, false, false);
        self.harness.input.text(text);
        self.harness.tap(key);
        self.frame();
        self.frame();
    }

    /// Drags the control at `at` by `by` units right: a frame for the
    /// press, the move and the release.
    fn drag(&mut self, at: Vec2, by: f32) {
        self.harness.press(at);
        self.frame();
        self.harness.move_to(at + Vec2::new(by, 0.0));
        self.frame();
        self.harness.release(at + Vec2::new(by, 0.0));
        self.frame();
    }

    /// Applies the commands the inspector queued, then draws a frame.
    fn apply(&mut self) {
        self.world.apply_commands();
        self.frame();
    }

    fn get<T: Component + Clone>(&self, entity: Entity) -> Option<T> {
        self.world.read::<T>().get(entity).cloned()
    }
}

#[test]
fn editing_a_component_through_reflect_changes_the_world_value() {
    let mut world = world();
    let goblin = world
        .spawn()
        .with(Health {
            current: 5,
            max: 10,
        })
        .with(Body::default())
        .id();
    let mut bench = Bench::new(world);
    assert!(bench.shows("1 entities"));
    assert!(bench.shows("Click an entity to inspect it."));

    // Its row: its number and its components' names; clicked, it is
    // selected, and its components are closed nodes.
    bench.click("  0v1 Health Body");
    assert_eq!(bench.inspector.selected(), Some(goblin));
    assert!(bench.shows("> 0v1 Health Body"));
    assert!(bench.shows("Entity 0v1"));
    assert!(bench.shows("Health") && bench.shows("Body"));
    assert!(!bench.shows("current"));

    // A whole number, typed and entered.
    bench.click("Health");
    assert!(bench.shows("5") && bench.shows("10"));
    bench.type_into("current", "7", Key::Enter);
    assert_eq!(bench.get::<Health>(goblin).map(|h| h.current), Some(7));

    // A float dragged, an enum picked, a box ticked, a pair of numbers
    // dragged, a color's red slid to 0, a nested struct's field and a
    // string typed: each changes the world's value at once.
    bench.click("Body");
    bench.drag(bench.control("speed"), 20.0);
    bench.harness.click(bench.control("mood"));
    bench.frame();
    bench.click("Angry");
    bench.click("awake");
    // The pair's first control ends 86 before its label: x, by 10.
    bench.drag(bench.at("at") + Vec2::new(-126.0, 2.0), 10.0);
    let red = bench.at("R");
    bench.drag(red + Vec2::new(-115.0, 2.0), -300.0);
    bench.click("health");
    // Two `max` rows now: the nested one is the second.
    let maxes: Vec<Vec2> = bench
        .harness
        .texts()
        .into_iter()
        .filter(|(text, _)| text == "max")
        .map(|(_, at)| at)
        .collect();
    assert_eq!(maxes.len(), 2);
    bench.type_at(maxes[1] + Vec2::new(-6.0, 2.0), "9", Key::Enter);
    bench.type_into("name", "Gob", Key::Enter);

    let body = bench.get::<Body>(goblin).expect("a body");
    assert!((body.speed - 2.0).abs() < 1e-5, "{}", body.speed);
    assert_eq!(body.mood, Mood::Angry);
    assert!(body.awake);
    assert!(
        (body.at.x - 1.0).abs() < 1e-5 && body.at.y == 0.0,
        "{:?}",
        body.at
    );
    assert_eq!(body.tint, Color::rgb(0, 255, 255));
    assert_eq!(body.health.max, 9);
    assert_eq!(body.name, "Gob");

    // And the world's value reads back as the edits made it.
    let mut cx = ReflectContext::new("check");
    let value = body.to_value(&mut cx);
    assert_eq!(value.get("mood").and_then(Value::as_str), Some("Angry"));
    assert_eq!(value.get("tint").and_then(Value::as_str), Some("#00ffff"));
    assert_eq!(
        Body::from_value(&value, &mut cx).expect("a body read back"),
        body
    );
}

#[test]
fn the_list_filters_by_number_and_component_and_pages_by_16() {
    let mut world = world();
    for index in 0..20 {
        let entity = world.spawn().with(Health::default());
        if index % 5 == 0 {
            entity.with(Player);
        }
    }
    let mut bench = Bench::new(world);
    assert!(bench.shows("20 entities"));
    assert!(bench.shows("  0v1 Health Player"));
    assert!(bench.shows("  15v1 Health Player"));
    assert!(!bench.shows("  16v1 Health"), "a page is 16");
    assert!(bench.shows("1-16 of 20"));
    bench.click(">");
    assert!(bench.shows("  16v1 Health") && bench.shows("  19v1 Health"));
    assert!(!bench.shows("  15v1 Health Player"));
    bench.click(">");
    assert!(bench.shows("17-20 of 20"), "the last page stays");

    // By a component's name, ignoring case: the filter goes back to the
    // first page.
    bench.type_into("Filter", "PLAY", Key::Enter);
    assert!(bench.shows("4 of 20 entities"));
    assert!(bench.shows("  5v1 Health Player") && !bench.shows("1-4 of 4"));
    // Every word must match: part of a number, and a component.
    bench.type_into("Filter", "5v player", Key::Enter);
    assert!(bench.shows("2 of 20 entities"));
    assert!(bench.shows("  5v1 Health Player") && bench.shows("  15v1 Health Player"));
    bench.type_into("Filter", "1v player", Key::Enter);
    assert!(
        bench.shows("0 of 20 entities"),
        "1v1 and 11v1 have no Player"
    );
    bench.type_into("Filter", "glow", Key::Enter);
    assert!(bench.shows("0 of 20 entities"));
}

#[test]
fn add_remove_and_despawn_go_through_commands() {
    let mut world = world();
    let goblin = world.spawn().with(Health::default()).id();
    let mut bench = Bench::new(world);
    bench.click("  0v1 Health");
    // The combo offers the types it lacks, in the registry's order.
    assert!(bench.shows("Body"));
    bench.click("Add");
    assert!(bench.shows("Added Body."));
    assert!(
        bench.get::<Body>(goblin).is_none(),
        "until the commands apply"
    );
    bench.apply();
    assert_eq!(bench.get::<Body>(goblin), Some(Body::default()));
    assert!(bench.shows("> 0v1 Health Body"));

    // A type that cannot be made from nothing says why, and adds nothing;
    // from the data typed beside it, it is added.
    bench
        .harness
        .click(bench.at("Player") + Vec2::new(1.0, 2.0));
    bench.frame();
    bench.click("Glow");
    bench.click("Add");
    assert!(
        bench.shows("Glow is not added: the inspector: at `Glow`: `color` is missing"),
        "{:?}",
        bench.harness.texts()
    );
    bench.apply();
    assert!(bench.get::<Glow>(goblin).is_none());
    // The data's field, between the combo and the button: 80 wide, the
    // button's text 8 after its end.
    let data = bench.at("Add") + Vec2::new(-48.0, 2.0);
    bench.type_at(data, r##"{"color": "#ff0000"} // red"##, Key::Enter);
    bench.click("Add");
    assert!(bench.shows("Added Glow."));
    bench.apply();
    let glow = bench.get::<Glow>(goblin).map(|glow| glow.color);
    assert_eq!(glow, Some(Color::rgb(255, 0, 0)));

    // A newtype, made from what it wraps; data that is not JSON says so.
    assert!(bench.shows("Score"), "the last type it lacks");
    bench.type_at(data, "{", Key::Enter);
    bench.click("Add");
    assert!(
        bench
            .harness
            .texts()
            .iter()
            .any(|(text, _)| text.starts_with("Score is not added: its data: ")),
        "{:?}",
        bench.harness.texts()
    );
    bench.type_at(data, "7", Key::Enter);
    bench.click("Add");
    bench.apply();
    assert_eq!(bench.get::<Score>(goblin), Some(Score(7)));

    // Each node's "Remove" takes its component away.
    bench.click("Health");
    bench.click("Remove");
    assert!(bench.shows("Removed Health."));
    bench.apply();
    assert!(bench.get::<Health>(goblin).is_none());
    assert!(bench.shows("> 0v1 Body Glow Score"));

    bench.click("Despawn");
    assert!(bench.shows("Despawned 0v1."));
    bench.apply();
    assert!(!bench.world.is_alive(goblin));
    assert!(bench.shows("Entity 0v1 is gone: click another."));
    assert!(bench.shows("0 entities"));
}

#[test]
fn a_value_refused_or_typed_wrong_is_dropped_and_escape_drops_an_edit() {
    let mut world = world();
    let goblin = world.spawn().with(Health { current: 1, max: 2 }).id();
    let mut bench = Bench::new(world);
    bench.click("  0v1 Health");
    bench.click("Health");
    let max = |bench: &Bench| bench.get::<Health>(goblin).map(|health| health.max);

    // Past a u8: the component refuses it, and the note names the field.
    bench.type_into("max", "300", Key::Enter);
    assert_eq!(max(&bench), Some(2));
    assert!(
        bench.shows("Not set: the inspector: at `Health.max`: 300 does not fit in a `u8`"),
        "{:?}",
        bench.harness.texts()
    );
    // Not a number at all.
    bench.type_into("max", "lots", Key::Enter);
    assert!(bench.shows("Not set: `lots` is not a whole number"));
    assert_eq!(max(&bench), Some(2));
    // Escape drops what was typed; the field shows the value again.
    bench.type_into("max", "40", Key::Escape);
    assert_eq!(max(&bench), Some(2));
    bench.frame();
    assert!(bench.shows("2") && !bench.shows("40"));
    // A press elsewhere applies it.
    bench.type_into("max", "41", Key::Right);
    assert_eq!(max(&bench), Some(2), "not yet: the field has the keyboard");
    bench.click("Entity 0v1");
    assert_eq!(max(&bench), Some(41));
}

#[test]
fn a_value_reflected_whole_is_edited_by_its_parts_each_applied_alone() {
    let mut world = world();
    let lamp = world
        .spawn()
        .with(Glow {
            color: Color::rgb(255, 0, 0),
            sizes: vec![1.0, 2.0],
            last: None,
        })
        .id();
    let mut bench = Bench::new(world);
    bench.click("  0v1 Glow");
    bench.click("Glow");
    // Its color, a string that writes one, is a color edit.
    let green = bench.at("G");
    bench.drag(green + Vec2::new(-115.0, 2.0), 300.0);
    let glow = bench.get::<Glow>(lamp).expect("a glow");
    assert_eq!(glow.color, Color::rgb(255, 255, 0));
    let mut cx = ReflectContext::new("check");
    let only_color = Value::Object(vec![(
        "color".to_string(),
        Color::rgb(255, 255, 0).to_value(&mut cx),
    )]);
    assert_eq!(glow.last, Some(only_color), "the part changed, alone");

    // Two numbers in one row, the label after the second: the list is
    // applied whole.
    bench.drag(bench.control("sizes"), 10.0);
    let glow = bench.get::<Glow>(lamp).expect("a glow");
    assert_eq!(glow.sizes.len(), 2);
    assert!(
        (glow.sizes[1] - 3.0).abs() < 1e-5 && glow.sizes[0] == 1.0,
        "{:?}",
        glow.sizes
    );
}

#[test]
fn save_as_prefab_asks_for_a_name_then_the_caller_writes_its_components() {
    let mut world = world();
    let goblin = world
        .spawn()
        .with(Health { current: 3, max: 4 })
        .with(Player)
        .with(Body::default())
        .id();
    let mut bench = Bench::new(world);
    bench.click("  0v1 Health Body Player");
    assert!(!bench.click("Save as prefab").save_prefab);
    assert!(bench.shows("Name the prefab first: lowercase letters, digits and _."));
    // The name's field, in the row before the button: 80 wide, the
    // button's text 8 after its end.
    let field = bench.at("Save as prefab") + Vec2::new(-48.0, 2.0);
    bench.type_at(field, "Gob", Key::Enter);
    assert!(!bench.click("Save as prefab").save_prefab, "no capitals");

    bench.type_at(field, "goblin-2", Key::Enter);
    assert!(
        !bench.click("Save as prefab").save_prefab,
        "a dash, which the prefab reader refuses"
    );

    bench.type_at(field, "goblin_2", Key::Enter);
    assert!(bench.click("Save as prefab").save_prefab);
    assert_eq!(bench.inspector.prefab_name(), "goblin_2");
    let mut cx = ReflectContext::new("prefab");
    let text = bench
        .inspector
        .prefab(&bench.world, &["Body"], &mut cx)
        .expect("a prefab");
    assert!(
        text.starts_with("// The prefab `goblin_2`, saved by the inspector from entity 0v1.\n"),
        "{text}"
    );
    let parsed = json::parse(&text, Limits::default()).expect("JSONC");
    assert_eq!(parsed.get("name").and_then(Value::as_str), Some("goblin_2"));
    let components = parsed.get("components").expect("components");
    let names: Vec<&str> = components
        .as_object()
        .expect("an object")
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Health", "Player"],
        "the registry's order, but Body"
    );
    assert_eq!(
        components.get("Health"),
        Some(&Value::Object(vec![
            ("current".to_string(), Value::Int(3)),
            ("max".to_string(), Value::Int(4)),
        ]))
    );

    bench.inspector.set_note("Wrote it.");
    bench.frame();
    assert!(bench.shows("Wrote it."));
    bench.world.despawn(goblin);
    assert!(bench.inspector.prefab(&bench.world, &[], &mut cx).is_none());
}

nadir_scene::reflect_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Span {
        pub(crate) from: Vec2,
        pub(crate) to: Vec2,
    }
}

nadir_scene::reflect_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Wide {
        pub(crate) id: u64,
    }
}

nadir_scene::reflect_newtype! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Name(pub(crate) String);
}

impl Component for Span {}
impl Component for Wide {}
impl Component for Name {}

/// A world whose registry has the types `register` adds, and one entity of
/// `component`.
fn world_of<T: Component + FromValue>(component: T) -> (World, Entity) {
    let mut registry = TypeRegistry::new();
    registry.register_component::<T>();
    let mut world = World::new();
    registry.register_components(&mut world);
    world.insert_resource(registry);
    let entity = world.spawn().with(component).id();
    (world, entity)
}

#[test]
fn each_row_of_numbers_edits_its_own_field() {
    let (world, span) = world_of(Span {
        from: Vec2::new(1.0, 2.0),
        to: Vec2::new(3.0, 4.0),
    });
    let mut bench = Bench::new(world);
    bench.click("  0v1 Span");
    bench.click("Span");
    // The second row's first number, its control 86 before its label's
    // end: `to.x`, not `from.x`, whose number comes first in the scope.
    bench.drag(bench.at("to") + Vec2::new(-126.0, 2.0), 20.0);
    bench.drag(bench.control("from"), 10.0);
    let moved = bench.get::<Span>(span).expect("a span");
    assert_eq!(moved.from, Vec2::new(1.0, 3.0));
    assert_eq!(moved.to, Vec2::new(5.0, 4.0));
}

#[test]
fn an_edit_whose_field_is_not_drawn_for_a_frame_is_dropped() {
    let mut world = world();
    let goblin = world.spawn().with(Health { current: 1, max: 2 }).id();
    let mut bench = Bench::new(world);
    bench.click("  0v1 Health");
    bench.click("Health");
    // Typed, not entered.
    bench.harness.click(bench.control("current"));
    bench.frame();
    bench.harness.input.key(Key::LeftControl, true, false);
    bench.harness.tap(Key::A);
    bench.harness.input.key(Key::LeftControl, false, false);
    bench.harness.input.text("9");
    bench.frame();
    assert!(bench.shows("9"));
    // The window closed for a frame, while the game changed the value.
    bench.harness.frame(|_| {});
    bench
        .world
        .write::<Health>()
        .get_mut(goblin)
        .expect("health")
        .current = 8;
    bench.frame();
    bench.frame();
    assert!(bench.shows("8") && !bench.shows("9"), "the live value");
    // A click into the field and one elsewhere apply nothing.
    bench.harness.click(bench.control("current"));
    bench.frame();
    bench.click("Entity 0v1");
    assert_eq!(bench.get::<Health>(goblin).map(|h| h.current), Some(8));
}

#[test]
fn a_u64_past_an_i64_is_typed_as_a_whole_number() {
    let (world, wide) = world_of(Wide { id: u64::MAX });
    let mut bench = Bench::new(world);
    bench.click("  0v1 Wide");
    bench.click("Wide");
    assert!(bench.shows("18446744073709551615"));
    bench.type_into("id", "5", Key::Enter);
    assert_eq!(bench.get::<Wide>(wide).map(|w| w.id), Some(5));
    bench.type_into("id", "18446744073709551615", Key::Enter);
    assert_eq!(bench.get::<Wide>(wide).map(|w| w.id), Some(u64::MAX));
}

#[test]
fn a_newtype_of_a_string_that_writes_a_color_is_text() {
    let (world, name) = world_of(Name("#c0ffee".to_string()));
    let mut bench = Bench::new(world);
    bench.click("  0v1 Name");
    bench.click("Name");
    assert!(bench.shows("#c0ffee") && !bench.shows("R"), "a text field");
    bench.type_into("value", "Bob", Key::Enter);
    assert_eq!(bench.get::<Name>(name), Some(Name("Bob".to_string())));
}

#[test]
fn a_world_with_no_registry_says_so() {
    let mut bench = Bench::new(World::new());
    bench.frame();
    assert!(bench.shows("The world has no type registry."));
}
