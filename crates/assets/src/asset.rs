//! The asset types' side: decoding, then finalizing on the main thread.

use std::any::{Any, type_name};

use nadir_codec::Limits;

use crate::AssetError;

/// A kind of asset: a texture, a sound, a level.
///
/// Loading one has two steps:
///
/// 1. [`Asset::decode`] turns the file's bytes into data. From roadmap
///    M5.3 this runs on a worker thread, so it reaches nothing but the
///    bytes and the [`DecodeCtx`].
/// 2. [`Asset::finalize`] makes the asset from that data on the main thread,
///    with the resources in the [`FinalizeCtx`]: a texture is uploaded to
///    the GPU there.
pub trait Asset: Sized + Send + 'static {
    /// What [`Asset::decode`] makes.
    type Decoded: Send + 'static;

    /// The extensions, without the dot, that a path of this type ends in.
    const EXTENSIONS: &'static [&'static str];

    /// Decodes a file's bytes.
    ///
    /// # Errors
    ///
    /// Any [`AssetError`]; usually [`AssetError::Decode`] or
    /// [`AssetError::Invalid`].
    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<Self::Decoded, AssetError>;

    /// Makes the asset from what [`Asset::decode`] made.
    ///
    /// # Errors
    ///
    /// Any [`AssetError`]; usually [`AssetError::MissingContext`] or
    /// [`AssetError::Invalid`].
    fn finalize(decoded: Self::Decoded, ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError>;

    /// Makes the stand-in for an asset that failed to load, when the server
    /// shows placeholders ([`OnFailure::Placeholder`](crate::OnFailure)).
    /// None by default.
    ///
    /// # Errors
    ///
    /// [`AssetError::NoPlaceholder`] if the type has none, or why it could
    /// not be made.
    fn placeholder(ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        let _ = ctx;
        Err(AssetError::NoPlaceholder)
    }
}

/// What [`Asset::decode`] may know besides the bytes.
#[derive(Clone, Copy, Debug)]
pub struct DecodeCtx<'a> {
    path: &'a str,
    limits: Limits,
}

impl<'a> DecodeCtx<'a> {
    /// A context for decoding the asset at `path`. The server makes one for
    /// each load; tests and tools can make their own.
    pub fn new(path: &'a str, limits: Limits) -> Self {
        Self { path, limits }
    }

    /// The asset's path, for labels and messages.
    pub fn path(&self) -> &str {
        self.path
    }

    /// The limits that decoding must keep to.
    pub fn limits(&self) -> Limits {
        self.limits
    }
}

/// The resources [`Asset::finalize`] makes assets with, such as the GPU
/// device, by type. It holds at most [`FinalizeCtx::CAPACITY`] of them, and
/// allocates nothing, so a game can make one every frame.
///
/// ```
/// use nadir_assets::FinalizeCtx;
///
/// struct Device;
/// let mut device = Device;
/// let mut ctx = FinalizeCtx::new().with(&mut device);
/// assert!(ctx.get_mut::<Device>().is_ok());
/// assert!(ctx.get_mut::<u32>().is_err());
/// ```
pub struct FinalizeCtx<'a> {
    resources: [Option<&'a mut dyn Any>; FinalizeCtx::CAPACITY],
}

impl<'a> FinalizeCtx<'a> {
    /// The most resources a context holds.
    pub const CAPACITY: usize = 4;

    /// A context with no resources.
    pub fn new() -> Self {
        Self {
            resources: [const { None }; Self::CAPACITY],
        }
    }

    /// The context with `resource` too. It replaces a resource of the same
    /// type.
    ///
    /// A resource past [`FinalizeCtx::CAPACITY`] is not kept, so
    /// [`FinalizeCtx::get_mut`] does not find it; debug builds panic
    /// instead.
    #[must_use]
    pub fn with<R: Any>(mut self, resource: &'a mut R) -> Self {
        let held =
            |slot: &Option<&'a mut dyn Any>| slot.as_deref().is_some_and(|held| held.is::<R>());
        let index = (self.resources.iter().position(held))
            .or_else(|| self.resources.iter().position(Option::is_none));
        match index {
            Some(index) => self.resources[index] = Some(resource),
            None => debug_assert!(
                false,
                "a FinalizeCtx holds at most {} resources",
                Self::CAPACITY
            ),
        }
        self
    }

    /// The resource of type `R`.
    ///
    /// # Errors
    ///
    /// [`AssetError::MissingContext`] if the context holds none.
    pub fn get_mut<R: Any>(&mut self) -> Result<&mut R, AssetError> {
        self.resources
            .iter_mut()
            .flatten()
            .find_map(|resource| resource.downcast_mut::<R>())
            .ok_or(AssetError::MissingContext(type_name::<R>()))
    }
}

impl Default for FinalizeCtx<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_are_found_by_type() {
        let (mut a, mut b, mut c) = (1u32, 2u64, String::from("x"));
        let mut ctx = FinalizeCtx::new().with(&mut a).with(&mut b).with(&mut c);
        *ctx.get_mut::<u32>().expect("held") += 10;
        assert_eq!(*ctx.get_mut::<u64>().expect("held"), 2);
        assert_eq!(ctx.get_mut::<String>().expect("held"), "x");
        assert_eq!(
            ctx.get_mut::<i8>().map(drop),
            Err(AssetError::MissingContext("i8"))
        );
        assert_eq!(a, 11);
    }

    #[test]
    fn a_resource_of_a_held_type_replaces_it() {
        let (mut first, mut second) = (1u32, 2u32);
        let mut ctx = FinalizeCtx::new().with(&mut first).with(&mut second);
        assert_eq!(*ctx.get_mut::<u32>().expect("held"), 2);
        assert_eq!(ctx.resources.iter().flatten().count(), 1);
    }

    #[test]
    fn a_context_holds_up_to_its_capacity() {
        let (mut a, mut b, mut c, mut d) = (1u8, 2u16, 3u32, 4u64);
        let mut ctx = FinalizeCtx::new()
            .with(&mut a)
            .with(&mut b)
            .with(&mut c)
            .with(&mut d);
        assert_eq!(*ctx.get_mut::<u8>().expect("held"), 1);
        assert_eq!(*ctx.get_mut::<u64>().expect("held"), 4);
    }

    #[test]
    #[should_panic(expected = "a FinalizeCtx holds at most 4 resources")]
    #[cfg(debug_assertions)]
    fn one_resource_too_many_panics_in_debug_builds() {
        let (mut a, mut b, mut c, mut d, mut e) = (1u8, 2u16, 3u32, 4u64, 5i8);
        let _ = FinalizeCtx::new()
            .with(&mut a)
            .with(&mut b)
            .with(&mut c)
            .with(&mut d)
            .with(&mut e);
    }
}
