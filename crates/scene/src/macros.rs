//! The reflection macros: each defines a type and its [`Reflect`] and
//! [`FromValue`] impls together (ADR-0007).
//!
//! The generated code names the types, functions and macros it uses by
//! absolute path, so that a caller's own items of the same names cannot
//! take them over, and it needs no imports besides the macro. (It still
//! calls methods by name, and binds locals with plain names such as
//! `value` and `this`: a caller's unit struct named `this` breaks it, as it
//! would the standard library's macros.)
//!
//! [`Reflect`]: crate::Reflect
//! [`FromValue`]: crate::FromValue

/// Defines a struct with named fields, and its [`Reflect`](crate::Reflect)
/// and [`FromValue`](crate::FromValue) impls. Every field's type must be
/// reflected, and the struct must implement `Default`: a value is made by
/// applying data to it, so that fields the data leaves out keep their
/// defaults.
///
/// ```
/// nadir_scene::reflect_struct! {
///     /// Hit points of a damageable entity.
///     #[derive(Clone, Debug, Default, PartialEq)]
///     pub struct Health {
///         /// What is left.
///         pub current: i32,
///         pub max: i32,
///     }
/// }
///
/// use nadir_scene::{FromValue, ReflectContext, Value};
///
/// let data = Value::Object(vec![("max".to_string(), Value::Int(12))]);
/// let mut cx = ReflectContext::new("example");
/// let health = Health::from_value(&data, &mut cx).expect("a Health");
/// assert_eq!(health, Health { current: 0, max: 12 });
/// ```
///
/// A unit struct, `struct Enemy;`, is written as `{}`: a tag component.
/// Generics are not supported, nor are raw identifiers (`r#type`) as field
/// names; write a hand-made impl for those.
#[macro_export]
macro_rules! reflect_struct {
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$attr])*
        $vis struct $name;

        impl $crate::Reflect for $name {
            fn type_name(&self) -> &'static ::core::primitive::str {
                ::core::stringify!($name)
            }

            fn kind(&self) -> $crate::ReflectKind {
                $crate::ReflectKind::Struct
            }

            fn fields(&self) -> &'static [$crate::FieldInfo] {
                &[]
            }

            fn field(&self, _index: ::core::primitive::usize) -> ::core::option::Option<&dyn $crate::Reflect> {
                ::core::option::Option::None
            }

            fn field_mut(&mut self, _index: ::core::primitive::usize) -> ::core::option::Option<&mut dyn $crate::Reflect> {
                ::core::option::Option::None
            }

            fn to_value(&self, _cx: &mut $crate::ReflectContext<'_>) -> $crate::Value {
                $crate::Value::Object(::std::vec::Vec::new())
            }

            fn apply_value(
                &mut self,
                value: &$crate::Value,
                cx: &mut $crate::ReflectContext<'_>,
            ) -> ::core::result::Result<(), $crate::ReflectError> {
                let ::core::option::Option::Some(members) = value.as_object() else {
                    return ::core::result::Result::Err(cx.expected("an object", value));
                };
                for member in members {
                    cx.unknown_field(::core::stringify!($name), &member.0);
                }
                ::core::result::Result::Ok(())
            }
        }

        impl $crate::FromValue for $name {
            const TYPE_NAME: &'static ::core::primitive::str = ::core::stringify!($name);

            fn from_value(
                value: &$crate::Value,
                cx: &mut $crate::ReflectContext<'_>,
            ) -> ::core::result::Result<Self, $crate::ReflectError> {
                let mut this = $name;
                $crate::Reflect::apply_value(&mut this, value, cx)?;
                ::core::result::Result::Ok(this)
            }
        }
    };
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$($field_attr:tt)*])*
                $field_vis:vis $field:ident : $type:ty
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        $vis struct $name {
            $(
                $(#[$($field_attr)*])*
                $field_vis $field: $type,
            )*
        }

        impl $crate::Reflect for $name {
            fn type_name(&self) -> &'static ::core::primitive::str {
                ::core::stringify!($name)
            }

            fn kind(&self) -> $crate::ReflectKind {
                $crate::ReflectKind::Struct
            }

            fn fields(&self) -> &'static [$crate::FieldInfo] {
                const FIELDS: &[$crate::FieldInfo] = &[$(
                    $crate::FieldInfo {
                        name: ::core::stringify!($field),
                        type_name: ::core::stringify!($type),
                        doc: $crate::__reflect_doc!($([$($field_attr)*])*),
                    },
                )*];
                FIELDS
            }

            fn field(&self, index: ::core::primitive::usize) -> ::core::option::Option<&dyn $crate::Reflect> {
                let fields: &[&dyn $crate::Reflect] = &[$(&self.$field,)*];
                fields.get(index).copied()
            }

            fn field_mut(&mut self, index: ::core::primitive::usize) -> ::core::option::Option<&mut dyn $crate::Reflect> {
                let fields: [&mut dyn $crate::Reflect; 0 $(+ $crate::__reflect_one!($field))*] =
                    [$(&mut self.$field,)*];
                fields.into_iter().nth(index)
            }

            fn to_value(&self, cx: &mut $crate::ReflectContext<'_>) -> $crate::Value {
                let _ = &cx;
                $crate::Value::Object(::std::vec![$(
                    (
                        ::std::string::String::from(::core::stringify!($field)),
                        $crate::Reflect::to_value(&self.$field, cx),
                    ),
                )*])
            }

            fn apply_value(
                &mut self,
                value: &$crate::Value,
                cx: &mut $crate::ReflectContext<'_>,
            ) -> ::core::result::Result<(), $crate::ReflectError> {
                let ::core::option::Option::Some(members) = value.as_object() else {
                    return ::core::result::Result::Err(cx.expected("an object", value));
                };
                for member in members {
                    let key = member.0.as_str();
                    $(
                        if key == ::core::stringify!($field) {
                            cx.field(::core::stringify!($field), |cx| {
                                $crate::Reflect::apply_value(&mut self.$field, &member.1, cx)
                            })?;
                            continue;
                        }
                    )*
                    cx.unknown_field(::core::stringify!($name), key);
                }
                ::core::result::Result::Ok(())
            }
        }

        impl $crate::FromValue for $name {
            const TYPE_NAME: &'static ::core::primitive::str = ::core::stringify!($name);

            fn from_value(
                value: &$crate::Value,
                cx: &mut $crate::ReflectContext<'_>,
            ) -> ::core::result::Result<Self, $crate::ReflectError> {
                let mut this = <Self as ::core::default::Default>::default();
                $crate::Reflect::apply_value(&mut this, value, cx)?;
                ::core::result::Result::Ok(this)
            }
        }
    };
}

/// Defines an enum of unit variants, and its [`Reflect`](crate::Reflect)
/// and [`FromValue`](crate::FromValue) impls. A value is written as its
/// variant's name.
///
/// ```
/// nadir_scene::reflect_enum! {
///     #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
///     pub enum Facing { #[default] Down, Up, Left, Right }
/// }
///
/// use nadir_scene::{FromValue, ReflectContext, Value};
///
/// let mut cx = ReflectContext::new("example");
/// let facing = Facing::from_value(&Value::String("Left".into()), &mut cx);
/// assert_eq!(facing, Ok(Facing::Left));
/// ```
///
/// Enums whose variants carry data are not supported: write a hand-made
/// impl for those.
#[macro_export]
macro_rules! reflect_enum {
    (
        $(#[$attr:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_attr:meta])*
                $variant:ident
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        $vis enum $name {
            $(
                $(#[$variant_attr])*
                $variant,
            )*
        }

        impl $crate::Reflect for $name {
            fn type_name(&self) -> &'static ::core::primitive::str {
                ::core::stringify!($name)
            }

            fn kind(&self) -> $crate::ReflectKind {
                $crate::ReflectKind::Enum
            }

            fn fields(&self) -> &'static [$crate::FieldInfo] {
                &[]
            }

            fn field(&self, _index: ::core::primitive::usize) -> ::core::option::Option<&dyn $crate::Reflect> {
                ::core::option::Option::None
            }

            fn field_mut(&mut self, _index: ::core::primitive::usize) -> ::core::option::Option<&mut dyn $crate::Reflect> {
                ::core::option::Option::None
            }

            fn to_value(&self, _cx: &mut $crate::ReflectContext<'_>) -> $crate::Value {
                let name = match self {
                    $(Self::$variant => ::core::stringify!($variant),)*
                };
                $crate::Value::String(::std::string::String::from(name))
            }

            fn apply_value(
                &mut self,
                value: &$crate::Value,
                cx: &mut $crate::ReflectContext<'_>,
            ) -> ::core::result::Result<(), $crate::ReflectError> {
                *self = <Self as $crate::FromValue>::from_value(value, cx)?;
                ::core::result::Result::Ok(())
            }
        }

        impl $crate::FromValue for $name {
            const TYPE_NAME: &'static ::core::primitive::str = ::core::stringify!($name);

            fn from_value(
                value: &$crate::Value,
                cx: &mut $crate::ReflectContext<'_>,
            ) -> ::core::result::Result<Self, $crate::ReflectError> {
                let ::core::option::Option::Some(name) = value.as_str() else {
                    return ::core::result::Result::Err(cx.expected("a variant's name", value));
                };
                $(
                    if name == ::core::stringify!($variant) {
                        return ::core::result::Result::Ok(Self::$variant);
                    }
                )*
                ::core::result::Result::Err(cx.error($crate::ReflectErrorKind::UnknownVariant {
                    type_name: ::core::stringify!($name),
                    found: ::std::string::String::from(name),
                    variants: &[$(::core::stringify!($variant)),*],
                }))
            }
        }
    };
}

/// Defines a struct that wraps one value, and its
/// [`Reflect`](crate::Reflect) and [`FromValue`](crate::FromValue) impls.
/// It is written as the value it wraps, and walks as it: only its type name
/// is its own.
///
/// ```
/// nadir_scene::reflect_newtype! {
///     #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
///     pub struct SaveId(pub u64);
/// }
///
/// use nadir_scene::{FromValue, Reflect, ReflectContext, Value};
///
/// let mut cx = ReflectContext::new("example");
/// assert_eq!(SaveId(7).to_value(&mut cx), Value::Int(7));
/// assert_eq!(SaveId::from_value(&Value::Int(9), &mut cx), Ok(SaveId(9)));
/// ```
#[macro_export]
macro_rules! reflect_newtype {
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident ( $(#[$inner_attr:meta])* $inner_vis:vis $inner:ty $(,)? );
    ) => {
        $(#[$attr])*
        $vis struct $name($(#[$inner_attr])* $inner_vis $inner);

        impl $crate::Reflect for $name {
            fn type_name(&self) -> &'static ::core::primitive::str {
                ::core::stringify!($name)
            }

            fn kind(&self) -> $crate::ReflectKind {
                $crate::Reflect::kind(&self.0)
            }

            fn fields(&self) -> &'static [$crate::FieldInfo] {
                $crate::Reflect::fields(&self.0)
            }

            fn field(&self, index: ::core::primitive::usize) -> ::core::option::Option<&dyn $crate::Reflect> {
                $crate::Reflect::field(&self.0, index)
            }

            fn field_mut(&mut self, index: ::core::primitive::usize) -> ::core::option::Option<&mut dyn $crate::Reflect> {
                $crate::Reflect::field_mut(&mut self.0, index)
            }

            fn to_value(&self, cx: &mut $crate::ReflectContext<'_>) -> $crate::Value {
                $crate::Reflect::to_value(&self.0, cx)
            }

            fn apply_value(
                &mut self,
                value: &$crate::Value,
                cx: &mut $crate::ReflectContext<'_>,
            ) -> ::core::result::Result<(), $crate::ReflectError> {
                $crate::Reflect::apply_value(&mut self.0, value, cx)
            }
        }

        impl $crate::FromValue for $name {
            const TYPE_NAME: &'static ::core::primitive::str = ::core::stringify!($name);

            fn from_value(
                value: &$crate::Value,
                cx: &mut $crate::ReflectContext<'_>,
            ) -> ::core::result::Result<Self, $crate::ReflectError> {
                ::core::result::Result::Ok(Self(<$inner as $crate::FromValue>::from_value(value, cx)?))
            }
        }
    };
}

/// The doc comments among a field's attributes, joined: each `[...]` is
/// one attribute's tokens.
#[doc(hidden)]
#[macro_export]
macro_rules! __reflect_doc {
    (@ [$($done:expr,)*]) => {
        ::core::concat!($($done,)*)
    };
    (@ [$($done:expr,)*] [doc = $doc:expr] $($rest:tt)*) => {
        $crate::__reflect_doc!(@ [$($done,)* $doc, "\n",] $($rest)*)
    };
    (@ [$($done:expr,)*] [$($other:tt)*] $($rest:tt)*) => {
        $crate::__reflect_doc!(@ [$($done,)*] $($rest)*)
    };
    ($($attrs:tt)*) => {
        $crate::__reflect_doc!(@ [] $($attrs)*)
    };
}

/// `1`, once for each field: the length of the field array.
#[doc(hidden)]
#[macro_export]
macro_rules! __reflect_one {
    ($field:ident) => {
        1
    };
}
