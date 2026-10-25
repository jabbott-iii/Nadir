//! The macros in a crate of their own, among items that share the names
//! of the standard ones they use: the generated code still means the
//! standard ones.

#![allow(dead_code, non_camel_case_types, unused_macros)] // The shadowing items exist only to be in the way.

use nadir_scene::{FromValue, Reflect, ReflectContext, Value};

struct Option;
struct Some;
struct None;
struct Result;
struct Ok;
struct Err;
struct String;
struct Vec;
struct usize;
struct str;

macro_rules! stringify {
    ($($tokens:tt)*) => {
        "not the standard stringify"
    };
}

macro_rules! concat {
    ($($tokens:tt)*) => {
        "not the standard concat"
    };
}

nadir_scene::reflect_struct! {
    #[derive(Debug, Default, PartialEq)]
    struct Spot {
        /// Where it is.
        at: nadir_core::Vec2,
        name: std::string::String,
    }
}

nadir_scene::reflect_enum! {
    #[derive(Debug, Default, PartialEq)]
    enum Mood { #[default] Calm, Cross }
}

nadir_scene::reflect_newtype! {
    #[derive(Debug, Default, PartialEq)]
    struct Id(u32);
}

nadir_scene::reflect_struct! {
    #[derive(Debug, Default, PartialEq)]
    struct Marker;
}

#[test]
fn the_macros_work_among_shadowing_names() {
    let mut cx = ReflectContext::new("shadowed");
    let spot = Spot {
        at: nadir_core::Vec2::new(1.0, 2.0),
        name: "here".into(),
    };
    let value = spot.to_value(&mut cx);
    assert_eq!(
        Spot::from_value(&value, &mut cx),
        core::result::Result::Ok(spot)
    );
    assert_eq!(Spot::default().fields()[0].name, "at");
    assert_eq!(Spot::default().fields()[0].doc, " Where it is.\n");
    assert_eq!(
        Mood::from_value(&Value::String("Cross".into()), &mut cx),
        core::result::Result::Ok(Mood::Cross)
    );
    assert_eq!(Id(3).to_value(&mut cx), Value::Int(3));
    assert_eq!(Marker.type_name(), "Marker");
}
