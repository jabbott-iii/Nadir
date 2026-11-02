// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Error alerts.

use nadir_sys::macos::appkit::{NSAlertStyleCritical, NSApplicationActivationPolicyRegular};
use nadir_sys::macos::foundation::{NSInteger, NSUInteger};
use nadir_sys::macos::objc::{BOOL, Id, YES};
use nadir_sys::msg_send;

use super::runtime::{self, AutoreleasePool, Owned, as_id, class, ns_string, sel};
use crate::PlatformError;

/// Shows an error in an alert, and waits until the player closes it. It
/// shows nothing off the main thread, where AppKit may not be used.
pub(crate) fn alert(title: &str, message: &str) {
    let _pool = AutoreleasePool::new();
    if !runtime::is_main_thread() {
        return;
    }
    // A failure has nowhere to be reported but where the message also went:
    // stderr, written before this.
    let _ = show(title, message);
}

fn show(title: &str, message: &str) -> Result<(), PlatformError> {
    // SAFETY: `+[NSApplication sharedApplication]` takes nothing and returns
    // the application, making it the first time.
    let app =
        unsafe { msg_send![as_id(class(c"NSApplication")?), sel(c"sharedApplication"), () -> Id] };
    // An error can come before the window, from a program started in a
    // terminal, which is no app yet: the alert would not come to the front.
    // SAFETY: `-setActivationPolicy:(NSApplicationActivationPolicy)` returns
    // a `BOOL`.
    unsafe {
        msg_send![
            app,
            sel(c"setActivationPolicy:"),
            (NSApplicationActivationPolicyRegular => NSInteger) -> BOOL
        ]
    };
    // SAFETY: `-activateIgnoringOtherApps:(BOOL)` returns nothing.
    unsafe { msg_send![app, sel(c"activateIgnoringOtherApps:"), (YES => BOOL) -> ()] };

    let (title, message) = (ns_string(title)?, ns_string(message)?);
    let alert_class = as_id(class(c"NSAlert")?);
    // SAFETY: `+alloc` takes nothing and returns a new, uninitialized
    // object.
    let blank = unsafe { msg_send![alert_class, sel(c"alloc"), () -> Id] };
    // SAFETY: `-init` takes nothing and returns the alert, holding a
    // reference for us.
    let alert = unsafe { msg_send![blank, sel(c"init"), () -> Id] };
    // SAFETY: the alert is new, and its reference is ours.
    let alert = unsafe { Owned::new(alert) };
    if alert.id().is_null() {
        return Err(PlatformError::CallFailed {
            call: "-[NSAlert init]",
        });
    }
    // SAFETY: `-setMessageText:(NSString *)` copies the string and returns
    // nothing.
    unsafe { msg_send![alert.id(), sel(c"setMessageText:"), (title.id() => Id) -> ()] };
    // SAFETY: `-setInformativeText:(NSString *)` copies the string and
    // returns nothing.
    unsafe { msg_send![alert.id(), sel(c"setInformativeText:"), (message.id() => Id) -> ()] };
    // SAFETY: `-setAlertStyle:(NSAlertStyle)` returns nothing.
    unsafe {
        msg_send![alert.id(), sel(c"setAlertStyle:"), (NSAlertStyleCritical => NSUInteger) -> ()]
    };
    // SAFETY: `-runModal` takes nothing, waits for the player, and returns
    // the button chosen, an `NSModalResponse`, which is an `NSInteger`.
    unsafe { msg_send![alert.id(), sel(c"runModal"), () -> NSInteger] };
    Ok(())
}
