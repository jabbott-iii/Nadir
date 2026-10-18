//! Foundation: integers, geometry, strings and run-loop modes
//! (`Foundation.framework`).
//!
//! Sources: `objc/NSObjCRuntime.h` and `CoreGraphics/CGBase.h` for the
//! integers, `CoreGraphics/CGGeometry.h` and `Foundation/NSGeometry.h` for
//! the geometry, `Foundation/NSString.h` and `Foundation/NSRunLoop.h` of the
//! macOS SDK. Each item names its header and quotes the declaration there.
//!
//! # Layouts
//!
//! The sizes and alignments asserted here are those of 64-bit macOS, worked
//! out from the declarations: `CGFloat` is a `double` there. They were not
//! read off the Rust types. Verify on macOS: this session had no SDK
//! headers.

// The constants and statics keep the names their headers give them
// (`NSUTF8StringEncoding`), so that each one can be compared with its header.
#![allow(non_upper_case_globals)]

use core::mem::offset_of;

use super::objc::Id;

/// A signed integer of the size of a pointer.
/// `objc/NSObjCRuntime.h`: `typedef long NSInteger;` (64-bit)
pub type NSInteger = isize;

/// An unsigned integer of the size of a pointer.
/// `objc/NSObjCRuntime.h`: `typedef unsigned long NSUInteger;` (64-bit)
pub type NSUInteger = usize;

/// A coordinate or a length, in points.
/// `CoreGraphics/CGBase.h`: `typedef double CGFloat;` (with `CGFLOAT_IS_DOUBLE`,
/// on 64-bit)
pub type CGFloat = f64;

/// A point. `CoreGraphics/CGGeometry.h`: `struct CGPoint { CGFloat x; CGFloat
/// y; };`, and `Foundation/NSGeometry.h`: `typedef CGPoint NSPoint;`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NSPoint {
    /// Points to the right.
    pub x: CGFloat,
    /// Points up, unless the view is flipped.
    pub y: CGFloat,
}

const _: () = assert!(size_of::<NSPoint>() == 16);
const _: () = assert!(align_of::<NSPoint>() == 8);

/// A size. `CoreGraphics/CGGeometry.h`: `struct CGSize { CGFloat width;
/// CGFloat height; };`, and `Foundation/NSGeometry.h`: `typedef CGSize
/// NSSize;`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NSSize {
    /// The width, in points.
    pub width: CGFloat,
    /// The height, in points.
    pub height: CGFloat,
}

const _: () = assert!(size_of::<NSSize>() == 16);
const _: () = assert!(align_of::<NSSize>() == 8);

/// A rectangle. `CoreGraphics/CGGeometry.h`: `struct CGRect { CGPoint origin;
/// CGSize size; };`, and `Foundation/NSGeometry.h`: `typedef CGRect NSRect;`
///
/// At 32 bytes it is returned through memory on x86_64, so a method that
/// returns one needs `objc_msgSend_stret` there.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NSRect {
    /// The corner nearest the origin of the coordinate system.
    pub origin: NSPoint,
    /// The width and height.
    pub size: NSSize,
}

const _: () = assert!(size_of::<NSRect>() == 32);
const _: () = assert!(align_of::<NSRect>() == 8);
const _: () = assert!(offset_of!(NSRect, size) == 16);

/// UTF-8, for `-[NSString initWithBytes:length:encoding:]`.
/// `Foundation/NSString.h`: `NSUTF8StringEncoding = 4,` (in `enum`, of type
/// `NSStringEncoding`, which is `NSUInteger`)
pub const NSUTF8StringEncoding: NSUInteger = 4;

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    /// The run-loop mode of ordinary event handling, an `NSString`.
    /// `Foundation/NSRunLoop.h`: `FOUNDATION_EXPORT NSRunLoopMode const
    /// NSDefaultRunLoopMode;` (`NSRunLoopMode` is `NSString *`)
    pub static NSDefaultRunLoopMode: Id;
}
