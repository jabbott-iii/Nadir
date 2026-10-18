//! Declarations for macOS: the Objective-C runtime, and the frameworks whose
//! classes the platform uses.

pub mod appkit;
pub mod foundation;
pub mod hitoolbox;
pub mod objc;

#[cfg(test)]
mod tests {
    use core::ffi::CStr;

    use crate::macos::foundation::{CGFloat, NSPoint, NSRect, NSSize};
    use crate::macos::objc::{
        Class, Id, Sel, objc_autoreleasePoolPop, objc_autoreleasePoolPush, objc_getClass,
        objc_object, sel_registerName,
    };
    use crate::msg_send;

    fn class(name: &CStr) -> Id {
        // SAFETY: the name is a C string.
        let class: Class = unsafe { objc_getClass(name.as_ptr()) };
        assert!(!class.is_null(), "{name:?}");
        class.cast::<objc_object>()
    }

    fn sel(name: &CStr) -> Sel {
        // SAFETY: the name is a C string.
        unsafe { sel_registerName(name.as_ptr()) }
    }

    #[test]
    fn a_message_takes_and_returns_a_double() {
        let pool = objc_autoreleasePoolPush();
        // SAFETY: `+[NSNumber numberWithDouble:(double)]` returns an
        // autoreleased number, which the pool keeps while it is used.
        let number = unsafe {
            msg_send![class(c"NSNumber"), sel(c"numberWithDouble:"), (2.5 => CGFloat) -> Id]
        };
        assert!(!number.is_null());
        // SAFETY: `-[NSNumber doubleValue]` takes nothing and returns a
        // `double`.
        let value = unsafe { msg_send![number, sel(c"doubleValue"), () -> CGFloat] };
        assert_eq!(value, 2.5);
        // SAFETY: the token came from the push above, on this thread.
        unsafe { objc_autoreleasePoolPop(pool) };
    }

    #[test]
    fn a_message_takes_and_returns_a_rect() {
        // A rect is 32 bytes, which x86_64 returns through memory: this is
        // the `objc_msgSend_stret` path there, and the plain one on arm64.
        let rect = NSRect {
            origin: NSPoint { x: 1.0, y: 2.0 },
            size: NSSize {
                width: 3.0,
                height: 4.0,
            },
        };
        let pool = objc_autoreleasePoolPush();
        // SAFETY: `+[NSValue valueWithRect:(NSRect)]` returns an autoreleased
        // value, which the pool keeps while it is used.
        let value =
            unsafe { msg_send![class(c"NSValue"), sel(c"valueWithRect:"), (rect => NSRect) -> Id] };
        assert!(!value.is_null());
        // SAFETY: `-[NSValue rectValue]` takes nothing and returns an
        // `NSRect`.
        let back = unsafe { msg_send![value, sel(c"rectValue"), () -> NSRect] };
        assert_eq!(back, rect);
        // SAFETY: as above.
        unsafe { objc_autoreleasePoolPop(pool) };
    }

    #[test]
    fn an_object_is_made_and_released() {
        let object_class = class(c"NSObject");
        // SAFETY: `+alloc` takes nothing and returns a new object.
        let blank = unsafe { msg_send![object_class, sel(c"alloc"), () -> Id] };
        // SAFETY: `-init` takes nothing and returns the object, with a
        // reference for us.
        let object = unsafe { msg_send![blank, sel(c"init"), () -> Id] };
        assert!(!object.is_null());
        // SAFETY: `-class` takes nothing and returns the object's class.
        let its_class = unsafe { msg_send![object, sel(c"class"), () -> Class] };
        assert_eq!(its_class.cast::<objc_object>(), object_class);
        // SAFETY: `-release` gives up our reference, and takes and returns
        // nothing.
        unsafe { msg_send![object, sel(c"release"), () -> ()] };
    }
}
