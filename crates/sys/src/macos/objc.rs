//! The Objective-C runtime (`libobjc.A.dylib`), and
//! [`msg_send!`](crate::msg_send).
//!
//! Sources: `objc/objc.h`, `objc/runtime.h` and `objc/message.h` of the macOS
//! SDK. Each item names its header and quotes the declaration there.
//!
//! # Sending messages
//!
//! `objc_msgSend` has no fixed signature: each method it calls has its own,
//! and the call must use exactly that one. Calling it through a C varargs
//! prototype is undefined behavior on arm64, where variadic arguments travel
//! differently from fixed ones. [`msg_send!`](crate::msg_send) therefore makes every call
//! through a function pointer of the exact type, which the call site spells
//! out.
//!
//! On x86_64 a method that returns a struct of more than 16 bytes, such as
//! `NSRect`, returns it through memory, and must be called through
//! `objc_msgSend_stret`. On arm64 there is no `_stret`. [`MessageArguments`]
//! chooses.

// The types, functions and parameters keep the names their headers give
// them (`objc_msgSend`, `extraBytes`), so that each one can be compared with
// its header.
#![allow(non_camel_case_types, non_snake_case)]

use core::ffi::{c_char, c_void};
use core::marker::{PhantomData, PhantomPinned};

/// An object. Only the runtime looks inside it.
/// `objc/objc.h`: `typedef struct objc_object *id;`
#[repr(C)]
pub struct objc_object {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// A class. `objc/objc.h`: `typedef struct objc_class *Class;`
#[repr(C)]
pub struct objc_class {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// A method name, as the runtime registered it.
/// `objc/objc.h`: `typedef struct objc_selector *SEL;`
#[repr(C)]
pub struct objc_selector {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// An instance variable of a class.
/// `objc/runtime.h`: `typedef struct objc_ivar *Ivar;`
#[repr(C)]
pub struct objc_ivar {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// An object, or null (`nil`). `objc/objc.h`: `typedef struct objc_object
/// *id;`
pub type Id = *mut objc_object;

/// A class, or null (`Nil`). `objc/objc.h`: `typedef struct objc_class
/// *Class;`
pub type Class = *mut objc_class;

/// A selector. `objc/objc.h`: `typedef struct objc_selector *SEL;`
pub type Sel = *mut objc_selector;

/// An instance variable. `objc/runtime.h`: `typedef struct objc_ivar *Ivar;`
pub type Ivar = *mut objc_ivar;

/// A method's implementation, typed as no function in particular. It is cast
/// to the method's real type to be called, and from it to be stored.
/// `objc/objc.h`: `typedef void (*IMP)(void /* id, SEL, ... */ );` (with
/// `OBJC_OLD_DISPATCH_PROTOTYPES` off, the SDK's default)
pub type Imp = unsafe extern "C" fn();

/// A truth value. On arm64 it is C's `bool`.
/// `objc/objc.h`: `typedef bool BOOL;` (when `OBJC_BOOL_IS_BOOL`, which the
/// header sets for arm64)
#[cfg(target_arch = "aarch64")]
pub type BOOL = bool;

/// A truth value. On x86_64 it is a `signed char`.
/// `objc/objc.h`: `typedef signed char BOOL;` (when `OBJC_BOOL_IS_CHAR`)
#[cfg(target_arch = "x86_64")]
pub type BOOL = i8;

/// `objc/objc.h`: `#define YES __objc_yes`, which is `((BOOL)1)`
#[cfg(target_arch = "aarch64")]
pub const YES: BOOL = true;

/// `objc/objc.h`: `#define NO __objc_no`, which is `((BOOL)0)`
#[cfg(target_arch = "aarch64")]
pub const NO: BOOL = false;

/// `objc/objc.h`: `#define YES __objc_yes`, which is `((BOOL)1)`
#[cfg(target_arch = "x86_64")]
pub const YES: BOOL = 1;

/// `objc/objc.h`: `#define NO __objc_no`, which is `((BOOL)0)`
#[cfg(target_arch = "x86_64")]
pub const NO: BOOL = 0;

/// The type encoding of [`BOOL`], for the type strings of
/// `class_addMethod`: `B` for `bool` on arm64. (`@encode(BOOL)`)
#[cfg(target_arch = "aarch64")]
pub const BOOL_ENCODING: &str = "B";

/// The type encoding of [`BOOL`]: `c` for `signed char` on x86_64.
/// (`@encode(BOOL)`)
#[cfg(target_arch = "x86_64")]
pub const BOOL_ENCODING: &str = "c";

#[link(name = "objc")]
unsafe extern "C" {
    /// Returns the class called `name`, or null if no loaded image defines
    /// one. `objc/runtime.h`: `Class objc_getClass(const char *name)`
    ///
    /// # Safety
    ///
    /// `name` ends with a NUL.
    pub fn objc_getClass(name: *const c_char) -> Class;

    /// Returns the selector for a method name, registering the name if it is
    /// new. Selectors live as long as the process.
    /// `objc/runtime.h`: `SEL sel_registerName(const char *str)`
    ///
    /// # Safety
    ///
    /// `str` ends with a NUL.
    pub fn sel_registerName(str: *const c_char) -> Sel;

    /// Sends a message. Declared without parameters, because it has no
    /// signature of its own: call it only through
    /// [`msg_send!`](crate::msg_send), which casts it to the method's.
    /// `objc/message.h`: `void objc_msgSend(void /* id self, SEL op, ... */ )`
    pub fn objc_msgSend();

    /// Sends a message to a method that returns a struct through memory, on
    /// x86_64. Call it only through [`msg_send!`](crate::msg_send).
    /// `objc/message.h`: `void objc_msgSend_stret(void /* id self, SEL op,
    /// ... */ )`
    #[cfg(target_arch = "x86_64")]
    pub fn objc_msgSend_stret();

    /// Starts making a class: a subclass of `superclass` called `name`, with
    /// `extraBytes` (normally 0) more in each instance. Returns null if a
    /// class of that name exists.
    /// `objc/runtime.h`: `Class objc_allocateClassPair(Class superclass, const
    /// char *name, size_t extraBytes)`
    ///
    /// # Safety
    ///
    /// `superclass` is a class, and `name` ends with a NUL.
    pub fn objc_allocateClassPair(
        superclass: Class,
        name: *const c_char,
        extraBytes: usize,
    ) -> Class;

    /// Finishes a class that `objc_allocateClassPair` started, so that it
    /// can be used. `objc/runtime.h`: `void objc_registerClassPair(Class cls)`
    ///
    /// # Safety
    ///
    /// `cls` came from `objc_allocateClassPair` and is not registered yet.
    pub fn objc_registerClassPair(cls: Class);

    /// Adds a method to a class that is being made. Returns `NO` if the class
    /// has one of that name already. `objc/runtime.h`: `BOOL
    /// class_addMethod(Class cls, SEL name, IMP imp, const char *types)`
    ///
    /// # Safety
    ///
    /// `imp` is a function with the signature that `types` describes, which
    /// ends with a NUL: the return type, then `self`, `_cmd` and the
    /// arguments.
    pub fn class_addMethod(cls: Class, name: Sel, imp: Imp, types: *const c_char) -> BOOL;

    /// Adds an instance variable to a class that is being made, before it is
    /// registered. `alignment` is the log2 of the alignment in bytes.
    /// Returns `NO` on failure. `objc/runtime.h`: `BOOL class_addIvar(Class
    /// cls, const char *name, size_t size, uint8_t alignment, const char
    /// *types)`
    ///
    /// # Safety
    ///
    /// `cls` came from `objc_allocateClassPair` and is not registered yet,
    /// and the strings end with a NUL.
    pub fn class_addIvar(
        cls: Class,
        name: *const c_char,
        size: usize,
        alignment: u8,
        types: *const c_char,
    ) -> BOOL;

    /// Returns the instance variable of a class called `name`, or null.
    /// `objc/runtime.h`: `Ivar class_getInstanceVariable(Class cls, const char
    /// *name)`
    ///
    /// # Safety
    ///
    /// `cls` is a class, and `name` ends with a NUL.
    pub fn class_getInstanceVariable(cls: Class, name: *const c_char) -> Ivar;

    /// Returns where an instance variable is, in bytes from the start of an
    /// instance. `objc/runtime.h`: `ptrdiff_t ivar_getOffset(Ivar v)`
    ///
    /// # Safety
    ///
    /// `v` came from `class_getInstanceVariable`.
    pub fn ivar_getOffset(v: Ivar) -> isize;

    /// Starts an autorelease pool on this thread, and returns a token for
    /// the matching pop. Not in the SDK's public headers: libobjc exports it,
    /// and Clang emits calls to it for every `@autoreleasepool` block
    /// (Clang's "Objective-C Automatic Reference Counting", section
    /// "Autorelease pools"). Runtime source, `NSObject.mm`: `void
    /// *objc_autoreleasePoolPush(void)`
    pub safe fn objc_autoreleasePoolPush() -> *mut c_void;

    /// Releases every object that was autoreleased since the matching push,
    /// and ends the pool. Exported like the push; runtime source,
    /// `NSObject.mm`: `void objc_autoreleasePoolPop(void *ctxt)`
    ///
    /// # Safety
    ///
    /// `context` came from `objc_autoreleasePoolPush` on this thread, and
    /// pools are popped in the reverse order of their pushes.
    pub fn objc_autoreleasePoolPop(context: *mut c_void);
}

/// The arguments of a message, as a tuple, and how to send a message with
/// them. [`msg_send!`](crate::msg_send) uses it; there is no need to name it.
pub trait MessageArguments: Sized {
    /// Sends `selector` to `receiver` with these arguments, through
    /// `objc_msgSend` cast to `R (*)(id, SEL, arguments...)`, or through
    /// `objc_msgSend_stret` when x86_64 returns an `R` through memory.
    ///
    /// # Safety
    ///
    /// - `receiver` is an object or a class, or null (which returns zero).
    /// - Its method for `selector` takes exactly these argument types and
    ///   returns exactly `R`, as C sees them.
    /// - What the method itself requires of its arguments holds.
    unsafe fn send<R>(receiver: Id, selector: Sel, arguments: Self) -> R;
}

/// The function that sends a message returning `R`: `objc_msgSend`, or on
/// x86_64 `objc_msgSend_stret` for a result of more than 16 bytes, which
/// that system's C convention returns through memory. (A `long double`
/// would need `objc_msgSend_fpret`; nothing returns one here.)
fn sender<R>() -> Imp {
    // A `#[cfg]` here would leave `R` unused on arm64, which clippy rejects
    // (`extra_unused_type_parameters`), so `STRET_SENDER` makes the choice
    // of system instead.
    match STRET_SENDER {
        Some(stret) if size_of::<R>() > 16 => stret,
        _ => objc_msgSend,
    }
}

/// `objc_msgSend_stret`, for [`sender`] on x86_64.
#[cfg(target_arch = "x86_64")]
const STRET_SENDER: Option<Imp> = Some(objc_msgSend_stret);

/// None: arm64 has no `_stret`, and `objc_msgSend` sends every message
/// there, whatever its result.
#[cfg(not(target_arch = "x86_64"))]
const STRET_SENDER: Option<Imp> = None;

/// Implements [`MessageArguments`] for tuples of these types.
macro_rules! message_arguments {
    ($($name:ident: $type:ident),*) => {
        impl<$($type),*> MessageArguments for ($($type,)*) {
            unsafe fn send<R>(receiver: Id, selector: Sel, arguments: Self) -> R {
                let ($($name,)*) = arguments;
                // SAFETY: all function pointers have the same size and
                // representation; only calling one through the wrong type is
                // undefined behavior, and the caller promises the type.
                let send = unsafe {
                    core::mem::transmute::<Imp, unsafe extern "C" fn(Id, Sel $(, $type)*) -> R>(
                        sender::<R>(),
                    )
                };
                // SAFETY: the caller promises that the method takes these
                // arguments and returns `R`, and `sender` picked the entry
                // point that the C convention for `R` needs.
                unsafe { send(receiver, selector $(, $name)*) }
            }
        }
    };
}

message_arguments!();
message_arguments!(a: A);
message_arguments!(a: A, b: B);
message_arguments!(a: A, b: B, c: C);
message_arguments!(a: A, b: B, c: C, d: D);

/// Sends a message, through `objc_msgSend` cast to the exact signature that
/// the call spells out. It must be used in an `unsafe` block, under the
/// rules of [`MessageArguments::send`].
///
/// ```text
/// // -[NSWindow setTitle:(NSString *)title]
/// msg_send![window, selectors.set_title, (title => Id) -> ()];
/// // -[NSView bounds] -> NSRect
/// let bounds = msg_send![view, selectors.bounds, () -> NSRect];
/// ```
///
/// Each argument is written with its type, and the result type is always
/// given, so that nothing about the signature is left to inference.
#[macro_export]
macro_rules! msg_send {
    ($receiver:expr, $selector:expr, ($($argument:expr => $type:ty),* $(,)?) -> $result:ty) => {
        <($($type,)*) as $crate::macos::objc::MessageArguments>::send::<$result>(
            $receiver,
            $selector,
            ($($argument,)*),
        )
    };
}
