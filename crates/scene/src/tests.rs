//! The reflection tests of `docs/subsystems/scene.md`: the macros' field
//! lists, `Value` round trips for every built-in type, and unknown-field
//! warnings; with the errors and the registry.

use std::fmt::Debug;

use nadir_codec::Limits;
use nadir_codec::json::{self, Value};
use nadir_core::testing::check;
use nadir_core::{Color, IVec2, Pcg32, Rect, Vec2};
use nadir_ecs::{Component, World};

use crate::{
    FieldInfo, FromValue, Reflect, ReflectContext, ReflectError, ReflectErrorKind, ReflectKind,
    TypeRegistry,
};

crate::reflect_struct! {
    /// Something with a field of every shape.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Thing {
        /// First line.
        /// Second line.
        pub count: i32,
        #[doc(alias = "pace")]
        /// After another attribute.
        pub(crate) speed: f32,
        tint: Option<Color>,
        pub tags: Vec<String>,
        pub inner: Inner,
        pub area: Rect,
        pub parts: Vec<Inner>,
    }
}

impl Component for Thing {}

crate::reflect_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Inner {
        a: u8,
        b: Facing,
    }
}

crate::reflect_enum! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) enum Facing {
        #[default]
        Down,
        /// Away from the camera.
        Up,
        Left,
        Right,
    }
}

crate::reflect_newtype! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct SaveId(pub u64);
}

crate::reflect_newtype! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Wrapped(Inner);
}

crate::reflect_struct! {
    #[derive(Debug, PartialEq)]
    pub(crate) struct Enemy;
}

crate::reflect_struct! {
    #[derive(Debug, Default, PartialEq)]
    pub(crate) struct Empty {}
}

fn object(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect(),
    )
}

fn string(text: &str) -> Value {
    Value::String(text.to_string())
}

fn ints(values: &[i64]) -> Value {
    Value::Array(values.iter().map(|&value| Value::Int(value)).collect())
}

/// `T` from `value`, read from `source`.
fn read<T: FromValue>(value: &Value) -> Result<T, ReflectError> {
    T::from_value(value, &mut ReflectContext::new("test.jsonc"))
}

// ---- The macros' field lists ---------------------------------------------

#[test]
fn a_struct_lists_its_fields_in_order_with_their_types_and_docs() {
    let field = |name, type_name, doc| FieldInfo {
        name,
        type_name,
        doc,
    };
    let thing = Thing::default();
    assert_eq!(
        thing.fields(),
        [
            field("count", "i32", " First line.\n Second line.\n"),
            field("speed", "f32", " After another attribute.\n"),
            field("tint", "Option<Color>", ""),
            field("tags", "Vec<String>", ""),
            field("inner", "Inner", ""),
            field("area", "Rect", ""),
            field("parts", "Vec<Inner>", ""),
        ]
    );
    assert_eq!(
        (thing.type_name(), thing.kind()),
        ("Thing", ReflectKind::Struct)
    );
    assert_eq!(<Thing as FromValue>::TYPE_NAME, "Thing");
    // Each field is the one its info names.
    let kinds: Vec<_> = (0..7)
        .map(|index| thing.field(index).map(|field| field.type_name()))
        .collect();
    assert_eq!(
        kinds,
        [
            Some("i32"),
            Some("f32"),
            Some("Option"),
            Some("Vec"),
            Some("Inner"),
            Some("Rect"),
            Some("Vec")
        ]
    );
    assert!(thing.field(7).is_none());
}

#[test]
fn a_field_can_be_changed_through_reflection() {
    let mut thing = Thing::default();
    let mut cx = ReflectContext::new("inspector");
    let speed = thing.field_mut(1).expect("speed");
    speed
        .apply_value(&Value::Float(2.5), &mut cx)
        .expect("a number");
    let inner = thing.field_mut(4).expect("inner");
    inner
        .field_mut(1)
        .expect("b")
        .apply_value(&string("Left"), &mut cx)
        .expect("a variant");
    assert_eq!((thing.speed, thing.inner.b), (2.5, Facing::Left));
}

#[test]
fn enums_newtypes_and_tags_read_and_write_as_their_shapes_say() {
    let mut cx = ReflectContext::new("test.jsonc");
    assert_eq!(Facing::Up.to_value(&mut cx), string("Up"));
    assert_eq!(read::<Facing>(&string("Right")), Ok(Facing::Right));
    assert_eq!(
        (Facing::Up.kind(), Facing::Up.fields()),
        (ReflectKind::Enum, &[][..])
    );

    // A newtype is its value, with its own name.
    assert_eq!(SaveId(41).to_value(&mut cx), Value::Int(41));
    assert_eq!(read::<SaveId>(&Value::Int(42)), Ok(SaveId(42)));
    let wrapped = Wrapped(Inner {
        a: 3,
        b: Facing::Up,
    });
    assert_eq!(wrapped.type_name(), "Wrapped");
    assert_eq!(wrapped.kind(), ReflectKind::Struct);
    assert_eq!(wrapped.fields(), Inner::default().fields());
    assert_eq!(
        wrapped.to_value(&mut cx),
        object(&[("a", Value::Int(3)), ("b", string("Up"))])
    );

    // A tag is an empty object; so is a struct with no fields.
    assert_eq!(Enemy.to_value(&mut cx), object(&[]));
    assert_eq!(read::<Enemy>(&object(&[])), Ok(Enemy));
    assert_eq!(read::<Empty>(&object(&[])), Ok(Empty {}));
    assert!(Enemy.fields().is_empty() && Empty {}.field(0).is_none());
}

// ---- Round trips -----------------------------------------------------------

/// `value` written to JSON text, read back, and made a `T` again.
fn through_text<T: FromValue + PartialEq + Debug>(value: &T) -> Result<(), String> {
    let mut cx = ReflectContext::new("round trip");
    let text = json::to_string_compact(&value.to_value(&mut cx));
    let parsed =
        json::parse(&text, Limits::default()).map_err(|error| format!("{text}: {error}"))?;
    let back = T::from_value(&parsed, &mut cx).map_err(|error| format!("{text}: {error}"))?;
    if back == *value && cx.warnings().is_empty() {
        Ok(())
    } else {
        Err(format!("{value:?} came back as {back:?} from {text}"))
    }
}

fn round_trips<T: FromValue + PartialEq + Debug>(name: &str, generate: impl Fn(&mut Pcg32) -> T) {
    check(name, 300, generate, through_text);
}

fn bits64(rng: &mut Pcg32) -> u64 {
    (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32())
}

/// Any finite f32, from its bits: all magnitudes and both zeros.
fn any_f32(rng: &mut Pcg32) -> f32 {
    loop {
        let value = f32::from_bits(rng.next_u32());
        if value.is_finite() {
            return value;
        }
    }
}

fn any_f64(rng: &mut Pcg32) -> f64 {
    loop {
        let value = f64::from_bits(bits64(rng));
        if value.is_finite() {
            return value;
        }
    }
}

/// Any string: quotes, backslashes, control characters, and characters
/// outside the basic plane.
fn any_string(rng: &mut Pcg32) -> String {
    (0..rng.range(0..12u32))
        .filter_map(|_| match rng.range(0..4u32) {
            0 => char::from_u32(rng.range(0..0x80u32)),
            1 => Some(['"', '\\', '\n', '\u{0}', '\u{7f}'][rng.range(0..5u32) as usize]),
            _ => char::from_u32(rng.range(0..0x11_0000u32)),
        })
        .collect()
}

fn any_vec2(rng: &mut Pcg32) -> Vec2 {
    Vec2::new(any_f32(rng), any_f32(rng))
}

fn any_inner(rng: &mut Pcg32) -> Inner {
    Inner {
        a: rng.next_u32() as u8,
        b: [Facing::Down, Facing::Up, Facing::Left, Facing::Right][rng.range(0..4u32) as usize],
    }
}

#[test]
fn every_built_in_type_comes_back_from_json_text_as_it_was() {
    round_trips("bool", |rng| rng.range(0..2u32) == 1);
    round_trips("i8", |rng| rng.next_u32() as i8);
    round_trips("i16", |rng| rng.next_u32() as i16);
    round_trips("i32", |rng| rng.next_u32() as i32);
    round_trips("i64", |rng| bits64(rng) as i64);
    round_trips("isize", |rng| bits64(rng) as isize);
    round_trips("u8", |rng| rng.next_u32() as u8);
    round_trips("u16", |rng| rng.next_u32() as u16);
    round_trips("u32", |rng| rng.next_u32());
    round_trips("u64", bits64);
    round_trips("usize", |rng| bits64(rng) as usize);
    round_trips("f32", any_f32);
    round_trips("f64", any_f64);
    round_trips("String", any_string);
    round_trips("Vec2", any_vec2);
    round_trips("IVec2", |rng| {
        IVec2::new(rng.next_u32() as i32, rng.next_u32() as i32)
    });
    round_trips("Color", |rng| {
        let [r, g, b, a] = rng.next_u32().to_le_bytes();
        // Opaque colors are written without their alpha.
        Color::rgba(r, g, b, if rng.range(0..2u32) == 0 { 255 } else { a })
    });
    round_trips("Rect", |rng| Rect::new(any_vec2(rng), any_vec2(rng)));
    round_trips("Option<i32>", |rng| {
        (rng.range(0..2u32) == 0).then(|| rng.next_u32() as i32)
    });
    round_trips("Vec<f32>", |rng| {
        (0..rng.range(0..6u32))
            .map(|_| any_f32(rng))
            .collect::<Vec<_>>()
    });
    round_trips("Facing", |rng| any_inner(rng).b);
    round_trips("SaveId", |rng| SaveId(bits64(rng)));
    round_trips("Thing", |rng| Thing {
        count: rng.next_u32() as i32,
        speed: any_f32(rng),
        tint: (rng.range(0..2u32) == 0).then(|| Color::rgb(1, 2, 3)),
        tags: (0..rng.range(0..3u32)).map(|_| any_string(rng)).collect(),
        inner: any_inner(rng),
        area: Rect::new(any_vec2(rng), any_vec2(rng)),
        parts: (0..rng.range(0..3u32)).map(|_| any_inner(rng)).collect(),
    });
}

#[test]
fn the_edges_of_the_encodings() {
    let mut cx = ReflectContext::new("test.jsonc");
    // What an i64 cannot hold is written as digits, and only that.
    assert_eq!(u64::MAX.to_value(&mut cx), string("18446744073709551615"));
    assert_eq!(read::<u64>(&string("18446744073709551615")), Ok(u64::MAX));
    assert_eq!(read::<u64>(&Value::Int(5)), Ok(5));
    assert!(read::<u64>(&string("5")).is_err());
    assert!(read::<i32>(&string("12")).is_err());
    // A whole float is not an integer.
    assert!(read::<i32>(&Value::Float(12.0)).is_err());
    // Integers are read as floats; f32 holds what fits, and infinities and
    // NaN as they are.
    assert_eq!(read::<f32>(&Value::Int(3)), Ok(3.0));
    assert_eq!(read::<f32>(&Value::Float(f64::INFINITY)), Ok(f32::INFINITY));
    assert!(read::<f32>(&Value::Float(f64::NAN)).is_ok_and(f32::is_nan));
    assert!(read::<f32>(&Value::Float(1e39)).is_err());
    let largest = json::parse("3.4028235e38", Limits::default()).expect("a number");
    assert_eq!(read::<f32>(&largest), Ok(f32::MAX));
    // Colors: lowercase, alpha only when not opaque; either case read.
    assert_eq!(Color::rgb(255, 128, 0).to_value(&mut cx), string("#ff8000"));
    assert_eq!(
        Color::rgba(255, 128, 0, 16).to_value(&mut cx),
        string("#ff800010")
    );
    assert_eq!(
        read::<Color>(&string("#FF8000")),
        Ok(Color::rgb(255, 128, 0))
    );
    // A digit string too large for the type is out of range.
    let too_big = read::<u64>(&string("18446744073709551616")).expect_err("too big");
    assert!(
        matches!(too_big.kind, ReflectErrorKind::OutOfRange { .. }),
        "{too_big}"
    );
    // A whole number too large for an i64 is parsed as a float.
    let huge = json::parse("99999999999999999999", Limits::default()).expect("a number");
    assert_eq!(
        read::<u64>(&huge).expect_err("not an integer").to_string(),
        "test.jsonc: expected an integer, found a number that is not an integer an i64 can hold"
    );
    // Applying a rect sets only the corners it names.
    let mut area = Rect::new(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0));
    area.apply_value(&object(&[("max", ints(&[9, 9]))]), &mut cx)
        .expect("a corner");
    assert_eq!(area, Rect::new(Vec2::new(1.0, 2.0), Vec2::new(9.0, 9.0)));
    // Some(None) is null, which reads back as None.
    let nested: Option<Option<i32>> = Some(None);
    assert_eq!(nested.to_value(&mut cx), Value::Null);
    assert_eq!(read::<Option<Option<i32>>>(&Value::Null), Ok(None));
}

#[test]
fn an_option_merges_into_what_it_holds_and_a_list_is_replaced_whole() {
    let mut cx = ReflectContext::new("test.jsonc");
    let mut held = Some(Inner {
        a: 7,
        b: Facing::Up,
    });
    held.apply_value(&object(&[("b", string("Left"))]), &mut cx)
        .expect("a partial Inner");
    assert_eq!(
        held,
        Some(Inner {
            a: 7,
            b: Facing::Left
        })
    );
    held.apply_value(&Value::Null, &mut cx).expect("null");
    assert_eq!(held, None);

    let mut tags = vec!["kept".to_string()];
    let failed = tags.apply_value(&Value::Array(vec![string("a"), Value::Int(3)]), &mut cx);
    assert_eq!(failed.expect_err("not a string").path, "[1]");
    assert_eq!(tags, ["kept"], "a failed list leaves the old one");
}

// ---- Errors ----------------------------------------------------------------

#[test]
fn an_error_names_the_source_the_path_and_what_is_wrong() {
    let error = |members: &[(&str, Value)]| {
        read::<Thing>(&object(members)).expect_err("the value does not fit")
    };
    let wrong = error(&[("count", string("x"))]);
    assert_eq!(
        wrong.to_string(),
        "test.jsonc: at `count`: expected an integer, found a string"
    );
    let range = error(&[("inner", object(&[("a", Value::Int(300))]))]);
    assert_eq!(range.path, "inner.a");
    assert_eq!(
        range.kind,
        ReflectErrorKind::OutOfRange {
            type_name: "u8",
            value: "300".into()
        }
    );
    assert_eq!(
        error(&[("tags", Value::Array(vec![string("a"), Value::Int(3)]))]).path,
        "tags[1]"
    );
    let length = error(&[("area", object(&[("min", ints(&[1, 2, 3]))]))]);
    assert_eq!(
        (length.path.as_str(), length.kind),
        (
            "area.min",
            ReflectErrorKind::WrongLength {
                expected: 2,
                found: 3
            }
        )
    );
    let variant = error(&[(
        "parts",
        Value::Array(vec![object(&[("b", string("North"))])]),
    )]);
    assert_eq!(
        variant.to_string(),
        "test.jsonc: at `parts[0].b`: `North` is not a `Facing`: it is one of Down, Up, Left, Right"
    );
    assert_eq!(
        read::<Thing>(&Value::Int(1))
            .expect_err("not an object")
            .to_string(),
        "test.jsonc: expected an object, found an integer"
    );
}

#[test]
fn a_color_is_a_hash_and_six_or_eight_hex_digits() {
    for good in ["#ff8000", "#FF8000", "#ff8000ff"] {
        assert_eq!(
            read::<Color>(&string(good)),
            Ok(Color::rgb(255, 128, 0)),
            "{good}"
        );
    }
    for bad in [
        "ff8000",
        "##ff8000",
        "#fff",
        "#ff800",
        "#ff8000a",
        "#ff8000aa1",
        "#a\u{e9}000",
        "#+f+f+f",
        "#gg0000",
        "",
        "#",
    ] {
        let error = read::<Color>(&string(bad)).expect_err(bad);
        assert_eq!(
            error.kind,
            ReflectErrorKind::BadColor(bad.to_string()),
            "{bad}"
        );
    }
}

// ---- Unknown fields ----------------------------------------------------------

#[test]
fn an_unknown_field_is_a_warning_naming_the_file_the_type_and_the_field() {
    let data = object(&[
        ("count", Value::Int(4)),
        ("colour", string("#ffffff")),
        (
            "inner",
            object(&[("a", Value::Int(1)), ("c", Value::Int(2))]),
        ),
        (
            "area",
            object(&[("middle", ints(&[0, 0])), ("max", ints(&[3, 4]))]),
        ),
        (
            "parts",
            Value::Array(vec![object(&[]), object(&[("x", Value::Int(1))])]),
        ),
    ]);
    let mut cx = ReflectContext::new("prefabs/thing.jsonc");
    let thing = Thing::from_value(&data, &mut cx).expect("the known fields fit");
    // The known fields are applied all the same.
    assert_eq!(
        (thing.count, thing.inner.a, thing.area.max),
        (4, 1, Vec2::new(3.0, 4.0))
    );
    let warnings: Vec<String> = cx.warnings().iter().map(ToString::to_string).collect();
    assert_eq!(
        warnings,
        [
            "prefabs/thing.jsonc: `Thing` has no field `colour`, so it was left out",
            "prefabs/thing.jsonc: `Inner` has no field `c`, so it was left out (at `inner`)",
            "prefabs/thing.jsonc: `Rect` has no field `middle`, so it was left out (at `area`)",
            "prefabs/thing.jsonc: `Inner` has no field `x`, so it was left out (at `parts[1]`)",
        ]
    );
    // A tag has no fields at all.
    let mut cx = ReflectContext::new("prefabs/enemy.jsonc");
    assert_eq!(
        Enemy::from_value(&object(&[("hp", Value::Int(1))]), &mut cx),
        Ok(Enemy)
    );
    assert_eq!(cx.warnings()[0].field, "hp");
}

// ---- The registry ----------------------------------------------------------

mod other {
    crate::reflect_struct! {
        #[derive(Debug, Default)]
        pub(crate) struct Thing {
            pub size: u8,
        }
    }

    impl nadir_ecs::Component for Thing {}
}

#[test]
fn the_registry_makes_inserts_and_inspects_components_by_name() {
    let mut registry = TypeRegistry::new();
    registry.register_component::<Thing>();
    registry.register_component::<Thing>();
    assert_eq!(
        registry.iter().count(),
        1,
        "adding a type again does nothing"
    );
    let registration = registry.get("Thing").expect("registered");
    assert!(registry.get("Inner").is_none());

    let mut cx = ReflectContext::new("prefabs/thing.jsonc");
    let data = object(&[
        ("count", Value::Int(9)),
        ("tags", Value::Array(vec![string("x")])),
    ]);
    let made = registration.from_value(&data, &mut cx).expect("a Thing");
    assert_eq!(made.type_name(), "Thing");
    assert!(registration.from_value(&Value::Null, &mut cx).is_err());

    let mut world = World::new();
    registry.register_components(&mut world);
    let entity = world.spawn().id();
    registration
        .insert(&mut world.commands(), entity, &data, &mut cx)
        .expect("a Thing");
    world.apply_commands();
    assert_eq!(
        world.read::<Thing>().get(entity).map(|thing| thing.count),
        Some(9)
    );

    let mut seen = Value::Null;
    assert!(registration.inspect(&world, entity, &mut |thing| seen = thing.to_value(&mut cx)));
    assert_eq!(seen.get("count"), Some(&Value::Int(9)));
    assert!(registration.inspect_mut(&world, entity, &mut |thing| {
        thing
            .field_mut(0)
            .expect("count")
            .apply_value(&Value::Int(10), &mut cx)
            .expect("an integer");
    }));
    assert_eq!(
        world.read::<Thing>().get(entity).map(|thing| thing.count),
        Some(10)
    );
    let nobody = world.spawn().id();
    assert!(!registration.inspect(&world, nobody, &mut |_| {}));
}

#[test]
#[should_panic(expected = "two component types are named `Thing`")]
fn two_types_with_one_name_cannot_both_be_registered() {
    let mut registry = TypeRegistry::new();
    registry.register_component::<Thing>();
    registry.register_component::<other::Thing>();
}

// ---- The world's hash ------------------------------------------------------

impl Component for SaveId {}

/// A registry of `Thing` then `SaveId`, and a world that can hold them.
fn hashed_world() -> (TypeRegistry, World) {
    let mut registry = TypeRegistry::new();
    registry.register_component::<Thing>();
    registry.register_component::<SaveId>();
    let mut world = World::new();
    registry.register_components(&mut world);
    (registry, world)
}

fn thing(count: i32, speed: f32, tags: &[&str]) -> Thing {
    Thing {
        count,
        speed,
        tags: tags.iter().map(|&tag| tag.to_string()).collect(),
        ..Thing::default()
    }
}

/// The world of `hashed_world` with two things, one with a save id.
fn two_things() -> (TypeRegistry, World) {
    let (registry, mut world) = hashed_world();
    world.spawn().with(thing(1, 0.5, &["a"])).with(SaveId(7));
    world.spawn().with(thing(2, 1.5, &[]));
    (registry, world)
}

#[test]
fn each_visits_every_component_of_a_type_in_its_storages_order() {
    let (registry, mut world) = hashed_world();
    let entities: Vec<_> = (0..4)
        .map(|count| world.spawn().with(thing(count, 0.0, &[])).id())
        .collect();
    // A remove moves the last into its place.
    world.write::<Thing>().remove(entities[1]);
    let mut seen = Vec::new();
    let mut cx = ReflectContext::new("test");
    registry
        .get("Thing")
        .expect("registered")
        .each(&world, &mut |entity, thing| {
            seen.push((entity, thing.to_value(&mut cx).get("count").cloned()));
        });
    let count = |n| Some(Value::Int(n));
    assert_eq!(
        seen,
        [
            (entities[0], count(0)),
            (entities[3], count(3)),
            (entities[2], count(2)),
        ]
    );
    let mut none = 0;
    registry
        .get("SaveId")
        .expect("registered")
        .each(&world, &mut |_, _| none += 1);
    assert_eq!(none, 0);
}

#[test]
fn worlds_made_alike_hash_alike_and_any_reflected_difference_shows() {
    let hash = |(registry, world): &(TypeRegistry, World)| {
        registry.hash_world(world, &mut ReflectContext::new("test"))
    };
    let base = hash(&two_things());
    assert_eq!(base, hash(&two_things()));

    // A value.
    let changed = two_things();
    changed
        .1
        .write::<Thing>()
        .iter_mut()
        .next()
        .expect("a thing")
        .1
        .count = 3;
    assert_ne!(hash(&changed), base);
    // A float, by its bits.
    let (registry, mut world) = hashed_world();
    world.spawn().with(thing(1, 1.0, &[]));
    let one = registry.hash_world(&world, &mut ReflectContext::new("test"));
    world
        .write::<Thing>()
        .iter_mut()
        .next()
        .expect("a thing")
        .1
        .speed = 1.0 + f32::EPSILON;
    assert_ne!(
        registry.hash_world(&world, &mut ReflectContext::new("test")),
        one
    );
    // Strings split differently.
    let (registry, mut world) = hashed_world();
    world.spawn().with(thing(1, 0.5, &["ab", "c"]));
    let (_, mut other) = hashed_world();
    other.spawn().with(thing(1, 0.5, &["a", "bc"]));
    assert_ne!(
        registry.hash_world(&world, &mut ReflectContext::new("test")),
        registry.hash_world(&other, &mut ReflectContext::new("test"))
    );
    // The entity: the same values on a later generation of the slot.
    let (registry, mut world) = hashed_world();
    let gone = world.spawn().id();
    world.despawn(gone);
    world.spawn().with(thing(1, 0.5, &["a"])).with(SaveId(7));
    world.spawn().with(thing(2, 1.5, &[]));
    assert_ne!(
        registry.hash_world(&world, &mut ReflectContext::new("test")),
        base
    );
    // The storage's order: the same components inserted the other way.
    let (registry, mut world) = hashed_world();
    let first = world.spawn().id();
    let second = world.spawn().id();
    world.write::<Thing>().insert(second, thing(2, 1.5, &[]));
    world.write::<Thing>().insert(first, thing(1, 0.5, &["a"]));
    world.write::<SaveId>().insert(first, SaveId(7));
    assert_ne!(
        registry.hash_world(&world, &mut ReflectContext::new("test")),
        base
    );
    // A component removed.
    let (registry, world) = two_things();
    let saved = world.read::<SaveId>().entities()[0];
    world.write::<SaveId>().remove(saved);
    assert_ne!(
        registry.hash_world(&world, &mut ReflectContext::new("test")),
        base
    );
}

#[test]
fn the_hash_counts_the_types_and_their_order_but_nothing_unregistered() {
    let (_, world) = two_things();
    let mut reversed = TypeRegistry::new();
    reversed.register_component::<SaveId>();
    reversed.register_component::<Thing>();
    let (registry, _) = two_things();
    assert_ne!(
        reversed.hash_world(&world, &mut ReflectContext::new("test")),
        registry.hash_world(&world, &mut ReflectContext::new("test"))
    );
    // A type with no components still counts, by its name.
    let mut fewer = TypeRegistry::new();
    fewer.register_component::<Thing>();
    let mut only_things = World::new();
    fewer.register_components(&mut only_things);
    only_things.register::<SaveId>();
    only_things.spawn().with(thing(1, 0.5, &[]));
    let mut with_ids = TypeRegistry::new();
    with_ids.register_component::<Thing>();
    with_ids.register_component::<SaveId>();
    assert_ne!(
        fewer.hash_world(&only_things, &mut ReflectContext::new("test")),
        with_ids.hash_world(&only_things, &mut ReflectContext::new("test"))
    );
    // A component of a type the registry does not know changes nothing.
    let before = fewer.hash_world(&only_things, &mut ReflectContext::new("test"));
    only_things.spawn().with(SaveId(3));
    assert_eq!(
        fewer.hash_world(&only_things, &mut ReflectContext::new("test")),
        before
    );
}

#[test]
fn zeros_of_either_sign_hash_alike_and_so_do_all_nans() {
    let (registry, mut world) = hashed_world();
    let entity = world.spawn().with(thing(1, 0.0, &[])).id();
    let hash_of = |speed: f32| {
        world
            .write::<Thing>()
            .get_mut(entity)
            .expect("a thing")
            .speed = speed;
        registry.hash_world(&world, &mut ReflectContext::new("test"))
    };
    assert_eq!(hash_of(0.0), hash_of(-0.0));
    let nan = hash_of(f32::from_bits(0x7fc0_0000));
    assert_eq!(nan, hash_of(f32::from_bits(0xffc0_0000)));
    assert_eq!(nan, hash_of(f32::from_bits(0x7f80_0001)));
    assert_ne!(nan, hash_of(0.0));
    assert_ne!(nan, hash_of(f32::INFINITY));
}

#[test]
fn the_hash_of_a_known_world_is_fixed() {
    // The form the hash is taken of, on every system, as `hash_world`
    // documents it: these values were worked out from that description
    // apart from the code. Changing the form changes every stored
    // determinism hash (`crates/engine/tests/determinism`).
    let (registry, world) = two_things();
    let mut cx = ReflectContext::new("test");
    assert_eq!(registry.hash_world(&world, &mut cx), 0x9fd6_59a3_36d7_b47a);
    // A NaN, whatever its bits, as `0x7ff8_0000_0000_0000`.
    let first = world.read::<Thing>().entities()[0];
    world
        .write::<Thing>()
        .get_mut(first)
        .expect("a thing")
        .speed = f32::from_bits(0xffc0_0001);
    assert_eq!(registry.hash_world(&world, &mut cx), 0x74d3_3433_a0cf_2672);
}

#[test]
#[should_panic(expected = "is not registered")]
fn hashing_a_world_that_cannot_hold_a_type_panics() {
    let (registry, _) = hashed_world();
    registry.hash_world(&World::new(), &mut ReflectContext::new("test"));
}
