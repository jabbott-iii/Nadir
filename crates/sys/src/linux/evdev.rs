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

//! Linux input devices (evdev): the kernel's numbers for keys, a gamepad's
//! buttons and axes, the records read from `/dev/input/event*`, and the
//! `ioctl` requests that describe a device.
//!
//! Sources: `<linux/input-event-codes.h>` for the codes, `<linux/input.h>`
//! for the structs and the requests, and `<asm-generic/ioctl.h>` for how a
//! request is numbered, which x86_64 and aarch64 both use. Each item quotes
//! its `#define` or its declaration.
//!
//! Only the codes that `nadir-platform` maps to a key or reads from a
//! gamepad are declared, in the header's order. A code's type is that of the
//! `code` field of the kernel's `struct input_event`, `__u16`.
//!
//! The key codes are also what an X server reports, shifted: on a server
//! whose keyboard rules are `evdev`, which is every current Linux desktop and
//! XWayland, an X keycode is one of these codes plus 8.
//!
//! The requests whose size is an argument of their macro (`EVIOCGBIT`,
//! `EVIOCGKEY`, `EVIOCGNAME`) or whose number is (`EVIOCGABS`) are declared
//! for the arguments that are used, each named after its macro and argument.
//! `_IOC(dir, type, nr, size)` is `dir << 30 | size << 16 | type << 8 | nr`,
//! with `_IOC_READ` 2.

use core::ffi::c_ulong;
use core::mem::offset_of;

use super::libc::timeval;

/// Event type: a marker between groups of events, such as the end of one
/// report of the device's state.
/// `linux/input-event-codes.h`: `#define EV_SYN 0x00`
pub const EV_SYN: u16 = 0x00;

/// Event type: a key or button went down (value 1), up (0), or repeated (2).
/// `linux/input-event-codes.h`: `#define EV_KEY 0x01`
pub const EV_KEY: u16 = 0x01;

/// Event type: an absolute axis has a new value.
/// `linux/input-event-codes.h`: `#define EV_ABS 0x03`
pub const EV_ABS: u16 = 0x03;

/// The number of event types: [`EVIOCGBIT_EV`] reports one bit for each.
/// `linux/input-event-codes.h`: `#define EV_MAX 0x1f` and `#define EV_CNT
/// (EV_MAX+1)`
pub const EV_CNT: u16 = 0x20;

/// An [`EV_SYN`] code: the end of a report. The events since the last one
/// happened at once.
/// `linux/input-event-codes.h`: `#define SYN_REPORT 0`
pub const SYN_REPORT: u16 = 0;

/// An [`EV_SYN`] code: the kernel's buffer of the device's events overflowed,
/// and events were lost. The events up to the next [`SYN_REPORT`] are to be
/// dropped, and the state read again with `ioctl`.
/// `linux/input-event-codes.h`: `#define SYN_DROPPED 3`
pub const SYN_DROPPED: u16 = 3;

/// `linux/input-event-codes.h`: `#define KEY_ESC 1`
pub const KEY_ESC: u16 = 1;

/// `linux/input-event-codes.h`: `#define KEY_1 2`
pub const KEY_1: u16 = 2;

/// `linux/input-event-codes.h`: `#define KEY_2 3`
pub const KEY_2: u16 = 3;

/// `linux/input-event-codes.h`: `#define KEY_3 4`
pub const KEY_3: u16 = 4;

/// `linux/input-event-codes.h`: `#define KEY_4 5`
pub const KEY_4: u16 = 5;

/// `linux/input-event-codes.h`: `#define KEY_5 6`
pub const KEY_5: u16 = 6;

/// `linux/input-event-codes.h`: `#define KEY_6 7`
pub const KEY_6: u16 = 7;

/// `linux/input-event-codes.h`: `#define KEY_7 8`
pub const KEY_7: u16 = 8;

/// `linux/input-event-codes.h`: `#define KEY_8 9`
pub const KEY_8: u16 = 9;

/// `linux/input-event-codes.h`: `#define KEY_9 10`
pub const KEY_9: u16 = 10;

/// `linux/input-event-codes.h`: `#define KEY_0 11`
pub const KEY_0: u16 = 11;

/// `linux/input-event-codes.h`: `#define KEY_MINUS 12`
pub const KEY_MINUS: u16 = 12;

/// `linux/input-event-codes.h`: `#define KEY_EQUAL 13`
pub const KEY_EQUAL: u16 = 13;

/// `linux/input-event-codes.h`: `#define KEY_BACKSPACE 14`
pub const KEY_BACKSPACE: u16 = 14;

/// `linux/input-event-codes.h`: `#define KEY_TAB 15`
pub const KEY_TAB: u16 = 15;

/// `linux/input-event-codes.h`: `#define KEY_Q 16`
pub const KEY_Q: u16 = 16;

/// `linux/input-event-codes.h`: `#define KEY_W 17`
pub const KEY_W: u16 = 17;

/// `linux/input-event-codes.h`: `#define KEY_E 18`
pub const KEY_E: u16 = 18;

/// `linux/input-event-codes.h`: `#define KEY_R 19`
pub const KEY_R: u16 = 19;

/// `linux/input-event-codes.h`: `#define KEY_T 20`
pub const KEY_T: u16 = 20;

/// `linux/input-event-codes.h`: `#define KEY_Y 21`
pub const KEY_Y: u16 = 21;

/// `linux/input-event-codes.h`: `#define KEY_U 22`
pub const KEY_U: u16 = 22;

/// `linux/input-event-codes.h`: `#define KEY_I 23`
pub const KEY_I: u16 = 23;

/// `linux/input-event-codes.h`: `#define KEY_O 24`
pub const KEY_O: u16 = 24;

/// `linux/input-event-codes.h`: `#define KEY_P 25`
pub const KEY_P: u16 = 25;

/// `linux/input-event-codes.h`: `#define KEY_LEFTBRACE 26`
pub const KEY_LEFTBRACE: u16 = 26;

/// `linux/input-event-codes.h`: `#define KEY_RIGHTBRACE 27`
pub const KEY_RIGHTBRACE: u16 = 27;

/// `linux/input-event-codes.h`: `#define KEY_ENTER 28`
pub const KEY_ENTER: u16 = 28;

/// `linux/input-event-codes.h`: `#define KEY_LEFTCTRL 29`
pub const KEY_LEFTCTRL: u16 = 29;

/// `linux/input-event-codes.h`: `#define KEY_A 30`
pub const KEY_A: u16 = 30;

/// `linux/input-event-codes.h`: `#define KEY_S 31`
pub const KEY_S: u16 = 31;

/// `linux/input-event-codes.h`: `#define KEY_D 32`
pub const KEY_D: u16 = 32;

/// `linux/input-event-codes.h`: `#define KEY_F 33`
pub const KEY_F: u16 = 33;

/// `linux/input-event-codes.h`: `#define KEY_G 34`
pub const KEY_G: u16 = 34;

/// `linux/input-event-codes.h`: `#define KEY_H 35`
pub const KEY_H: u16 = 35;

/// `linux/input-event-codes.h`: `#define KEY_J 36`
pub const KEY_J: u16 = 36;

/// `linux/input-event-codes.h`: `#define KEY_K 37`
pub const KEY_K: u16 = 37;

/// `linux/input-event-codes.h`: `#define KEY_L 38`
pub const KEY_L: u16 = 38;

/// `linux/input-event-codes.h`: `#define KEY_SEMICOLON 39`
pub const KEY_SEMICOLON: u16 = 39;

/// `linux/input-event-codes.h`: `#define KEY_APOSTROPHE 40`
pub const KEY_APOSTROPHE: u16 = 40;

/// `linux/input-event-codes.h`: `#define KEY_GRAVE 41`
pub const KEY_GRAVE: u16 = 41;

/// `linux/input-event-codes.h`: `#define KEY_LEFTSHIFT 42`
pub const KEY_LEFTSHIFT: u16 = 42;

/// `linux/input-event-codes.h`: `#define KEY_BACKSLASH 43`
pub const KEY_BACKSLASH: u16 = 43;

/// `linux/input-event-codes.h`: `#define KEY_Z 44`
pub const KEY_Z: u16 = 44;

/// `linux/input-event-codes.h`: `#define KEY_X 45`
pub const KEY_X: u16 = 45;

/// `linux/input-event-codes.h`: `#define KEY_C 46`
pub const KEY_C: u16 = 46;

/// `linux/input-event-codes.h`: `#define KEY_V 47`
pub const KEY_V: u16 = 47;

/// `linux/input-event-codes.h`: `#define KEY_B 48`
pub const KEY_B: u16 = 48;

/// `linux/input-event-codes.h`: `#define KEY_N 49`
pub const KEY_N: u16 = 49;

/// `linux/input-event-codes.h`: `#define KEY_M 50`
pub const KEY_M: u16 = 50;

/// `linux/input-event-codes.h`: `#define KEY_COMMA 51`
pub const KEY_COMMA: u16 = 51;

/// `linux/input-event-codes.h`: `#define KEY_DOT 52`
pub const KEY_DOT: u16 = 52;

/// `linux/input-event-codes.h`: `#define KEY_SLASH 53`
pub const KEY_SLASH: u16 = 53;

/// `linux/input-event-codes.h`: `#define KEY_RIGHTSHIFT 54`
pub const KEY_RIGHTSHIFT: u16 = 54;

/// `linux/input-event-codes.h`: `#define KEY_KPASTERISK 55`
pub const KEY_KPASTERISK: u16 = 55;

/// `linux/input-event-codes.h`: `#define KEY_LEFTALT 56`
pub const KEY_LEFTALT: u16 = 56;

/// `linux/input-event-codes.h`: `#define KEY_SPACE 57`
pub const KEY_SPACE: u16 = 57;

/// `linux/input-event-codes.h`: `#define KEY_CAPSLOCK 58`
pub const KEY_CAPSLOCK: u16 = 58;

/// `linux/input-event-codes.h`: `#define KEY_F1 59`
pub const KEY_F1: u16 = 59;

/// `linux/input-event-codes.h`: `#define KEY_F2 60`
pub const KEY_F2: u16 = 60;

/// `linux/input-event-codes.h`: `#define KEY_F3 61`
pub const KEY_F3: u16 = 61;

/// `linux/input-event-codes.h`: `#define KEY_F4 62`
pub const KEY_F4: u16 = 62;

/// `linux/input-event-codes.h`: `#define KEY_F5 63`
pub const KEY_F5: u16 = 63;

/// `linux/input-event-codes.h`: `#define KEY_F6 64`
pub const KEY_F6: u16 = 64;

/// `linux/input-event-codes.h`: `#define KEY_F7 65`
pub const KEY_F7: u16 = 65;

/// `linux/input-event-codes.h`: `#define KEY_F8 66`
pub const KEY_F8: u16 = 66;

/// `linux/input-event-codes.h`: `#define KEY_F9 67`
pub const KEY_F9: u16 = 67;

/// `linux/input-event-codes.h`: `#define KEY_F10 68`
pub const KEY_F10: u16 = 68;

/// `linux/input-event-codes.h`: `#define KEY_NUMLOCK 69`
pub const KEY_NUMLOCK: u16 = 69;

/// `linux/input-event-codes.h`: `#define KEY_SCROLLLOCK 70`
pub const KEY_SCROLLLOCK: u16 = 70;

/// `linux/input-event-codes.h`: `#define KEY_KP7 71`
pub const KEY_KP7: u16 = 71;

/// `linux/input-event-codes.h`: `#define KEY_KP8 72`
pub const KEY_KP8: u16 = 72;

/// `linux/input-event-codes.h`: `#define KEY_KP9 73`
pub const KEY_KP9: u16 = 73;

/// `linux/input-event-codes.h`: `#define KEY_KPMINUS 74`
pub const KEY_KPMINUS: u16 = 74;

/// `linux/input-event-codes.h`: `#define KEY_KP4 75`
pub const KEY_KP4: u16 = 75;

/// `linux/input-event-codes.h`: `#define KEY_KP5 76`
pub const KEY_KP5: u16 = 76;

/// `linux/input-event-codes.h`: `#define KEY_KP6 77`
pub const KEY_KP6: u16 = 77;

/// `linux/input-event-codes.h`: `#define KEY_KPPLUS 78`
pub const KEY_KPPLUS: u16 = 78;

/// `linux/input-event-codes.h`: `#define KEY_KP1 79`
pub const KEY_KP1: u16 = 79;

/// `linux/input-event-codes.h`: `#define KEY_KP2 80`
pub const KEY_KP2: u16 = 80;

/// `linux/input-event-codes.h`: `#define KEY_KP3 81`
pub const KEY_KP3: u16 = 81;

/// `linux/input-event-codes.h`: `#define KEY_KP0 82`
pub const KEY_KP0: u16 = 82;

/// `linux/input-event-codes.h`: `#define KEY_KPDOT 83`
pub const KEY_KPDOT: u16 = 83;

/// `linux/input-event-codes.h`: `#define KEY_102ND 86`
pub const KEY_102ND: u16 = 86;

/// `linux/input-event-codes.h`: `#define KEY_F11 87`
pub const KEY_F11: u16 = 87;

/// `linux/input-event-codes.h`: `#define KEY_F12 88`
pub const KEY_F12: u16 = 88;

/// `linux/input-event-codes.h`: `#define KEY_KPENTER 96`
pub const KEY_KPENTER: u16 = 96;

/// `linux/input-event-codes.h`: `#define KEY_RIGHTCTRL 97`
pub const KEY_RIGHTCTRL: u16 = 97;

/// `linux/input-event-codes.h`: `#define KEY_KPSLASH 98`
pub const KEY_KPSLASH: u16 = 98;

/// `linux/input-event-codes.h`: `#define KEY_SYSRQ 99`
pub const KEY_SYSRQ: u16 = 99;

/// `linux/input-event-codes.h`: `#define KEY_RIGHTALT 100`
pub const KEY_RIGHTALT: u16 = 100;

/// `linux/input-event-codes.h`: `#define KEY_HOME 102`
pub const KEY_HOME: u16 = 102;

/// `linux/input-event-codes.h`: `#define KEY_UP 103`
pub const KEY_UP: u16 = 103;

/// `linux/input-event-codes.h`: `#define KEY_PAGEUP 104`
pub const KEY_PAGEUP: u16 = 104;

/// `linux/input-event-codes.h`: `#define KEY_LEFT 105`
pub const KEY_LEFT: u16 = 105;

/// `linux/input-event-codes.h`: `#define KEY_RIGHT 106`
pub const KEY_RIGHT: u16 = 106;

/// `linux/input-event-codes.h`: `#define KEY_END 107`
pub const KEY_END: u16 = 107;

/// `linux/input-event-codes.h`: `#define KEY_DOWN 108`
pub const KEY_DOWN: u16 = 108;

/// `linux/input-event-codes.h`: `#define KEY_PAGEDOWN 109`
pub const KEY_PAGEDOWN: u16 = 109;

/// `linux/input-event-codes.h`: `#define KEY_INSERT 110`
pub const KEY_INSERT: u16 = 110;

/// `linux/input-event-codes.h`: `#define KEY_DELETE 111`
pub const KEY_DELETE: u16 = 111;

/// `linux/input-event-codes.h`: `#define KEY_KPEQUAL 117`
pub const KEY_KPEQUAL: u16 = 117;

/// `linux/input-event-codes.h`: `#define KEY_PAUSE 119`
pub const KEY_PAUSE: u16 = 119;

/// `linux/input-event-codes.h`: `#define KEY_LEFTMETA 125`
pub const KEY_LEFTMETA: u16 = 125;

/// `linux/input-event-codes.h`: `#define KEY_RIGHTMETA 126`
pub const KEY_RIGHTMETA: u16 = 126;

/// `linux/input-event-codes.h`: `#define KEY_COMPOSE 127`
pub const KEY_COMPOSE: u16 = 127;

/// `linux/input-event-codes.h`: `#define KEY_F13 183`
pub const KEY_F13: u16 = 183;

/// `linux/input-event-codes.h`: `#define KEY_F14 184`
pub const KEY_F14: u16 = 184;

/// `linux/input-event-codes.h`: `#define KEY_F15 185`
pub const KEY_F15: u16 = 185;

/// `linux/input-event-codes.h`: `#define KEY_F16 186`
pub const KEY_F16: u16 = 186;

/// `linux/input-event-codes.h`: `#define KEY_F17 187`
pub const KEY_F17: u16 = 187;

/// `linux/input-event-codes.h`: `#define KEY_F18 188`
pub const KEY_F18: u16 = 188;

/// `linux/input-event-codes.h`: `#define KEY_F19 189`
pub const KEY_F19: u16 = 189;

/// `linux/input-event-codes.h`: `#define KEY_F20 190`
pub const KEY_F20: u16 = 190;

/// `linux/input-event-codes.h`: `#define KEY_F21 191`
pub const KEY_F21: u16 = 191;

/// `linux/input-event-codes.h`: `#define KEY_F22 192`
pub const KEY_F22: u16 = 192;

/// `linux/input-event-codes.h`: `#define KEY_F23 193`
pub const KEY_F23: u16 = 193;

/// `linux/input-event-codes.h`: `#define KEY_F24 194`
pub const KEY_F24: u16 = 194;

/// A gamepad's bottom face button, whatever its label (Xbox A, PlayStation
/// ✕). Also `BTN_A`, and `BTN_GAMEPAD`, the first gamepad button.
/// `linux/input-event-codes.h`: `#define BTN_SOUTH 0x130`
pub const BTN_SOUTH: u16 = 0x130;

/// A gamepad's right face button, whatever its label (Xbox B, PlayStation
/// ○). Also `BTN_B`.
/// `linux/input-event-codes.h`: `#define BTN_EAST 0x131`
pub const BTN_EAST: u16 = 0x131;

/// A gamepad's top face button (PlayStation △), by the kernel's
/// documentation. The Xbox driver `xpad` reports the X button, on the left,
/// as this code under its other name, [`BTN_X`].
/// `linux/input-event-codes.h`: `#define BTN_NORTH 0x133`
pub const BTN_NORTH: u16 = 0x133;

/// The same code as [`BTN_NORTH`], by the name that drivers which report the
/// buttons by their Xbox labels use for X.
/// `linux/input-event-codes.h`: `#define BTN_X BTN_NORTH`
pub const BTN_X: u16 = BTN_NORTH;

/// A gamepad's left face button (PlayStation □), by the kernel's
/// documentation. The Xbox driver `xpad` reports the Y button, on top, as
/// this code under its other name, [`BTN_Y`].
/// `linux/input-event-codes.h`: `#define BTN_WEST 0x134`
pub const BTN_WEST: u16 = 0x134;

/// The same code as [`BTN_WEST`], by the name that drivers which report the
/// buttons by their Xbox labels use for Y.
/// `linux/input-event-codes.h`: `#define BTN_Y BTN_WEST`
pub const BTN_Y: u16 = BTN_WEST;

/// The left shoulder button (LB, L1, L).
/// `linux/input-event-codes.h`: `#define BTN_TL 0x136`
pub const BTN_TL: u16 = 0x136;

/// The right shoulder button (RB, R1, R).
/// `linux/input-event-codes.h`: `#define BTN_TR 0x137`
pub const BTN_TR: u16 = 0x137;

/// The left trigger as a button (L2, ZL): a pad with an analog trigger may
/// report both.
/// `linux/input-event-codes.h`: `#define BTN_TL2 0x138`
pub const BTN_TL2: u16 = 0x138;

/// The right trigger as a button (R2, ZR).
/// `linux/input-event-codes.h`: `#define BTN_TR2 0x139`
pub const BTN_TR2: u16 = 0x139;

/// The left middle button (Xbox View or Back, PlayStation Create, Nintendo
/// −).
/// `linux/input-event-codes.h`: `#define BTN_SELECT 0x13a`
pub const BTN_SELECT: u16 = 0x13a;

/// The right middle button (Xbox Menu or Start, PlayStation Options,
/// Nintendo +).
/// `linux/input-event-codes.h`: `#define BTN_START 0x13b`
pub const BTN_START: u16 = 0x13b;

/// The logo button (Xbox, PS, Home).
/// `linux/input-event-codes.h`: `#define BTN_MODE 0x13c`
pub const BTN_MODE: u16 = 0x13c;

/// The left stick pressed in.
/// `linux/input-event-codes.h`: `#define BTN_THUMBL 0x13d`
pub const BTN_THUMBL: u16 = 0x13d;

/// The right stick pressed in.
/// `linux/input-event-codes.h`: `#define BTN_THUMBR 0x13e`
pub const BTN_THUMBR: u16 = 0x13e;

/// The d-pad's up, on a pad that reports the d-pad as buttons and not as
/// [`ABS_HAT0X`] and [`ABS_HAT0Y`].
/// `linux/input-event-codes.h`: `#define BTN_DPAD_UP 0x220`
pub const BTN_DPAD_UP: u16 = 0x220;

/// The d-pad's down.
/// `linux/input-event-codes.h`: `#define BTN_DPAD_DOWN 0x221`
pub const BTN_DPAD_DOWN: u16 = 0x221;

/// The d-pad's left.
/// `linux/input-event-codes.h`: `#define BTN_DPAD_LEFT 0x222`
pub const BTN_DPAD_LEFT: u16 = 0x222;

/// The d-pad's right.
/// `linux/input-event-codes.h`: `#define BTN_DPAD_RIGHT 0x223`
pub const BTN_DPAD_RIGHT: u16 = 0x223;

/// The number of key and button codes: [`EVIOCGBIT_KEY`] and
/// [`EVIOCGKEY_ALL`] report one bit for each.
/// `linux/input-event-codes.h`: `#define KEY_MAX 0x2ff` and `#define KEY_CNT
/// (KEY_MAX+1)`
pub const KEY_CNT: u16 = 0x300;

/// The left stick's horizontal axis. Negative is left.
/// `linux/input-event-codes.h`: `#define ABS_X 0x00`
pub const ABS_X: u16 = 0x00;

/// The left stick's vertical axis. Negative is up.
/// `linux/input-event-codes.h`: `#define ABS_Y 0x01`
pub const ABS_Y: u16 = 0x01;

/// The left analog trigger, on the gamepads of `xpad` and
/// `hid-playstation`; the right stick's horizontal axis on an Xbox pad over
/// Bluetooth (see [`ABS_GAS`]).
/// `linux/input-event-codes.h`: `#define ABS_Z 0x02`
pub const ABS_Z: u16 = 0x02;

/// The right stick's horizontal axis.
/// `linux/input-event-codes.h`: `#define ABS_RX 0x03`
pub const ABS_RX: u16 = 0x03;

/// The right stick's vertical axis.
/// `linux/input-event-codes.h`: `#define ABS_RY 0x04`
pub const ABS_RY: u16 = 0x04;

/// The right analog trigger, on the gamepads of `xpad` and
/// `hid-playstation`; the right stick's vertical axis on an Xbox pad over
/// Bluetooth.
/// `linux/input-event-codes.h`: `#define ABS_RZ 0x05`
pub const ABS_RZ: u16 = 0x05;

/// The right trigger of an Xbox pad over Bluetooth, through `hid-generic`
/// or `hid-microsoft`, which put its right stick on [`ABS_Z`] and
/// [`ABS_RZ`].
/// `linux/input-event-codes.h`: `#define ABS_GAS 0x09`
pub const ABS_GAS: u16 = 0x09;

/// The left trigger of an Xbox pad over Bluetooth (see [`ABS_GAS`]).
/// `linux/input-event-codes.h`: `#define ABS_BRAKE 0x0a`
pub const ABS_BRAKE: u16 = 0x0a;

/// The d-pad's horizontal axis, -1 to 1, on a pad that reports the d-pad
/// as a hat.
/// `linux/input-event-codes.h`: `#define ABS_HAT0X 0x10`
pub const ABS_HAT0X: u16 = 0x10;

/// The d-pad's vertical axis, -1 (up) to 1 (down).
/// `linux/input-event-codes.h`: `#define ABS_HAT0Y 0x11`
pub const ABS_HAT0Y: u16 = 0x11;

/// The number of absolute axis codes: [`EVIOCGBIT_ABS`] reports one bit for
/// each.
/// `linux/input-event-codes.h`: `#define ABS_MAX 0x3f` and `#define ABS_CNT
/// (ABS_MAX+1)`
pub const ABS_CNT: u16 = 0x40;

/// One event, as `read` gives them from an event device: a whole number of
/// these, never part of one.
/// `linux/input.h`:
///
/// ```c
/// struct input_event {
///     struct timeval time;   /* on 64-bit targets */
///     __u16 type;
///     __u16 code;
///     __s32 value;
/// };
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct input_event {
    /// When the event happened, by the clock the device was set to
    /// (`CLOCK_REALTIME` unless changed).
    pub time: timeval,
    /// The event type: [`EV_SYN`], [`EV_KEY`], [`EV_ABS`], ...
    pub type_: u16,
    /// The code within the type: a `KEY_*`, `BTN_*` or `ABS_*`.
    pub code: u16,
    /// For a key, 1 down, 0 up and 2 a repeat; for an axis, its value.
    pub value: i32,
}

// Our targets are 64-bit, where the header uses `struct timeval`.
const _: () = assert!(size_of::<input_event>() == 24);
const _: () = assert!(align_of::<input_event>() == 8);
const _: () = assert!(offset_of!(input_event, type_) == 16);
const _: () = assert!(offset_of!(input_event, code) == 18);
const _: () = assert!(offset_of!(input_event, value) == 20);

/// Who made a device, as [`EVIOCGID`] gives it.
/// `linux/input.h`:
///
/// ```c
/// struct input_id {
///     __u16 bustype;
///     __u16 vendor;
///     __u16 product;
///     __u16 version;
/// };
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct input_id {
    /// The bus, a `BUS_*` number (USB, Bluetooth, ...).
    pub bustype: u16,
    /// The vendor's id: the USB vendor id for a USB or Bluetooth device.
    pub vendor: u16,
    /// The vendor's product id.
    pub product: u16,
    /// The product's version.
    pub version: u16,
}

const _: () = assert!(size_of::<input_id>() == 8);
const _: () = assert!(align_of::<input_id>() == 2);
const _: () = assert!(offset_of!(input_id, vendor) == 2);
const _: () = assert!(offset_of!(input_id, product) == 4);

/// An absolute axis: its value and its range, as an `EVIOCGABS` request
/// gives them.
/// `linux/input.h`:
///
/// ```c
/// struct input_absinfo {
///     __s32 value;
///     __s32 minimum;
///     __s32 maximum;
///     __s32 fuzz;
///     __s32 flat;
///     __s32 resolution;
/// };
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct input_absinfo {
    /// The axis's value now.
    pub value: i32,
    /// The smallest value the axis reports.
    pub minimum: i32,
    /// The largest value the axis reports.
    pub maximum: i32,
    /// The noise the driver filters out.
    pub fuzz: i32,
    /// The driver's suggested dead zone around the center.
    pub flat: i32,
    /// Units per millimeter, or per radian: often 0, unknown.
    pub resolution: i32,
}

const _: () = assert!(size_of::<input_absinfo>() == 24);
const _: () = assert!(align_of::<input_absinfo>() == 4);
const _: () = assert!(offset_of!(input_absinfo, minimum) == 4);
const _: () = assert!(offset_of!(input_absinfo, maximum) == 8);

/// Reads the device's [`input_id`].
/// `linux/input.h`: `#define EVIOCGID _IOR('E', 0x02, struct input_id)`
pub const EVIOCGID: c_ulong = 0x8008_4502;

/// Reads the device's name into a buffer of 256 bytes, with a NUL at the
/// end; `ioctl` returns the bytes written, the NUL among them.
/// `linux/input.h`: `#define EVIOCGNAME(len) _IOC(_IOC_READ, 'E', 0x06,
/// len)`, with `len` 256.
pub const EVIOCGNAME_256: c_ulong = 0x8100_4506;

/// Reads the device's state of every key and button: [`KEY_CNT`] bits, one
/// for each code, in [`KEY_CNT`]` / 8` bytes, bit `n % 8` of byte `n / 8`
/// set while code `n` is down.
/// `linux/input.h`: `#define EVIOCGKEY(len) _IOC(_IOC_READ, 'E', 0x18,
/// len)`, with `len` `KEY_CNT / 8`.
pub const EVIOCGKEY_ALL: c_ulong = 0x8060_4518;

/// Reads which event types the device reports: [`EV_CNT`] bits in
/// [`EV_CNT`]` / 8` bytes, bit `n % 8` of byte `n / 8` for type `n`.
/// `linux/input.h`: `#define EVIOCGBIT(ev,len) _IOC(_IOC_READ, 'E', 0x20 +
/// (ev), len)`, with `ev` 0 and `len` `EV_CNT / 8`.
pub const EVIOCGBIT_EV: c_ulong = 0x8004_4520;

/// Reads which key and button codes the device reports: [`KEY_CNT`] bits in
/// [`KEY_CNT`]` / 8` bytes.
/// `linux/input.h`: `EVIOCGBIT(ev,len)`, with `ev` [`EV_KEY`] and `len`
/// `KEY_CNT / 8`.
pub const EVIOCGBIT_KEY: c_ulong = 0x8060_4521;

/// Reads which absolute axes the device reports: [`ABS_CNT`] bits in
/// [`ABS_CNT`]` / 8` bytes.
/// `linux/input.h`: `EVIOCGBIT(ev,len)`, with `ev` [`EV_ABS`] and `len`
/// `ABS_CNT / 8`.
pub const EVIOCGBIT_ABS: c_ulong = 0x8008_4523;

/// Reads [`ABS_X`]'s [`input_absinfo`].
/// `linux/input.h`: `#define EVIOCGABS(abs) _IOR('E', 0x40 + (abs), struct
/// input_absinfo)`, with `abs` `ABS_X`.
pub const EVIOCGABS_X: c_ulong = 0x8018_4540;

/// Reads [`ABS_Y`]'s [`input_absinfo`]: `EVIOCGABS(ABS_Y)`.
pub const EVIOCGABS_Y: c_ulong = 0x8018_4541;

/// Reads [`ABS_Z`]'s [`input_absinfo`]: `EVIOCGABS(ABS_Z)`.
pub const EVIOCGABS_Z: c_ulong = 0x8018_4542;

/// Reads [`ABS_RX`]'s [`input_absinfo`]: `EVIOCGABS(ABS_RX)`.
pub const EVIOCGABS_RX: c_ulong = 0x8018_4543;

/// Reads [`ABS_RY`]'s [`input_absinfo`]: `EVIOCGABS(ABS_RY)`.
pub const EVIOCGABS_RY: c_ulong = 0x8018_4544;

/// Reads [`ABS_RZ`]'s [`input_absinfo`]: `EVIOCGABS(ABS_RZ)`.
pub const EVIOCGABS_RZ: c_ulong = 0x8018_4545;

/// Reads [`ABS_GAS`]'s [`input_absinfo`]: `EVIOCGABS(ABS_GAS)`.
pub const EVIOCGABS_GAS: c_ulong = 0x8018_4549;

/// Reads [`ABS_BRAKE`]'s [`input_absinfo`]: `EVIOCGABS(ABS_BRAKE)`.
pub const EVIOCGABS_BRAKE: c_ulong = 0x8018_454a;

/// Reads [`ABS_HAT0X`]'s [`input_absinfo`]: `EVIOCGABS(ABS_HAT0X)`.
pub const EVIOCGABS_HAT0X: c_ulong = 0x8018_4550;

/// Reads [`ABS_HAT0Y`]'s [`input_absinfo`]: `EVIOCGABS(ABS_HAT0Y)`.
pub const EVIOCGABS_HAT0Y: c_ulong = 0x8018_4551;
