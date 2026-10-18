//! AppKit: the constants of the classes we use (`AppKit.framework`).
//!
//! AppKit is all classes and messages, sent with [`msg_send!`](crate::msg_send),
//! so what is declared here is the numbers those messages take and return.
//! Sources: `AppKit/NSApplication.h`, `AppKit/NSWindow.h`,
//! `AppKit/NSGraphics.h`, `AppKit/NSEvent.h` and `AppKit/NSAlert.h` of the
//! macOS SDK, and `IOKit/hidsystem/IOLLEvent.h` for the device bits of the
//! modifier flags. Each item names its header and quotes the declaration
//! there.
//!
//! Every AppKit message is sent on the main thread.

// The constants keep the names their headers give them
// (`NSEventTypeKeyDown`), so that each one can be compared with its header.
#![allow(non_upper_case_globals)]

use super::foundation::{NSInteger, NSUInteger};

// The classes are looked up by name at run time, so nothing calls into the
// framework directly. Linking it is what loads it, and its classes with it.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

// ---------------------------------------------------------------------------
// The application
// ---------------------------------------------------------------------------

/// An ordinary app, with a Dock icon and a menu bar, for
/// `-[NSApplication setActivationPolicy:]`.
/// `AppKit/NSRunningApplication.h`: `NSApplicationActivationPolicyRegular,`
/// (the first of `NS_ENUM(NSInteger, NSApplicationActivationPolicy)`)
pub const NSApplicationActivationPolicyRegular: NSInteger = 0;

/// What `-applicationShouldTerminate:` returns to keep the app running.
/// `AppKit/NSApplication.h`: `NSTerminateCancel = 0,` (in `NS_ENUM(NSUInteger,
/// NSApplicationTerminateReply)`)
pub const NSTerminateCancel: NSUInteger = 0;

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// A window style: a title bar. `AppKit/NSWindow.h`:
/// `NSWindowStyleMaskTitled = 1 << 0,` (in `NS_OPTIONS(NSUInteger,
/// NSWindowStyleMask)`)
pub const NSWindowStyleMaskTitled: NSUInteger = 1 << 0;

/// A window style: a close button. `AppKit/NSWindow.h`:
/// `NSWindowStyleMaskClosable = 1 << 1,`
pub const NSWindowStyleMaskClosable: NSUInteger = 1 << 1;

/// A window style: a minimize button. `AppKit/NSWindow.h`:
/// `NSWindowStyleMaskMiniaturizable = 1 << 2,`
pub const NSWindowStyleMaskMiniaturizable: NSUInteger = 1 << 2;

/// A window style: the user can resize it. `AppKit/NSWindow.h`:
/// `NSWindowStyleMaskResizable = 1 << 3,`
pub const NSWindowStyleMaskResizable: NSUInteger = 1 << 3;

/// The window keeps its contents in a buffer of its own, for
/// `-[NSWindow initWithContentRect:styleMask:backing:defer:]`.
/// `AppKit/NSGraphics.h`: `NSBackingStoreBuffered = 2` (in
/// `NS_ENUM(NSUInteger, NSBackingStoreType)`)
pub const NSBackingStoreBuffered: NSUInteger = 2;

/// The window can go fullscreen in a Space of its own, even if it may not
/// be resized. `AppKit/NSWindow.h`:
/// `NSWindowCollectionBehaviorFullScreenPrimary = 1 << 7,` (in
/// `NS_OPTIONS(NSUInteger, NSWindowCollectionBehavior)`)
pub const NSWindowCollectionBehaviorFullScreenPrimary: NSUInteger = 1 << 7;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Every kind of event, for `-[NSApplication
/// nextEventMatchingMask:untilDate:inMode:dequeue:]`.
/// `AppKit/NSEvent.h`: `NSEventMaskAny = NSUIntegerMax,` (in
/// `NS_OPTIONS(unsigned long long, NSEventMask)`)
pub const NSEventMaskAny: u64 = u64::MAX;

/// `AppKit/NSEvent.h`: `NSEventTypeLeftMouseDown = 1,` (in
/// `NS_ENUM(NSUInteger, NSEventType)`)
pub const NSEventTypeLeftMouseDown: NSUInteger = 1;

/// `AppKit/NSEvent.h`: `NSEventTypeLeftMouseUp = 2,`
pub const NSEventTypeLeftMouseUp: NSUInteger = 2;

/// `AppKit/NSEvent.h`: `NSEventTypeRightMouseDown = 3,`
pub const NSEventTypeRightMouseDown: NSUInteger = 3;

/// `AppKit/NSEvent.h`: `NSEventTypeRightMouseUp = 4,`
pub const NSEventTypeRightMouseUp: NSUInteger = 4;

/// The pointer moved with no button down. `AppKit/NSEvent.h`:
/// `NSEventTypeMouseMoved = 5,`
pub const NSEventTypeMouseMoved: NSUInteger = 5;

/// `AppKit/NSEvent.h`: `NSEventTypeLeftMouseDragged = 6,`
pub const NSEventTypeLeftMouseDragged: NSUInteger = 6;

/// `AppKit/NSEvent.h`: `NSEventTypeRightMouseDragged = 7,`
pub const NSEventTypeRightMouseDragged: NSUInteger = 7;

/// `AppKit/NSEvent.h`: `NSEventTypeKeyDown = 10,`
pub const NSEventTypeKeyDown: NSUInteger = 10;

/// `AppKit/NSEvent.h`: `NSEventTypeKeyUp = 11,`
pub const NSEventTypeKeyUp: NSUInteger = 11;

/// A modifier key went down or up, which no key event reports.
/// `AppKit/NSEvent.h`: `NSEventTypeFlagsChanged = 12,`
pub const NSEventTypeFlagsChanged: NSUInteger = 12;

/// `AppKit/NSEvent.h`: `NSEventTypeScrollWheel = 22,`
pub const NSEventTypeScrollWheel: NSUInteger = 22;

/// A button other than the left and right went down; `buttonNumber` says
/// which. `AppKit/NSEvent.h`: `NSEventTypeOtherMouseDown = 25,`
pub const NSEventTypeOtherMouseDown: NSUInteger = 25;

/// `AppKit/NSEvent.h`: `NSEventTypeOtherMouseUp = 26,`
pub const NSEventTypeOtherMouseUp: NSUInteger = 26;

/// `AppKit/NSEvent.h`: `NSEventTypeOtherMouseDragged = 27,`
pub const NSEventTypeOtherMouseDragged: NSUInteger = 27;

/// Caps Lock is on. `AppKit/NSEvent.h`: `NSEventModifierFlagCapsLock = 1 <<
/// 16,` (in `NS_OPTIONS(NSUInteger, NSEventModifierFlags)`)
pub const NSEventModifierFlagCapsLock: NSUInteger = 1 << 16;

/// A Shift key is down. `AppKit/NSEvent.h`: `NSEventModifierFlagShift = 1 <<
/// 17,`
pub const NSEventModifierFlagShift: NSUInteger = 1 << 17;

/// A Control key is down. `AppKit/NSEvent.h`: `NSEventModifierFlagControl =
/// 1 << 18,`
pub const NSEventModifierFlagControl: NSUInteger = 1 << 18;

/// An Option key is down. `AppKit/NSEvent.h`: `NSEventModifierFlagOption = 1
/// << 19,`
pub const NSEventModifierFlagOption: NSUInteger = 1 << 19;

/// A Command key is down. `AppKit/NSEvent.h`: `NSEventModifierFlagCommand =
/// 1 << 20,`
pub const NSEventModifierFlagCommand: NSUInteger = 1 << 20;

// The low bits of the modifier flags say which side's key is down. AppKit
// passes them through from the event system without naming them.

/// The left Control key is down. `IOKit/hidsystem/IOLLEvent.h`: `#define
/// NX_DEVICELCTLKEYMASK 0x00000001`
pub const NX_DEVICELCTLKEYMASK: NSUInteger = 0x0000_0001;

/// The left Shift key is down. `IOKit/hidsystem/IOLLEvent.h`: `#define
/// NX_DEVICELSHIFTKEYMASK 0x00000002`
pub const NX_DEVICELSHIFTKEYMASK: NSUInteger = 0x0000_0002;

/// The right Shift key is down. `IOKit/hidsystem/IOLLEvent.h`: `#define
/// NX_DEVICERSHIFTKEYMASK 0x00000004`
pub const NX_DEVICERSHIFTKEYMASK: NSUInteger = 0x0000_0004;

/// The left Command key is down. `IOKit/hidsystem/IOLLEvent.h`: `#define
/// NX_DEVICELCMDKEYMASK 0x00000008`
pub const NX_DEVICELCMDKEYMASK: NSUInteger = 0x0000_0008;

/// The right Command key is down. `IOKit/hidsystem/IOLLEvent.h`: `#define
/// NX_DEVICERCMDKEYMASK 0x00000010`
pub const NX_DEVICERCMDKEYMASK: NSUInteger = 0x0000_0010;

/// The left Option key is down. `IOKit/hidsystem/IOLLEvent.h`: `#define
/// NX_DEVICELALTKEYMASK 0x00000020`
pub const NX_DEVICELALTKEYMASK: NSUInteger = 0x0000_0020;

/// The right Option key is down. `IOKit/hidsystem/IOLLEvent.h`: `#define
/// NX_DEVICERALTKEYMASK 0x00000040`
pub const NX_DEVICERALTKEYMASK: NSUInteger = 0x0000_0040;

/// The right Control key is down. `IOKit/hidsystem/IOLLEvent.h`: `#define
/// NX_DEVICERCTLKEYMASK 0x00002000`
pub const NX_DEVICERCTLKEYMASK: NSUInteger = 0x0000_2000;

// ---------------------------------------------------------------------------
// Alerts
// ---------------------------------------------------------------------------

/// An alert about a serious problem. `AppKit/NSAlert.h`:
/// `NSAlertStyleCritical = 2,` (in `NS_ENUM(NSUInteger, NSAlertStyle)`)
pub const NSAlertStyleCritical: NSUInteger = 2;
