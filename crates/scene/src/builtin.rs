//! [`Reflect`] and [`FromValue`] for the built-in types: numbers, `bool`,
//! `String`, `Vec2`, `IVec2`, `Color`, `Rect`, `Option<T>` and `Vec<T>`.
//!
//! The encodings (ADR-0020):
//!
//! - integers are JSON integers; a `u64` or `usize` too large for an `i64`
//!   is written as a string of its digits, and read back from one;
//! - floats are JSON numbers, and integers are read as floats too. A
//!   non-finite float is a `Value::Float`, which JSON text writes as `null`,
//!   so it does not survive text;
//! - `Vec2` and `IVec2` are `[x, y]`;
//! - `Color` is `"#rrggbb"`, or `"#rrggbbaa"` when it is not opaque;
//! - `Rect` is `{"min": [x, y], "max": [x, y]}`, and applying one sets only
//!   the corners it names;
//! - `Option<T>` is `null` or the value. So `Some(None)` reads back as
//!   `None`, as does a `Some` of anything written as `null`.
//! - `Vec<T>` is an array. Applying one replaces the list, once every
//!   element has been read.

use nadir_codec::json::Value;
use nadir_core::{Color, IVec2, Rect, Vec2};

use crate::{
    FieldInfo, FromValue, Reflect, ReflectContext, ReflectError, ReflectErrorKind, ReflectKind,
};

/// The parts of [`Reflect`] that are the same for every primitive: no
/// fields, and applying is making a new value.
macro_rules! primitive {
    ($name:expr) => {
        fn type_name(&self) -> &'static str {
            $name
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

        fn apply_value(
            &mut self,
            value: &Value,
            cx: &mut ReflectContext<'_>,
        ) -> Result<(), ReflectError> {
            *self = Self::from_value(value, cx)?;
            Ok(())
        }
    };
}

macro_rules! integers {
    ($($int:ty),*) => {$(
        impl Reflect for $int {
            primitive!(stringify!($int));

            fn to_value(&self, _cx: &mut ReflectContext<'_>) -> Value {
                match i64::try_from(*self) {
                    Ok(value) => Value::Int(value),
                    Err(_) => Value::String(self.to_string()),
                }
            }
        }

        impl FromValue for $int {
            const TYPE_NAME: &'static str = stringify!($int);

            fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
                let out_of_range = |cx: &ReflectContext<'_>, text: String| {
                    cx.error(ReflectErrorKind::OutOfRange {
                        type_name: Self::TYPE_NAME,
                        value: text,
                    })
                };
                match value {
                    Value::Int(int) => <$int>::try_from(*int).map_err(|_| out_of_range(cx, int.to_string())),
                    // Only what an i64 cannot hold is written as digits.
                    Value::String(digits) => match digits.parse::<$int>() {
                        Ok(int) if i64::try_from(int).is_err() => Ok(int),
                        Err(_) if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) => {
                            Err(out_of_range(cx, digits.clone()))
                        }
                        _ => Err(cx.expected("an integer", value)),
                    },
                    _ => Err(cx.expected("an integer", value)),
                }
            }
        }
    )*};
}

integers!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl Reflect for f64 {
    primitive!("f64");

    fn to_value(&self, _cx: &mut ReflectContext<'_>) -> Value {
        Value::Float(*self)
    }
}

impl FromValue for f64 {
    const TYPE_NAME: &'static str = "f64";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        match *value {
            Value::Float(float) => Ok(float),
            Value::Int(int) => Ok(int as f64),
            _ => Err(cx.expected("a number", value)),
        }
    }
}

impl Reflect for f32 {
    primitive!("f32");

    fn to_value(&self, _cx: &mut ReflectContext<'_>) -> Value {
        Value::Float(f64::from(*self))
    }
}

impl FromValue for f32 {
    const TYPE_NAME: &'static str = "f32";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let wide = f64::from_value(value, cx)?;
        let narrow = wide as f32;
        // Too large for an f32; infinities and NaN stay what they are.
        if wide.is_finite() && narrow.is_infinite() {
            return Err(cx.error(ReflectErrorKind::OutOfRange {
                type_name: Self::TYPE_NAME,
                value: wide.to_string(),
            }));
        }
        Ok(narrow)
    }
}

impl Reflect for bool {
    primitive!("bool");

    fn to_value(&self, _cx: &mut ReflectContext<'_>) -> Value {
        Value::Bool(*self)
    }
}

impl FromValue for bool {
    const TYPE_NAME: &'static str = "bool";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        value
            .as_bool()
            .ok_or_else(|| cx.expected("a boolean", value))
    }
}

impl Reflect for String {
    primitive!("String");

    fn to_value(&self, _cx: &mut ReflectContext<'_>) -> Value {
        Value::String(self.clone())
    }
}

impl FromValue for String {
    const TYPE_NAME: &'static str = "String";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| cx.expected("a string", value))
    }
}

/// The two elements of an `[x, y]` array.
fn pair<'v>(
    value: &'v Value,
    cx: &ReflectContext<'_>,
) -> Result<(&'v Value, &'v Value), ReflectError> {
    match value.as_array() {
        Some([x, y]) => Ok((x, y)),
        Some(other) => Err(cx.error(ReflectErrorKind::WrongLength {
            expected: 2,
            found: other.len(),
        })),
        None => Err(cx.expected("an array `[x, y]`", value)),
    }
}

impl Reflect for Vec2 {
    primitive!("Vec2");

    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        Value::Array(vec![self.x.to_value(cx), self.y.to_value(cx)])
    }
}

impl FromValue for Vec2 {
    const TYPE_NAME: &'static str = "Vec2";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let (x, y) = pair(value, cx)?;
        let x = cx.index(0, |cx| f32::from_value(x, cx))?;
        let y = cx.index(1, |cx| f32::from_value(y, cx))?;
        Ok(Vec2::new(x, y))
    }
}

impl Reflect for IVec2 {
    primitive!("IVec2");

    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        Value::Array(vec![self.x.to_value(cx), self.y.to_value(cx)])
    }
}

impl FromValue for IVec2 {
    const TYPE_NAME: &'static str = "IVec2";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let (x, y) = pair(value, cx)?;
        let x = cx.index(0, |cx| i32::from_value(x, cx))?;
        let y = cx.index(1, |cx| i32::from_value(y, cx))?;
        Ok(IVec2::new(x, y))
    }
}

impl Reflect for Color {
    primitive!("Color");

    fn to_value(&self, _cx: &mut ReflectContext<'_>) -> Value {
        let Color { r, g, b, a } = *self;
        Value::String(if a == u8::MAX {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        })
    }
}

impl FromValue for Color {
    const TYPE_NAME: &'static str = "Color";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let text = value
            .as_str()
            .ok_or_else(|| cx.expected("a color `#rrggbb` or `#rrggbbaa`", value))?;
        // One `#`, then the digits: `from_hex` would take a second one.
        text.strip_prefix('#')
            .filter(|digits| !digits.starts_with('#'))
            .and_then(|digits| Color::from_hex(digits).ok())
            .ok_or_else(|| cx.error(ReflectErrorKind::BadColor(text.to_string())))
    }
}

impl Reflect for Rect {
    fn type_name(&self) -> &'static str {
        "Rect"
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
            ("min".to_string(), self.min.to_value(cx)),
            ("max".to_string(), self.max.to_value(cx)),
        ])
    }

    /// Sets the corners the value names, as a struct's fields are set: a
    /// corner it leaves out stays where it is.
    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        let members = value
            .as_object()
            .ok_or_else(|| cx.expected("an object `{\"min\": [x, y], \"max\": [x, y]}`", value))?;
        for (key, member) in members {
            match key.as_str() {
                "min" => self.min = cx.field("min", |cx| Vec2::from_value(member, cx))?,
                "max" => self.max = cx.field("max", |cx| Vec2::from_value(member, cx))?,
                _ => cx.unknown_field(<Self as FromValue>::TYPE_NAME, key),
            }
        }
        Ok(())
    }
}

impl FromValue for Rect {
    const TYPE_NAME: &'static str = "Rect";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let mut rect = Rect::default();
        rect.apply_value(value, cx)?;
        Ok(rect)
    }
}

impl<T: FromValue> Reflect for Option<T> {
    fn type_name(&self) -> &'static str {
        "Option"
    }

    fn kind(&self) -> ReflectKind {
        ReflectKind::Option
    }

    fn fields(&self) -> &'static [FieldInfo] {
        &[]
    }

    fn field(&self, index: usize) -> Option<&dyn Reflect> {
        match self {
            Some(inner) if index == 0 => Some(inner),
            _ => None,
        }
    }

    fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect> {
        match self {
            Some(inner) if index == 0 => Some(inner),
            _ => None,
        }
    }

    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        match self {
            Some(inner) => inner.to_value(cx),
            None => Value::Null,
        }
    }

    /// `null` makes it `None`. Anything else is applied to the value it
    /// holds, or, if it holds none, made into one.
    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        match (self.as_mut(), value) {
            (_, Value::Null) => *self = None,
            (Some(inner), _) => inner.apply_value(value, cx)?,
            (None, _) => *self = Some(T::from_value(value, cx)?),
        }
        Ok(())
    }
}

impl<T: FromValue> FromValue for Option<T> {
    const TYPE_NAME: &'static str = "Option";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        match value {
            Value::Null => Ok(None),
            _ => T::from_value(value, cx).map(Some),
        }
    }
}

impl<T: FromValue> Reflect for Vec<T> {
    fn type_name(&self) -> &'static str {
        "Vec"
    }

    fn kind(&self) -> ReflectKind {
        ReflectKind::List
    }

    fn fields(&self) -> &'static [FieldInfo] {
        &[]
    }

    fn field(&self, index: usize) -> Option<&dyn Reflect> {
        self.get(index).map(|element| element as &dyn Reflect)
    }

    fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect> {
        self.get_mut(index)
            .map(|element| element as &mut dyn Reflect)
    }

    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value {
        Value::Array(self.iter().map(|element| element.to_value(cx)).collect())
    }

    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError> {
        *self = Self::from_value(value, cx)?;
        Ok(())
    }
}

impl<T: FromValue> FromValue for Vec<T> {
    const TYPE_NAME: &'static str = "Vec";

    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError> {
        let elements = value
            .as_array()
            .ok_or_else(|| cx.expected("an array", value))?;
        elements
            .iter()
            .enumerate()
            .map(|(index, element)| cx.index(index, |cx| T::from_value(element, cx)))
            .collect()
    }
}
