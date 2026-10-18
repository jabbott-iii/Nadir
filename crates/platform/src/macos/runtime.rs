//! Safe pieces over the Objective-C runtime: selectors, classes, owned
//! objects, strings, autorelease pools, and the classes we build.
//!
//! Every function here runs on the main thread, as AppKit requires. The
//! backend checks that before it creates anything.

use core::ffi::{CStr, c_void};
use std::ffi::CString;

use nadir_sys::macos::foundation::{NSUInteger, NSUTF8StringEncoding};
use nadir_sys::macos::objc::{
    BOOL, BOOL_ENCODING, Class, Id, Imp, NO, Sel, class_addIvar, class_addMethod,
    class_getInstanceVariable, ivar_getOffset, objc_allocateClassPair, objc_autoreleasePoolPop,
    objc_autoreleasePoolPush, objc_getClass, objc_object, objc_registerClassPair, sel_registerName,
};
use nadir_sys::msg_send;

use crate::PlatformError;

/// The name of the instance variable, in each class of ours, that points to
/// the state the backend shares with it.
const STATE_IVAR: &CStr = c"nadirState";

/// Returns the selector for a method name.
pub(super) fn sel(name: &CStr) -> Sel {
    // SAFETY: `name` is a C string.
    unsafe { sel_registerName(name.as_ptr()) }
}

/// Returns the class called `name`.
pub(super) fn class(name: &CStr) -> Result<Class, PlatformError> {
    // SAFETY: `name` is a C string.
    let class = unsafe { objc_getClass(name.as_ptr()) };
    if class.is_null() {
        Err(PlatformError::CallFailed {
            call: "objc_getClass",
        })
    } else {
        Ok(class)
    }
}

/// A class, as the receiver of a message.
pub(super) fn as_id(class: Class) -> Id {
    class.cast::<objc_object>()
}

/// Whether this is the main thread, where AppKit must be used.
pub(super) fn is_main_thread() -> bool {
    let Ok(thread) = class(c"NSThread") else {
        return false;
    };
    // SAFETY: `+[NSThread isMainThread]` takes nothing and returns a `BOOL`,
    // and may be sent on any thread.
    let main = unsafe { msg_send![as_id(thread), sel(c"isMainThread"), () -> BOOL] };
    main != NO
}

/// An object we hold a reference to: one we made with `alloc` and an
/// `init` method, or one we retained. Dropping this releases it.
pub(super) struct Owned(Id);

impl Owned {
    /// Takes over a reference to `object`, which may be null.
    ///
    /// # Safety
    ///
    /// `object` is null, or an object that the caller holds a reference to
    /// and hands over, on the main thread.
    pub(super) unsafe fn new(object: Id) -> Self {
        Self(object)
    }

    /// The object, without a reference of its own.
    pub(super) fn id(&self) -> Id {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: we hold a reference to the object, which this gives up.
        // `-release` takes nothing and returns nothing.
        unsafe { msg_send![self.0, sel(c"release"), () -> ()] };
    }
}

/// Makes an `NSString` of `text`.
pub(super) fn ns_string(text: &str) -> Result<Owned, PlatformError> {
    let class = class(c"NSString")?;
    // SAFETY: `+alloc` takes nothing and returns a new, uninitialized
    // object, which `init…` below takes over.
    let blank = unsafe { msg_send![as_id(class), sel(c"alloc"), () -> Id] };
    let bytes = text.as_ptr().cast::<c_void>();
    // SAFETY: `-[NSString initWithBytes:(const void *)bytes
    // length:(NSUInteger)length encoding:(NSStringEncoding)encoding]`
    // returns the initialized string, holding a reference for us, or nil. It
    // copies the bytes, which are valid UTF-8 for `len()` bytes.
    let string = unsafe {
        msg_send![
            blank,
            sel(c"initWithBytes:length:encoding:"),
            (bytes => *const c_void, text.len() => NSUInteger, NSUTF8StringEncoding => NSUInteger) -> Id
        ]
    };
    if string.is_null() {
        return Err(PlatformError::CallFailed {
            call: "-[NSString initWithBytes:length:encoding:]",
        });
    }
    // SAFETY: the string is new, and its reference is ours.
    Ok(unsafe { Owned::new(string) })
}

/// An autorelease pool for this thread, which releases what was
/// autoreleased in it when it is dropped.
pub(super) struct AutoreleasePool {
    context: *mut c_void,
}

impl AutoreleasePool {
    /// Starts a pool. Pools nest, and must be dropped in the reverse order
    /// of their making, which holding each in a local variable ensures.
    pub(super) fn new() -> Self {
        Self {
            context: objc_autoreleasePoolPush(),
        }
    }
}

impl Drop for AutoreleasePool {
    fn drop(&mut self) {
        // SAFETY: the token came from the push in `new`, on this thread (the
        // pool holds a raw pointer, so it cannot move to another), and pools
        // made later were dropped first.
        unsafe { objc_autoreleasePoolPop(self.context) };
    }
}

/// A method to add to a class: its selector, its implementation, and its
/// type encoding without the `self` and `_cmd` parts.
pub(super) struct Method {
    pub(super) selector: &'static CStr,
    pub(super) imp: Imp,
    /// The result's encoding, then the arguments': `v` for `void`, `@` for
    /// an object, `B`/`c` for `BOOL` ([`BOOL_ENCODING`]), `Q` for
    /// `NSUInteger`.
    pub(super) result: &'static str,
    pub(super) arguments: &'static str,
}

/// Returns the class called `name`, a subclass of `superclass` with a
/// state pointer and these methods, and makes it the first time.
///
/// # Safety
///
/// Each method's `imp` is a function with the signature its encoding
/// describes, taking `self` and `_cmd` first, and sound to call for any
/// instance of the class.
pub(super) unsafe fn build_class(
    name: &CStr,
    superclass: &CStr,
    methods: &[Method],
) -> Result<Class, PlatformError> {
    if let Ok(existing) = class(name) {
        return Ok(existing);
    }
    let superclass = class(superclass)?;
    // SAFETY: the superclass is a class, and the name a C string.
    let new = unsafe { objc_allocateClassPair(superclass, name.as_ptr(), 0) };
    if new.is_null() {
        return Err(PlatformError::CallFailed {
            call: "objc_allocateClassPair",
        });
    }
    let pointer = size_of::<*const c_void>();
    // The alignment is given as its log2.
    let alignment = pointer.trailing_zeros() as u8;
    // SAFETY: the class is being made, and the strings are C strings: the
    // variable is a `void *`.
    let added =
        unsafe { class_addIvar(new, STATE_IVAR.as_ptr(), pointer, alignment, c"^v".as_ptr()) };
    if added == NO {
        return Err(PlatformError::CallFailed {
            call: "class_addIvar",
        });
    }
    for method in methods {
        let types =
            CString::new(format!("{}@:{}", method.result, method.arguments)).map_err(|_| {
                PlatformError::CallFailed {
                    call: "class_addMethod",
                }
            })?;
        // SAFETY: the caller promises that the function has the signature the
        // encoding describes, and the encoding is a C string.
        let added =
            unsafe { class_addMethod(new, sel(method.selector), method.imp, types.as_ptr()) };
        if added == NO {
            return Err(PlatformError::CallFailed {
                call: "class_addMethod",
            });
        }
    }
    // SAFETY: the class came from `objc_allocateClassPair` just now.
    unsafe { objc_registerClassPair(new) };
    Ok(new)
}

/// The encoding of a method that returns a `BOOL`.
pub(super) const RETURNS_BOOL: &str = BOOL_ENCODING;

/// Where the state pointer of `object`'s class is, in bytes from the start
/// of an instance, or `None` if its class has none.
fn state_offset(object: Id) -> Option<usize> {
    if object.is_null() {
        return None;
    }
    // SAFETY: `-class` takes nothing and returns the object's class.
    let class = unsafe { msg_send![object, sel(c"class"), () -> Class] };
    // SAFETY: the class is a class, and the name a C string.
    let ivar = unsafe { class_getInstanceVariable(class, STATE_IVAR.as_ptr()) };
    if ivar.is_null() {
        return None;
    }
    // SAFETY: the variable came from `class_getInstanceVariable`.
    let offset = unsafe { ivar_getOffset(ivar) };
    usize::try_from(offset).ok()
}

/// Points the state variable of `object` at `state`, or at nothing.
///
/// # Safety
///
/// `object` is an instance of a class from [`build_class`], and while the
/// variable points at `state`, `state` is valid for `T`.
pub(super) unsafe fn set_state<T>(object: Id, state: *const T) {
    let Some(offset) = state_offset(object) else {
        return;
    };
    let slot = object
        .cast::<u8>()
        .wrapping_add(offset)
        .cast::<*const c_void>();
    // SAFETY: `build_class` gave the class a pointer-sized, pointer-aligned
    // variable at this offset in every instance, and `object` is one.
    unsafe { slot.write(state.cast()) };
}

/// The state that the state variable of `object` points at, or `None`.
///
/// # Safety
///
/// `object` is an instance of a class from [`build_class`], whose state
/// variable is null or points at a valid `T`, which stays valid while the
/// result is in use.
pub(super) unsafe fn state<'a, T>(object: Id) -> Option<&'a T> {
    let offset = state_offset(object)?;
    let slot = object
        .cast::<u8>()
        .wrapping_add(offset)
        .cast::<*const c_void>();
    // SAFETY: as in `set_state`, the variable is there, and it was set to
    // null or to a pointer.
    let pointer = unsafe { slot.read() };
    // SAFETY: the caller promises that a pointer that is not null is to a
    // valid `T`.
    unsafe { pointer.cast::<T>().as_ref() }
}
