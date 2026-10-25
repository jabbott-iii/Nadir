//! Scene layer for the Nadir engine: reflection macros, prefabs, levels,
//! saves and game states.
//!
//! So far there is reflection (roadmap M4.2): the [`Reflect`] and
//! [`FromValue`] traits, the macros [`reflect_struct!`], [`reflect_enum!`]
//! and [`reflect_newtype!`] that define a type with its impls, the
//! built-in impls, and the [`TypeRegistry`] of component types. Values go
//! to and from [`Value`], the JSON tree that prefabs, levels and saves are
//! read into. `docs/subsystems/scene.md` has the design.

mod builtin;
mod macros;
mod reflect;
mod registry;
#[cfg(test)]
mod tests;

pub use nadir_codec::json::Value;
pub use reflect::{
    FieldInfo, FromValue, Reflect, ReflectContext, ReflectError, ReflectErrorKind, ReflectKind,
    ReflectWarning,
};
pub use registry::{Registration, TypeRegistry};
