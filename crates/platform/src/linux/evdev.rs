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

//! Gamepads on Linux: the kernel's event devices, `/dev/input/event*`, read
//! directly, with no library.
//!
//! - A device is a gamepad if it reports keys and absolute axes, and has
//!   `BTN_SOUTH`, the first gamepad button. Its axes' ranges come from
//!   `EVIOCGABS`.
//! - The first poll opens every gamepad there is. After that, `inotify` on
//!   `/dev/input` says when a node is made (`IN_CREATE`) and when its
//!   permissions change (`IN_ATTRIB`): udev gives a new node its permissions
//!   after it makes it, so a node that cannot be opened when it appears is
//!   tried again then. A pad that is unplugged fails its reads with `ENODEV`.
//! - The modern kernel drivers (`xpad`, `hid-playstation`, `hid-nintendo`)
//!   report the standard codes, so one table maps every pad. The face
//!   buttons differ: the kernel's documentation reports them by place, as
//!   `hid-playstation` and `hid-nintendo` do, while `xpad`, and an Xbox pad
//!   over Bluetooth, report them by their Xbox labels, which puts X, on the
//!   left, on the code of the top button. So the codes are read by place for
//!   Sony's pads, as SDL does, and Nintendo's, whose driver reports by place
//!   too, and by label for every other.
//! - The axes differ too. `xpad`, `hid-playstation` and `hid-nintendo` put
//!   the right stick on `ABS_RX` and `ABS_RY`, and analog triggers on
//!   `ABS_Z` and `ABS_RZ`. An Xbox pad over Bluetooth, through
//!   `hid-generic` or `hid-microsoft`, has no `ABS_RX` or `ABS_RY`: its right
//!   stick is `ABS_Z` and `ABS_RZ`, and its triggers `ABS_BRAKE` (left) and
//!   `ABS_GAS` (right). So a pad without `ABS_RX` and `ABS_RY` is read that
//!   way.
//! - The d-pad comes as a hat (`ABS_HAT0X`, `ABS_HAT0Y`) or as buttons
//!   (`BTN_DPAD_*`). A pad whose triggers are only buttons (`BTN_TL2`,
//!   `BTN_TR2`, such as the Switch Pro Controller's) gives trigger axes of 0
//!   or 1; one with analog triggers gives those and ignores the buttons.
//! - If the kernel's buffer of a device's events overflows, it says so with
//!   `SYN_DROPPED`. Every record from there on that was read or is still
//!   queued is older than the state, so all are dropped, as libevdev does:
//!   the rest of the read, and what the device still has queued. Then the
//!   state is read again with `EVIOCGABS` and `EVIOCGKEY`, and the changes
//!   are reported. (`EVIOCGKEY` also takes the queued key records out of
//!   the kernel's queue: applying an older one after it would leave a
//!   button stuck.)
//! - A pad is read whether or not the window has the focus.
//!
//! Access to the nodes comes from the desktop seat's udev rules (`uaccess`),
//! which give the user at the seat the gamepads, but not the keyboards and
//! mice. If the first poll finds a gamepad it cannot open, it logs a hint.
//!
//! Nothing here allocates while no pad is plugged in or unplugged: the reads
//! go into buffers on the stack.

use core::ffi::{CStr, c_int, c_ulong};
use core::mem::offset_of;
use core::ptr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use nadir_core::log::{debug, info, warn};
use nadir_core::{GamepadAxis, GamepadButton};
use nadir_sys::linux::evdev::{
    ABS_BRAKE, ABS_CNT, ABS_GAS, ABS_HAT0X, ABS_HAT0Y, ABS_RX, ABS_RY, ABS_RZ, ABS_X, ABS_Y, ABS_Z,
    BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_DPAD_UP, BTN_EAST, BTN_MODE, BTN_NORTH,
    BTN_SELECT, BTN_SOUTH, BTN_START, BTN_THUMBL, BTN_THUMBR, BTN_TL, BTN_TL2, BTN_TR, BTN_TR2,
    BTN_WEST, EV_ABS, EV_CNT, EV_KEY, EV_SYN, EVIOCGABS_BRAKE, EVIOCGABS_GAS, EVIOCGABS_HAT0X,
    EVIOCGABS_HAT0Y, EVIOCGABS_RX, EVIOCGABS_RY, EVIOCGABS_RZ, EVIOCGABS_X, EVIOCGABS_Y,
    EVIOCGABS_Z, EVIOCGBIT_ABS, EVIOCGBIT_EV, EVIOCGBIT_KEY, EVIOCGID, EVIOCGKEY_ALL,
    EVIOCGNAME_256, KEY_CNT, SYN_DROPPED, SYN_REPORT, input_absinfo, input_event, input_id,
};
use nadir_sys::linux::libc::{
    _IOC_DIRSHIFT, _IOC_READ, _IOC_SIZEBITS, _IOC_SIZESHIFT, ENODEV, IN_ATTRIB, IN_CLOEXEC,
    IN_CREATE, IN_IGNORED, IN_NONBLOCK, IN_Q_OVERFLOW, O_NONBLOCK, inotify_add_watch,
    inotify_event, inotify_init1, ioctl,
};

use crate::{Event, GamepadId, GamepadState};

/// The folder of the event devices.
const INPUT_DIR: &str = "/dev/input";

/// [`INPUT_DIR`], for `inotify_add_watch`.
const INPUT_DIR_C: &CStr = c"/dev/input";

/// The folder in which sysfs describes each input device, readable by
/// everyone: `<node>/device/capabilities/{ev,key}`.
const SYSFS_INPUT_DIR: &str = "/sys/class/input";

/// Sony's USB vendor id. `hid-playstation` and `hid-sony` report the face
/// buttons by place.
const SONY: u16 = 0x054c;

/// Nintendo's USB vendor id. `hid-nintendo` reports the face buttons by
/// place.
const NINTENDO: u16 = 0x057e;

/// The bytes of one record read from an event device.
const RECORD: usize = size_of::<input_event>();

/// How many records one read takes at most.
const RECORDS_PER_READ: usize = 64;

/// The bytes of the bitmap of event types.
const TYPE_BYTES: usize = EV_CNT as usize / 8;

/// The bytes of the bitmap of key and button codes.
const KEY_BYTES: usize = KEY_CNT as usize / 8;

/// The bytes of the bitmap of absolute axes.
const AXIS_BYTES: usize = ABS_CNT as usize / 8;

/// The key codes a pad is read through, in the order its state is read
/// again.
const KEYS: [u16; 17] = [
    BTN_SOUTH,
    BTN_EAST,
    BTN_NORTH,
    BTN_WEST,
    BTN_TL,
    BTN_TR,
    BTN_TL2,
    BTN_TR2,
    BTN_SELECT,
    BTN_START,
    BTN_MODE,
    BTN_THUMBL,
    BTN_THUMBR,
    BTN_DPAD_UP,
    BTN_DPAD_DOWN,
    BTN_DPAD_LEFT,
    BTN_DPAD_RIGHT,
];

/// What an absolute axis drives.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    /// A stick's axis, -1 to 1 around the middle of the range.
    Stick(GamepadAxis),
    /// A trigger, 0 to 1 from the bottom of the range.
    Trigger(GamepadAxis),
    /// Two buttons, one below the middle of the range and one above it: the
    /// d-pad as a hat.
    Hat {
        negative: GamepadButton,
        positive: GamepadButton,
    },
}

/// The absolute axes a pad may be read through: each one's code, and the
/// request that reads its range and value. What each drives depends on the
/// pad: see [`roles`].
const AXES: [(u16, c_ulong); 10] = [
    (ABS_X, EVIOCGABS_X),
    (ABS_Y, EVIOCGABS_Y),
    (ABS_Z, EVIOCGABS_Z),
    (ABS_RX, EVIOCGABS_RX),
    (ABS_RY, EVIOCGABS_RY),
    (ABS_RZ, EVIOCGABS_RZ),
    (ABS_GAS, EVIOCGABS_GAS),
    (ABS_BRAKE, EVIOCGABS_BRAKE),
    (ABS_HAT0X, EVIOCGABS_HAT0X),
    (ABS_HAT0Y, EVIOCGABS_HAT0Y),
];

/// What each axis of [`AXES`] drives on a pad that has the absolute axes of
/// the bitmap `axes`.
///
/// `xpad`, `hid-playstation` and `hid-nintendo` put the right stick on
/// `ABS_RX` and `ABS_RY`, and analog triggers on `ABS_Z` and `ABS_RZ`. A pad
/// without `ABS_RX` and `ABS_RY`, such as an Xbox pad over Bluetooth through
/// `hid-generic` or `hid-microsoft`, has its right stick on `ABS_Z` and
/// `ABS_RZ`, and its triggers, if analog, on `ABS_BRAKE` (left) and
/// `ABS_GAS` (right).
fn roles(axes: &[u8; AXIS_BYTES]) -> [Option<Target>; AXES.len()] {
    let has = |code| bit(axes, code);
    let right_on_z = !has(ABS_RX) && !has(ABS_RY);
    AXES.map(|(code, _)| {
        let target = match code {
            ABS_X => Target::Stick(GamepadAxis::LeftX),
            ABS_Y => Target::Stick(GamepadAxis::LeftY),
            ABS_RX => Target::Stick(GamepadAxis::RightX),
            ABS_RY => Target::Stick(GamepadAxis::RightY),
            ABS_Z if right_on_z => Target::Stick(GamepadAxis::RightX),
            ABS_RZ if right_on_z => Target::Stick(GamepadAxis::RightY),
            ABS_Z => Target::Trigger(GamepadAxis::LeftTrigger),
            ABS_RZ => Target::Trigger(GamepadAxis::RightTrigger),
            // Only where `ABS_Z` and `ABS_RZ` do not already drive them.
            ABS_BRAKE if right_on_z || !has(ABS_Z) => Target::Trigger(GamepadAxis::LeftTrigger),
            ABS_GAS if right_on_z || !has(ABS_RZ) => Target::Trigger(GamepadAxis::RightTrigger),
            ABS_HAT0X => Target::Hat {
                negative: GamepadButton::DPadLeft,
                positive: GamepadButton::DPadRight,
            },
            ABS_HAT0Y => Target::Hat {
                negative: GamepadButton::DPadUp,
                positive: GamepadButton::DPadDown,
            },
            _ => return None,
        };
        Some(target)
    })
}

/// The place of an axis's code in [`AXES`].
fn axis_slot(code: u16) -> Option<usize> {
    AXES.iter().position(|&(axis, ..)| axis == code)
}

/// Whether bit `n` is set in a bitmap that the kernel filled: bit `n % 8`
/// of byte `n / 8`, on our little-endian targets.
fn bit(bytes: &[u8], n: u16) -> bool {
    bytes
        .get(usize::from(n / 8))
        .is_some_and(|byte| byte & (1 << (n % 8)) != 0)
}

/// What a device says it reports.
#[derive(Clone, Copy, Debug)]
struct Capabilities {
    types: [u8; TYPE_BYTES],
    keys: [u8; KEY_BYTES],
    axes: [u8; AXIS_BYTES],
}

impl Capabilities {
    fn none() -> Self {
        Self {
            types: [0; TYPE_BYTES],
            keys: [0; KEY_BYTES],
            axes: [0; AXIS_BYTES],
        }
    }

    /// Whether the device is a gamepad: keys and absolute axes, and the
    /// first gamepad button among the keys.
    fn is_gamepad(&self) -> bool {
        bit(&self.types, EV_KEY) && bit(&self.types, EV_ABS) && bit(&self.keys, BTN_SOUTH)
    }
}

/// An absolute axis's range, as the driver gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    min: i32,
    max: i32,
}

impl Range {
    /// The range of `info`, unless it is empty.
    fn new(info: &input_absinfo) -> Option<Self> {
        (info.minimum < info.maximum).then_some(Self {
            min: info.minimum,
            max: info.maximum,
        })
    }

    /// The middle of the range, rounded up: 128 for a stick from 0 to 255,
    /// whose rest is there, and 0 for one from -32768 to 32767.
    fn center(self) -> i64 {
        (i64::from(self.min) + i64::from(self.max) + 1).div_euclid(2)
    }

    /// A stick's value: -1 at the bottom of the range, 0 at its middle and 1
    /// at its top, in a straight line on each side. A range of two values
    /// has no middle: its top is its rounded-up middle, and gives 1.
    fn stick(self, value: i32) -> f32 {
        let value = i64::from(value);
        let center = self.center();
        let span = if value >= center {
            i64::from(self.max) - center
        } else {
            center - i64::from(self.min)
        };
        if span == 0 {
            // Only the top of a range of two values, whose center is its
            // top, or a value above it.
            return 1.0;
        }
        ((value - center) as f64 / span as f64).clamp(-1.0, 1.0) as f32
    }

    /// A trigger's value: 0 at the bottom of the range and 1 at its top.
    fn trigger(self, value: i32) -> f32 {
        let pulled = i64::from(value) - i64::from(self.min);
        let span = i64::from(self.max) - i64::from(self.min);
        (pulled as f64 / span as f64).clamp(0.0, 1.0) as f32
    }
}

/// What a key code drives on one pad.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Control {
    Button(GamepadButton),
    /// A trigger that is only a button, at 0 or 1.
    Trigger(GamepadAxis),
}

/// How one pad's codes map to its buttons and axes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// Whether the driver reports the face buttons by place, as the kernel's
    /// documentation asks, and not by their Xbox labels.
    by_place: bool,
    /// What each axis of [`AXES`] drives on this pad: see [`roles`].
    roles: [Option<Target>; AXES.len()],
    /// The range of each axis of [`AXES`] that the pad has, once read.
    ranges: [Option<Range>; AXES.len()],
}

impl Layout {
    /// The layout of a pad with the absolute axes of the bitmap `axes`,
    /// before its ranges are read.
    fn new(by_place: bool, axes: &[u8; AXIS_BYTES]) -> Self {
        Self {
            by_place,
            roles: roles(axes),
            ranges: [None; AXES.len()],
        }
    }

    /// What the key `code` drives, if anything.
    fn key(&self, code: u16) -> Option<Control> {
        let button = match code {
            BTN_SOUTH => GamepadButton::A,
            BTN_EAST => GamepadButton::B,
            // By place, this is the top button. `xpad` reports X, on the
            // left, here, by its other name, `BTN_X`.
            BTN_NORTH if self.by_place => GamepadButton::Y,
            BTN_NORTH => GamepadButton::X,
            // By place, the left button; by label, `BTN_Y`.
            BTN_WEST if self.by_place => GamepadButton::X,
            BTN_WEST => GamepadButton::Y,
            BTN_TL => GamepadButton::LeftBumper,
            BTN_TR => GamepadButton::RightBumper,
            BTN_TL2 => return self.digital_trigger(GamepadAxis::LeftTrigger),
            BTN_TR2 => return self.digital_trigger(GamepadAxis::RightTrigger),
            BTN_SELECT => GamepadButton::Back,
            BTN_START => GamepadButton::Start,
            BTN_MODE => GamepadButton::Guide,
            BTN_THUMBL => GamepadButton::LeftStick,
            BTN_THUMBR => GamepadButton::RightStick,
            BTN_DPAD_UP => GamepadButton::DPadUp,
            BTN_DPAD_DOWN => GamepadButton::DPadDown,
            BTN_DPAD_LEFT => GamepadButton::DPadLeft,
            BTN_DPAD_RIGHT => GamepadButton::DPadRight,
            _ => return None,
        };
        Some(Control::Button(button))
    }

    /// A trigger button drives its trigger axis only if the pad has no
    /// analog trigger there.
    fn digital_trigger(&self, axis: GamepadAxis) -> Option<Control> {
        let analog = self
            .roles
            .iter()
            .zip(&self.ranges)
            .any(|(&role, range)| role == Some(Target::Trigger(axis)) && range.is_some());
        (!analog).then_some(Control::Trigger(axis))
    }

    /// Applies a key's new value to the state.
    fn apply_key(
        &self,
        code: u16,
        down: bool,
        id: GamepadId,
        state: &mut GamepadState,
        out: &mut Vec<Event>,
    ) {
        match self.key(code) {
            Some(Control::Button(button)) => state.set_button(id, button, down, out),
            Some(Control::Trigger(axis)) => {
                state.set_axis(id, axis, if down { 1.0 } else { 0.0 }, out);
            }
            None => {}
        }
    }

    /// Applies an axis's new value to the state.
    fn apply_axis(
        &self,
        code: u16,
        value: i32,
        id: GamepadId,
        state: &mut GamepadState,
        out: &mut Vec<Event>,
    ) {
        let Some(slot) = axis_slot(code) else {
            return;
        };
        let (Some(target), Some(range)) = (self.roles[slot], self.ranges[slot]) else {
            return;
        };
        match target {
            Target::Stick(axis) => state.set_axis(id, axis, range.stick(value), out),
            Target::Trigger(axis) => state.set_axis(id, axis, range.trigger(value), out),
            Target::Hat { negative, positive } => {
                let center = range.center();
                state.set_button(id, negative, i64::from(value) < center, out);
                state.set_button(id, positive, i64::from(value) > center, out);
            }
        }
    }
}

/// One record read from an event device: the fields of an
/// [`input_event`] that are used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Record {
    kind: u16,
    code: u16,
    value: i32,
}

impl Record {
    /// Reads a record from its [`RECORD`] bytes.
    fn parse(bytes: &[u8; RECORD]) -> Self {
        let u16_at = |offset: usize| u16::from_ne_bytes([bytes[offset], bytes[offset + 1]]);
        let value = offset_of!(input_event, value);
        Self {
            kind: u16_at(offset_of!(input_event, type_)),
            code: u16_at(offset_of!(input_event, code)),
            value: i32::from_ne_bytes([
                bytes[value],
                bytes[value + 1],
                bytes[value + 2],
                bytes[value + 3],
            ]),
        }
    }
}

/// What a pad's reader does with a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Applies it to the state.
    Apply(Record),
    /// Nothing: a marker, or a record of a report that was partly lost.
    Skip,
    /// Reads the whole state from the device again: a lost report ended.
    Resync,
}

/// Follows the markers of a pad's records, and drops the records of a
/// report the kernel lost part of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Decoder {
    /// Whether a `SYN_DROPPED` came, and its report's `SYN_REPORT` not yet.
    dropping: bool,
}

impl Decoder {
    fn step(&mut self, record: Record) -> Step {
        if record.kind == EV_SYN {
            match record.code {
                SYN_DROPPED => self.dropping = true,
                SYN_REPORT if self.dropping => {
                    self.dropping = false;
                    return Step::Resync;
                }
                _ => {}
            }
            return Step::Skip;
        }
        if self.dropping {
            Step::Skip
        } else {
            Step::Apply(record)
        }
    }
}

/// Types that every pattern of bytes is a valid value of, which a request
/// may fill.
///
/// # Safety
///
/// Every bit pattern of the type's size is a valid value of it.
unsafe trait Plain {}

// SAFETY: every pattern of bytes is a valid array of bytes.
unsafe impl<const N: usize> Plain for [u8; N] {}

// SAFETY: `input_absinfo` is six `i32`s and nothing else.
unsafe impl Plain for input_absinfo {}

// SAFETY: `input_id` is four `u16`s and nothing else.
unsafe impl Plain for input_id {}

/// Sends `file`'s driver a request that reads into `value`, and returns
/// what `ioctl` returns.
///
/// Refuses, with `InvalidInput`, a request that is not a read of exactly
/// the size of `T`.
fn read_request<T: Plain>(file: &File, request: c_ulong, value: &mut T) -> io::Result<c_int> {
    let direction = request >> _IOC_DIRSHIFT;
    let size = (request >> _IOC_SIZESHIFT) & ((1 << _IOC_SIZEBITS) - 1);
    if direction != _IOC_READ || usize::try_from(size).ok() != Some(size_of::<T>()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "request {request:#x} does not read {} bytes",
                size_of::<T>()
            ),
        ));
    }
    // SAFETY: the request writes at most its size, which is the size of
    // `T`, into the value, which is valid for writes of that many bytes and
    // valid whatever bytes they are (`Plain`). The descriptor is open while
    // `file` lives. A driver that does not take the request fails it.
    let result = unsafe { ioctl(file.as_raw_fd(), request, ptr::from_mut(value)) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// Whether a node's name is that of an event device: `event` and a number.
fn is_event_node(name: &[u8]) -> bool {
    name.strip_prefix(b"event")
        .is_some_and(|number| !number.is_empty() && number.iter().all(u8::is_ascii_digit))
}

/// The number of an event device's node, to open the nodes in order.
fn node_number(name: &str) -> u32 {
    name.strip_prefix("event")
        .and_then(|number| number.parse().ok())
        .unwrap_or(u32::MAX)
}

/// The events in what one read of an inotify instance gave: each one's mask,
/// and the name of the file, without its NUL padding.
fn inotify_events(bytes: &[u8]) -> impl Iterator<Item = (u32, &[u8])> {
    let header = size_of::<inotify_event>();
    let u32_at = |bytes: &[u8], offset: usize| {
        bytes
            .get(offset..offset + 4)
            .and_then(|field| field.try_into().ok())
            .map(u32::from_ne_bytes)
    };
    let mut rest = bytes;
    core::iter::from_fn(move || {
        let mask = u32_at(rest, offset_of!(inotify_event, mask))?;
        let len = usize::try_from(u32_at(rest, offset_of!(inotify_event, len))?).ok()?;
        let name = rest.get(header..header.checked_add(len)?)?;
        let name = name.split(|&byte| byte == 0).next().unwrap_or(name);
        rest = &rest[header + len..];
        Some((mask, name))
    })
}

/// Whether bit `n` is set in a bitmap as sysfs prints it: words of 64 bits
/// in hexadecimal, separated by spaces, the highest first, and the highest
/// that are zero left out.
fn sysfs_bit(text: &str, n: u16) -> bool {
    text.split_whitespace()
        .rev()
        .nth(usize::from(n / 64))
        .and_then(|word| u64::from_str_radix(word, 16).ok())
        .is_some_and(|word| word & (1 << (n % 64)) != 0)
}

/// Whether sysfs, which everyone can read, says that the node is a gamepad,
/// as [`Capabilities::is_gamepad`] would.
fn sysfs_says_gamepad(node: &str) -> bool {
    let capabilities = Path::new(SYSFS_INPUT_DIR)
        .join(node)
        .join("device/capabilities");
    let read = |name: &str| fs::read_to_string(capabilities.join(name)).ok();
    match (read("ev"), read("key")) {
        (Some(types), Some(keys)) => {
            sysfs_bit(&types, EV_KEY) && sysfs_bit(&types, EV_ABS) && sysfs_bit(&keys, BTN_SOUTH)
        }
        _ => false,
    }
}

/// What one read of a device gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Got {
    /// This many bytes.
    Bytes(usize),
    /// Nothing yet: it would have waited.
    Nothing,
    /// The device is gone, or cannot be read.
    Gone,
}

/// One gamepad that is plugged in.
struct Pad {
    id: GamepadId,
    /// The node's name in [`INPUT_DIR`], such as `event23`.
    node: String,
    file: File,
    /// Which key codes the device has.
    keys: [u8; KEY_BYTES],
    /// Which absolute axes the device has.
    axes: [u8; AXIS_BYTES],
    layout: Layout,
    decoder: Decoder,
    state: GamepadState,
}

impl Pad {
    /// The gamepad that `file`, an event device, is, or `None` if it is
    /// another kind of device. Its state is at rest: [`Pad::sync`] reads it.
    fn probe(node: &str, file: File, id: GamepadId) -> io::Result<Option<Self>> {
        let mut capabilities = Capabilities::none();
        read_request(&file, EVIOCGBIT_EV, &mut capabilities.types)?;
        if !bit(&capabilities.types, EV_KEY) || !bit(&capabilities.types, EV_ABS) {
            return Ok(None);
        }
        read_request(&file, EVIOCGBIT_KEY, &mut capabilities.keys)?;
        read_request(&file, EVIOCGBIT_ABS, &mut capabilities.axes)?;
        if !capabilities.is_gamepad() {
            return Ok(None);
        }
        let mut device = input_id::default();
        read_request(&file, EVIOCGID, &mut device)?;
        let mut name = [0u8; 256];
        // A device may have no name.
        let name = match read_request(&file, EVIOCGNAME_256, &mut name) {
            Ok(_) => {
                let end = name
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len());
                String::from_utf8_lossy(&name[..end]).into_owned()
            }
            Err(_) => String::new(),
        };
        let by_place = matches!(device.vendor, SONY | NINTENDO);
        info!(
            "gamepad {} connected: {name:?} ({:04x}:{:04x}) at {INPUT_DIR}/{node}, its face \
             buttons read by {}",
            id.raw(),
            device.vendor,
            device.product,
            if by_place { "place" } else { "label" },
        );
        Ok(Some(Self {
            id,
            node: node.to_string(),
            file,
            keys: capabilities.keys,
            axes: capabilities.axes,
            layout: Layout::new(by_place, &capabilities.axes),
            decoder: Decoder::default(),
            state: GamepadState::new(name),
        }))
    }

    /// Reads the device's whole state, its axes' ranges among it, and
    /// reports what changed.
    fn sync(&mut self, out: &mut Vec<Event>) -> io::Result<()> {
        // The ranges first: whether a trigger button drives its axis depends
        // on them.
        let mut values = [None; AXES.len()];
        for (slot, &(code, request)) in AXES.iter().enumerate() {
            if !bit(&self.axes, code) || self.layout.roles[slot].is_none() {
                continue;
            }
            let mut info = input_absinfo::default();
            read_request(&self.file, request, &mut info)?;
            self.layout.ranges[slot] = Range::new(&info);
            values[slot] = Some(info.value);
        }
        // A gamepad has `BTN_SOUTH` at least; a device with none of these
        // keys, such as the tests' stand-in, has no key state to read.
        if KEYS.iter().any(|&code| bit(&self.keys, code)) {
            let mut down = [0u8; KEY_BYTES];
            read_request(&self.file, EVIOCGKEY_ALL, &mut down)?;
            for code in KEYS {
                if bit(&self.keys, code) {
                    self.layout
                        .apply_key(code, bit(&down, code), self.id, &mut self.state, out);
                }
            }
        }
        for (slot, value) in values.into_iter().enumerate() {
            if let Some(value) = value {
                self.layout
                    .apply_axis(AXES[slot].0, value, self.id, &mut self.state, out);
            }
        }
        Ok(())
    }

    /// Reads what the device sent since the last poll, and reports it.
    /// Returns `false` once the device is gone, or can no longer be read.
    fn read(&mut self, out: &mut Vec<Event>) -> bool {
        let mut buffer = [0u8; RECORD * RECORDS_PER_READ];
        'reads: loop {
            let read = match self.read_into(&mut buffer) {
                Got::Bytes(read) => read,
                Got::Nothing => return true,
                Got::Gone => return false,
            };
            // The kernel gives whole records only.
            for bytes in buffer[..read].as_chunks::<RECORD>().0 {
                match self.decoder.step(Record::parse(bytes)) {
                    Step::Apply(record) => self.apply(record, out),
                    Step::Skip => {}
                    Step::Resync => {
                        if !self.resync(out) {
                            return false;
                        }
                        // The rest of this read is older than the state just
                        // read: drop it too.
                        continue 'reads;
                    }
                }
            }
        }
    }

    /// One read of the device into `buffer`.
    fn read_into(&mut self, buffer: &mut [u8]) -> Got {
        loop {
            match self.file.read(buffer) {
                // An event device never ends: only a file that stands in for
                // one, in tests, does.
                Ok(0) => return Got::Gone,
                Ok(read) => return Got::Bytes(read),
                Err(error) => match error.kind() {
                    io::ErrorKind::WouldBlock => return Got::Nothing,
                    io::ErrorKind::Interrupted => {}
                    _ => {
                        if error.raw_os_error() != Some(ENODEV) {
                            warn!(
                                "gamepad {} at {INPUT_DIR}/{} cannot be read, and is taken as \
                                 unplugged: {error}",
                                self.id.raw(),
                                self.node
                            );
                        }
                        return Got::Gone;
                    }
                },
            }
        }
    }

    /// After a report the kernel lost part of: drops every record the device
    /// still has queued, which is older than its state, as libevdev does,
    /// then reads the state again and reports what changed. Returns `false`
    /// once the device is gone.
    fn resync(&mut self, out: &mut Vec<Event>) -> bool {
        debug!(
            "gamepad {} lost events: its state is read again",
            self.id.raw()
        );
        let mut stale = [0u8; RECORD * RECORDS_PER_READ];
        loop {
            match self.read_into(&mut stale) {
                Got::Bytes(_) => {}
                Got::Nothing => break,
                Got::Gone => return false,
            }
        }
        if let Err(error) = self.sync(out) {
            if error.raw_os_error() != Some(ENODEV) {
                warn!(
                    "gamepad {}'s state cannot be read again, and it is taken as unplugged: \
                     {error}",
                    self.id.raw()
                );
            }
            return false;
        }
        true
    }

    fn apply(&mut self, record: Record, out: &mut Vec<Event>) {
        match record.kind {
            EV_KEY => {
                self.layout.apply_key(
                    record.code,
                    record.value != 0,
                    self.id,
                    &mut self.state,
                    out,
                );
            }
            EV_ABS => {
                self.layout
                    .apply_axis(record.code, record.value, self.id, &mut self.state, out);
            }
            _ => {}
        }
    }

    /// Reports the pad gone: at rest first, then disconnected.
    fn disconnect(&mut self, out: &mut Vec<Event>) {
        self.state.rest(self.id, out);
        out.push(Event::GamepadDisconnected(self.id));
        info!(
            "gamepad {} disconnected: {:?}",
            self.id.raw(),
            self.state.name()
        );
    }
}

/// The gamepads plugged in, and the watch on [`INPUT_DIR`] that sees others
/// come.
pub(crate) struct Gamepads {
    /// The inotify instance that watches [`INPUT_DIR`], unless it could not
    /// be made or stopped working.
    watch: Option<File>,
    pads: Vec<Pad>,
    /// The number of the next pad's id.
    next_id: u32,
    /// Whether the first poll, which opens the pads already there, has run.
    scanned: bool,
}

impl Gamepads {
    /// Starts watching [`INPUT_DIR`]. The pads already there are opened by
    /// the first poll, which reports them. Fails never: without a watch, no
    /// pad plugged in later is seen, and a warning says so.
    pub(crate) fn new() -> Self {
        Self {
            watch: watch_input_dir(),
            pads: Vec::new(),
            next_id: 1,
            scanned: false,
        }
    }

    /// Appends what happened to the gamepads since the last poll.
    pub(crate) fn poll(&mut self, out: &mut Vec<Event>) {
        if !self.scanned {
            self.scanned = true;
            self.scan(true, out);
        }
        self.pads.retain_mut(|pad| {
            let alive = pad.read(out);
            if !alive {
                pad.disconnect(out);
            }
            alive
        });
        self.read_watch(out);
    }

    /// The state of the pad `id`, while it is plugged in.
    pub(crate) fn state(&self, id: GamepadId) -> Option<&GamepadState> {
        self.pads
            .iter()
            .find(|pad| pad.id == id)
            .map(|pad| &pad.state)
    }

    /// Opens every gamepad in [`INPUT_DIR`] that is not open yet, in the
    /// order of the nodes' numbers. At the first poll, a gamepad that cannot
    /// be opened for want of permission is logged with a hint.
    fn scan(&mut self, first: bool, out: &mut Vec<Event>) {
        let entries = match fs::read_dir(INPUT_DIR) {
            Ok(entries) => entries,
            Err(error) => {
                debug!("no gamepads: {INPUT_DIR} cannot be listed: {error}");
                return;
            }
        };
        let mut nodes: Vec<String> = entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| is_event_node(name.as_bytes()))
            .collect();
        nodes.sort_by_key(|name| node_number(name));
        for node in &nodes {
            self.open(node, first, out);
        }
    }

    /// Opens the node if it is a gamepad not open yet, and reports it.
    fn open(&mut self, node: &str, hint: bool, out: &mut Vec<Event>) {
        if self.pads.iter().any(|pad| pad.node == node) {
            return;
        }
        let path = Path::new(INPUT_DIR).join(node);
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) => {
                // A keyboard's or a mouse's node is usually closed to the
                // user, and a new node until udev gives it its permissions.
                if hint
                    && error.kind() == io::ErrorKind::PermissionDenied
                    && sysfs_says_gamepad(node)
                {
                    warn!(
                        "the gamepad at {} cannot be opened: {error}. Games read gamepads \
                         through the udev rules that give the user at the desktop seat their \
                         devices (`uaccess`, from systemd's 70-uaccess.rules or the \
                         distribution's game-devices rules); without them, a user in the \
                         `input` group can read every input device",
                        path.display()
                    );
                }
                return;
            }
        };
        let Some(after) = self.next_id.checked_add(1) else {
            warn!(
                "a gamepad at {} is left out: no ids are left",
                path.display()
            );
            return;
        };
        let id = GamepadId::from_raw(self.next_id);
        let mut pad = match Pad::probe(node, file, id) {
            Ok(Some(pad)) => pad,
            Ok(None) => return,
            Err(error) => {
                debug!("{} cannot be described: {error}", path.display());
                return;
            }
        };
        self.next_id = after;
        out.push(Event::GamepadConnected(id));
        if let Err(error) = pad.sync(out) {
            warn!(
                "gamepad {}'s state cannot be read, and it is taken as unplugged: {error}",
                id.raw()
            );
            pad.disconnect(out);
            return;
        }
        self.pads.push(pad);
    }

    /// Reads what the watch saw since the last poll, and opens what it says
    /// may be a new gamepad.
    fn read_watch(&mut self, out: &mut Vec<Event>) {
        // A name is at most 255 bytes and a NUL: a read takes one event at
        // least.
        let mut buffer = [0u8; 4096];
        let mut rescan = false;
        let mut lost = None;
        while let Some(watch) = self.watch.as_mut() {
            let read = match watch.read(&mut buffer) {
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    lost = Some(error.to_string());
                    break;
                }
            };
            if read == 0 {
                break;
            }
            for (mask, name) in inotify_events(&buffer[..read]) {
                if mask & IN_Q_OVERFLOW != 0 {
                    rescan = true;
                }
                if mask & IN_IGNORED != 0 {
                    lost = Some(format!("{INPUT_DIR} is gone"));
                }
                if mask & (IN_CREATE | IN_ATTRIB) != 0
                    && is_event_node(name)
                    && let Ok(node) = core::str::from_utf8(name)
                {
                    self.open(node, false, out);
                }
            }
            if lost.is_some() {
                break;
            }
        }
        if let Some(reason) = lost {
            self.watch = None;
            warn!(
                "stopped watching {INPUT_DIR} ({reason}): a gamepad plugged in from now on is not \
                 seen"
            );
        }
        if rescan {
            // Some events were lost: look at every node.
            self.scan(false, out);
        }
    }
}

/// Watches [`INPUT_DIR`] for nodes made and nodes whose permissions change.
fn watch_input_dir() -> Option<File> {
    let fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
    if fd < 0 {
        warn!(
            "a gamepad plugged in will not be seen: inotify_init1 failed: {}",
            io::Error::last_os_error()
        );
        return None;
    }
    // SAFETY: `inotify_init1` returned a new descriptor, which nothing else
    // owns or closes.
    let watch = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    // SAFETY: the path is a C string, and the descriptor is the inotify
    // instance just made, open while `watch` lives.
    let added = unsafe {
        inotify_add_watch(
            watch.as_raw_fd(),
            INPUT_DIR_C.as_ptr(),
            IN_CREATE | IN_ATTRIB,
        )
    };
    if added < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            debug!("no gamepads: there is no {INPUT_DIR}");
        } else {
            warn!("a gamepad plugged in will not be seen: {INPUT_DIR} cannot be watched: {error}");
        }
        return None;
    }
    Some(watch)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    use nadir_core::testing::check;

    use super::*;

    const PAD: GamepadId = GamepadId::from_raw(1);

    /// The layout of a pad that has these axes, of these ranges.
    fn layout(by_place: bool, axes: &[(u16, Range)]) -> Layout {
        let mut bitmap = [0u8; AXIS_BYTES];
        for &(code, _) in axes {
            bitmap[usize::from(code / 8)] |= 1 << (code % 8);
        }
        let mut layout = Layout::new(by_place, &bitmap);
        for &(code, range) in axes {
            layout.ranges[axis_slot(code).expect("an axis of AXES")] = Some(range);
        }
        layout
    }

    const HAT: Range = Range { min: -1, max: 1 };

    /// An Xbox pad on `xpad`: sticks of -32768 to 32767, triggers of 0 to
    /// 255, and a hat.
    fn xbox() -> Layout {
        let stick = Range {
            min: -32768,
            max: 32767,
        };
        let trigger = Range { min: 0, max: 255 };
        layout(
            false,
            &[
                (ABS_X, stick),
                (ABS_Y, stick),
                (ABS_Z, trigger),
                (ABS_RX, stick),
                (ABS_RY, stick),
                (ABS_RZ, trigger),
                (ABS_HAT0X, HAT),
                (ABS_HAT0Y, HAT),
            ],
        )
    }

    /// A Switch Pro Controller on `hid-nintendo`: by place, triggers only
    /// buttons, and no `ABS_Z` or `ABS_RZ`.
    fn switch_pro() -> Layout {
        let stick = Range {
            min: -32767,
            max: 32767,
        };
        layout(
            true,
            &[
                (ABS_X, stick),
                (ABS_Y, stick),
                (ABS_RX, stick),
                (ABS_RY, stick),
                (ABS_HAT0X, HAT),
                (ABS_HAT0Y, HAT),
            ],
        )
    }

    /// An Xbox pad over Bluetooth on `hid-microsoft`, as SDL's database
    /// maps it: sticks of 0 to 65535, the right one on `ABS_Z` and
    /// `ABS_RZ`, and the triggers on `ABS_BRAKE` and `ABS_GAS`, 0 to 1023.
    fn xbox_bluetooth() -> Layout {
        let stick = Range { min: 0, max: 65535 };
        let trigger = Range { min: 0, max: 1023 };
        layout(
            false,
            &[
                (ABS_X, stick),
                (ABS_Y, stick),
                (ABS_Z, stick),
                (ABS_RZ, stick),
                (ABS_GAS, trigger),
                (ABS_BRAKE, trigger),
                (ABS_HAT0X, HAT),
                (ABS_HAT0Y, HAT),
            ],
        )
    }

    /// A pad of the DirectInput kind on `hid-generic`: the right stick on
    /// `ABS_Z` and `ABS_RZ`, and the triggers only buttons.
    fn direct_input() -> Layout {
        let stick = Range { min: 0, max: 255 };
        layout(
            false,
            &[
                (ABS_X, stick),
                (ABS_Y, stick),
                (ABS_Z, stick),
                (ABS_RZ, stick),
                (ABS_HAT0X, HAT),
                (ABS_HAT0Y, HAT),
            ],
        )
    }

    fn every_layout() -> [Layout; 4] {
        [xbox(), switch_pro(), xbox_bluetooth(), direct_input()]
    }

    fn record(kind: u16, code: u16, value: i32) -> Record {
        Record { kind, code, value }
    }

    /// The bytes of a record, as the kernel writes them.
    fn bytes(record: Record) -> [u8; RECORD] {
        let mut bytes = [0u8; RECORD];
        let at = |offset: usize, field: &[u8], bytes: &mut [u8; RECORD]| {
            bytes[offset..offset + field.len()].copy_from_slice(field);
        };
        at(
            offset_of!(input_event, type_),
            &record.kind.to_ne_bytes(),
            &mut bytes,
        );
        at(
            offset_of!(input_event, code),
            &record.code.to_ne_bytes(),
            &mut bytes,
        );
        at(
            offset_of!(input_event, value),
            &record.value.to_ne_bytes(),
            &mut bytes,
        );
        bytes
    }

    fn button(button: GamepadButton, pressed: bool) -> Event {
        Event::GamepadButton {
            id: PAD,
            button,
            pressed,
        }
    }

    fn axis(axis: GamepadAxis, value: f32) -> Event {
        Event::GamepadAxis {
            id: PAD,
            axis,
            value,
        }
    }

    #[test]
    fn the_face_buttons_are_read_by_label_on_xbox_pads_and_by_place_on_others() {
        let by_label = xbox();
        // `xpad`: BTN_A, BTN_B, BTN_X and BTN_Y, by the pad's labels.
        assert_eq!(
            by_label.key(BTN_SOUTH),
            Some(Control::Button(GamepadButton::A))
        );
        assert_eq!(
            by_label.key(BTN_EAST),
            Some(Control::Button(GamepadButton::B))
        );
        assert_eq!(
            by_label.key(nadir_sys::linux::evdev::BTN_X),
            Some(Control::Button(GamepadButton::X))
        );
        assert_eq!(
            by_label.key(nadir_sys::linux::evdev::BTN_Y),
            Some(Control::Button(GamepadButton::Y))
        );
        // `hid-playstation` and `hid-nintendo`: by place, so the top
        // button (△, Nintendo X) is Y, and the left one (□, Nintendo Y) X.
        let by_place = switch_pro();
        assert_eq!(
            by_place.key(BTN_SOUTH),
            Some(Control::Button(GamepadButton::A))
        );
        assert_eq!(
            by_place.key(BTN_EAST),
            Some(Control::Button(GamepadButton::B))
        );
        assert_eq!(
            by_place.key(BTN_NORTH),
            Some(Control::Button(GamepadButton::Y))
        );
        assert_eq!(
            by_place.key(BTN_WEST),
            Some(Control::Button(GamepadButton::X))
        );
    }

    #[test]
    fn the_other_buttons_have_one_code_each() {
        let expected = [
            (BTN_TL, GamepadButton::LeftBumper),
            (BTN_TR, GamepadButton::RightBumper),
            (BTN_SELECT, GamepadButton::Back),
            (BTN_START, GamepadButton::Start),
            (BTN_MODE, GamepadButton::Guide),
            (BTN_THUMBL, GamepadButton::LeftStick),
            (BTN_THUMBR, GamepadButton::RightStick),
            (BTN_DPAD_UP, GamepadButton::DPadUp),
            (BTN_DPAD_DOWN, GamepadButton::DPadDown),
            (BTN_DPAD_LEFT, GamepadButton::DPadLeft),
            (BTN_DPAD_RIGHT, GamepadButton::DPadRight),
        ];
        for layout in every_layout() {
            for (code, button) in expected {
                assert_eq!(layout.key(code), Some(Control::Button(button)), "{code:#x}");
            }
            // Keys of keyboards, and the gamepad codes no standard pad
            // has (BTN_C, BTN_Z), drive nothing.
            for code in [1, 30, 0x132, 0x135, 0x13f, 0x224] {
                assert_eq!(layout.key(code), None, "{code:#x}");
            }
        }
    }

    #[test]
    fn every_button_and_axis_is_reached_on_every_layout() {
        for layout in every_layout() {
            let mut buttons = Vec::new();
            let mut axes = Vec::new();
            for code in KEYS {
                match layout.key(code) {
                    Some(Control::Button(button)) => buttons.push(button),
                    Some(Control::Trigger(axis)) => axes.push(axis),
                    None => {}
                }
            }
            for (slot, role) in layout.roles.iter().enumerate() {
                let Some(target) = role else {
                    continue;
                };
                if layout.ranges[slot].is_none() {
                    continue;
                }
                match *target {
                    Target::Stick(axis) | Target::Trigger(axis) => axes.push(axis),
                    Target::Hat { negative, positive } => buttons.extend([negative, positive]),
                }
            }
            for &wanted in GamepadButton::ALL {
                assert!(buttons.contains(&wanted), "{layout:?}: {wanted:?}");
            }
            for &wanted in GamepadAxis::ALL {
                assert_eq!(
                    axes.iter().filter(|&&axis| axis == wanted).count(),
                    1,
                    "{layout:?}: {wanted:?}"
                );
            }
        }
    }

    #[test]
    fn a_trigger_button_drives_its_axis_only_without_an_analog_trigger() {
        // An Xbox pad has ABS_Z and ABS_RZ; a DualSense has them and
        // BTN_TL2 and BTN_TR2 too. The buttons are left out.
        assert_eq!(xbox().key(BTN_TL2), None);
        assert_eq!(xbox().key(BTN_TR2), None);
        let switch = switch_pro();
        assert_eq!(
            switch.key(BTN_TL2),
            Some(Control::Trigger(GamepadAxis::LeftTrigger))
        );
        assert_eq!(
            switch.key(BTN_TR2),
            Some(Control::Trigger(GamepadAxis::RightTrigger))
        );

        let mut state = GamepadState::new(String::new());
        let mut out = Vec::new();
        switch.apply_key(BTN_TR2, true, PAD, &mut state, &mut out);
        switch.apply_key(BTN_TR2, false, PAD, &mut state, &mut out);
        assert_eq!(
            out,
            [
                axis(GamepadAxis::RightTrigger, 1.0),
                axis(GamepadAxis::RightTrigger, 0.0)
            ]
        );
    }

    #[test]
    fn an_xbox_pad_over_bluetooth_has_its_right_stick_on_z_and_rz() {
        let layout = xbox_bluetooth();
        let mut state = GamepadState::new(String::new());
        let mut out = Vec::new();
        // At rest, the middle of 0 to 65535: the stick at 0, not a trigger
        // half pulled.
        layout.apply_axis(ABS_Z, 32768, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_RZ, 32768, PAD, &mut state, &mut out);
        assert!(out.is_empty(), "{out:?}");
        layout.apply_axis(ABS_Z, 65535, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_RZ, 0, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_BRAKE, 1023, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_GAS, 0, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_GAS, 1023, PAD, &mut state, &mut out);
        assert_eq!(
            out,
            [
                axis(GamepadAxis::RightX, 1.0),
                axis(GamepadAxis::RightY, -1.0),
                axis(GamepadAxis::LeftTrigger, 1.0),
                axis(GamepadAxis::RightTrigger, 1.0),
            ]
        );
        // The triggers are analog, so their buttons are left out.
        assert_eq!(layout.key(BTN_TL2), None);
        assert_eq!(layout.key(BTN_TR2), None);
    }

    #[test]
    fn a_pad_with_its_right_stick_on_z_and_no_analog_trigger_has_trigger_buttons() {
        let layout = direct_input();
        assert_eq!(
            layout.key(BTN_TL2),
            Some(Control::Trigger(GamepadAxis::LeftTrigger))
        );
        assert_eq!(
            layout.key(BTN_TR2),
            Some(Control::Trigger(GamepadAxis::RightTrigger))
        );
        let mut state = GamepadState::new(String::new());
        let mut out = Vec::new();
        layout.apply_axis(ABS_RZ, 255, PAD, &mut state, &mut out);
        assert_eq!(out, [axis(GamepadAxis::RightY, 1.0)]);
    }

    #[test]
    fn xpads_z_and_rz_are_triggers_and_brake_and_gas_are_left_out() {
        // A pad with both: Z and RZ drive the triggers, and nothing else
        // does.
        let stick = Range {
            min: -32768,
            max: 32767,
        };
        let trigger = Range { min: 0, max: 255 };
        let both = layout(
            false,
            &[
                (ABS_RX, stick),
                (ABS_RY, stick),
                (ABS_Z, trigger),
                (ABS_RZ, trigger),
                (ABS_GAS, trigger),
                (ABS_BRAKE, trigger),
            ],
        );
        let role = |code| both.roles[axis_slot(code).expect("an axis")];
        assert_eq!(role(ABS_Z), Some(Target::Trigger(GamepadAxis::LeftTrigger)));
        assert_eq!(
            role(ABS_RZ),
            Some(Target::Trigger(GamepadAxis::RightTrigger))
        );
        assert_eq!(role(ABS_GAS), None);
        assert_eq!(role(ABS_BRAKE), None);
        // A pad of RX and RY with no Z or RZ takes BRAKE and GAS.
        let brake_and_gas = layout(
            false,
            &[
                (ABS_RX, stick),
                (ABS_RY, stick),
                (ABS_BRAKE, trigger),
                (ABS_GAS, trigger),
            ],
        );
        let role = |code| brake_and_gas.roles[axis_slot(code).expect("an axis")];
        assert_eq!(
            role(ABS_BRAKE),
            Some(Target::Trigger(GamepadAxis::LeftTrigger))
        );
        assert_eq!(
            role(ABS_GAS),
            Some(Target::Trigger(GamepadAxis::RightTrigger))
        );
    }

    #[test]
    fn sticks_go_from_minus_one_through_zero_to_one() {
        let xpad = Range {
            min: -32768,
            max: 32767,
        };
        assert_eq!(xpad.stick(-32768), -1.0);
        assert_eq!(xpad.stick(0), 0.0);
        assert_eq!(xpad.stick(32767), 1.0);
        assert!((xpad.stick(16384) - 0.5).abs() < 1e-4);
        assert!((xpad.stick(-16384) + 0.5).abs() < 1e-4);
        // `hid-playstation`: 0 to 255, at rest at 128.
        let dualsense = Range { min: 0, max: 255 };
        assert_eq!(dualsense.stick(0), -1.0);
        assert_eq!(dualsense.stick(128), 0.0);
        assert_eq!(dualsense.stick(255), 1.0);
        // `hid-nintendo`: -32767 to 32767.
        let switch = Range {
            min: -32767,
            max: 32767,
        };
        assert_eq!(switch.stick(-32767), -1.0);
        assert_eq!(switch.stick(0), 0.0);
        assert_eq!(switch.stick(32767), 1.0);
        // Out of range, as a driver's calibration may give, is clamped.
        assert_eq!(dualsense.stick(-5), -1.0);
        assert_eq!(dualsense.stick(300), 1.0);
        // A range of two values has ends, and no middle.
        let two = Range { min: 0, max: 1 };
        assert_eq!(two.stick(0), -1.0);
        assert_eq!(two.stick(1), 1.0);
        assert_eq!(two.stick(2), 1.0);
    }

    #[test]
    fn stick_values_stay_in_range_and_grow_with_the_input() {
        check(
            "a stick's value is in -1..=1 and monotonic",
            500,
            |rng| {
                let min = rng.range(-70_000..70_000);
                let max = min + rng.range(1..140_000);
                let a = rng.range(min - 1000..max + 1000);
                let b = rng.range(min - 1000..max + 1000);
                (Range { min, max }, a.min(b), a.max(b))
            },
            |&(range, low, high)| {
                let (from, to) = (range.stick(low), range.stick(high));
                if !(-1.0..=1.0).contains(&from) || !(-1.0..=1.0).contains(&to) {
                    return Err(format!("{from} or {to} out of range"));
                }
                if from > to {
                    return Err(format!("{low} gives {from}, more than {high}'s {to}"));
                }
                if range.stick(range.min) != -1.0 || range.stick(range.max) != 1.0 {
                    return Err("the ends are not -1 and 1".to_string());
                }
                Ok(())
            },
        );
    }

    #[test]
    fn triggers_go_from_zero_to_one() {
        let xbox_360 = Range { min: 0, max: 255 };
        assert_eq!(xbox_360.trigger(0), 0.0);
        assert_eq!(xbox_360.trigger(255), 1.0);
        assert!((xbox_360.trigger(51) - 0.2).abs() < 1e-6);
        let xbox_one = Range { min: 0, max: 1023 };
        assert_eq!(xbox_one.trigger(1023), 1.0);
        assert_eq!(xbox_one.trigger(-1), 0.0);
        assert_eq!(xbox_one.trigger(2000), 1.0);
        check(
            "a trigger's value is in 0..=1 and monotonic",
            500,
            |rng| {
                let min = rng.range(-70_000..70_000);
                let max = min + rng.range(1..140_000);
                let a = rng.range(min - 1000..max + 1000);
                let b = rng.range(min - 1000..max + 1000);
                (Range { min, max }, a.min(b), a.max(b))
            },
            |&(range, low, high)| {
                let (from, to) = (range.trigger(low), range.trigger(high));
                if !(0.0..=1.0).contains(&from) || !(0.0..=1.0).contains(&to) || from > to {
                    return Err(format!("{from} and {to}"));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn an_empty_range_is_no_axis() {
        let info = |minimum, maximum| input_absinfo {
            minimum,
            maximum,
            ..input_absinfo::default()
        };
        assert_eq!(Range::new(&info(0, 0)), None);
        assert_eq!(Range::new(&info(5, -5)), None);
        assert_eq!(Range::new(&info(-1, 1)), Some(Range { min: -1, max: 1 }));
    }

    #[test]
    fn the_hat_presses_one_side_at_a_time() {
        let layout = xbox();
        let mut state = GamepadState::new(String::new());
        let mut out = Vec::new();
        layout.apply_axis(ABS_HAT0X, -1, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_HAT0X, 1, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_HAT0Y, 1, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_HAT0X, 0, PAD, &mut state, &mut out);
        layout.apply_axis(ABS_HAT0Y, 0, PAD, &mut state, &mut out);
        assert_eq!(
            out,
            [
                button(GamepadButton::DPadLeft, true),
                button(GamepadButton::DPadLeft, false),
                button(GamepadButton::DPadRight, true),
                button(GamepadButton::DPadDown, true),
                button(GamepadButton::DPadRight, false),
                button(GamepadButton::DPadDown, false),
            ]
        );
    }

    #[test]
    fn an_axis_the_pad_lacks_or_no_standard_pad_has_is_ignored() {
        let layout = switch_pro();
        let mut state = GamepadState::new(String::new());
        let mut out = Vec::new();
        // ABS_Z: this pad has no analog trigger. ABS_MISC (0x28): none has.
        layout.apply_axis(ABS_Z, 200, PAD, &mut state, &mut out);
        layout.apply_axis(0x28, 200, PAD, &mut state, &mut out);
        assert!(out.is_empty());
        // +Y is down, as the kernel and the engine both have it.
        layout.apply_axis(ABS_RY, 32767, PAD, &mut state, &mut out);
        assert_eq!(out, [axis(GamepadAxis::RightY, 1.0)]);
    }

    #[test]
    fn a_report_lost_in_part_is_dropped_and_the_state_read_again() {
        let mut decoder = Decoder::default();
        let press = record(EV_KEY, BTN_SOUTH, 1);
        assert_eq!(decoder.step(press), Step::Apply(press));
        assert_eq!(decoder.step(record(EV_SYN, SYN_REPORT, 0)), Step::Skip);
        assert_eq!(decoder.step(record(EV_SYN, SYN_DROPPED, 0)), Step::Skip);
        assert_eq!(decoder.step(record(EV_ABS, ABS_X, 7)), Step::Skip);
        assert_eq!(decoder.step(record(EV_KEY, BTN_EAST, 1)), Step::Skip);
        assert_eq!(decoder.step(record(EV_SYN, SYN_REPORT, 0)), Step::Resync);
        let release = record(EV_KEY, BTN_SOUTH, 0);
        assert_eq!(decoder.step(release), Step::Apply(release));
        assert_eq!(decoder.step(record(EV_SYN, SYN_REPORT, 0)), Step::Skip);
    }

    #[test]
    fn a_record_is_read_from_the_kernels_bytes() {
        let written = record(EV_ABS, ABS_RY, -12345);
        assert_eq!(Record::parse(&bytes(written)), written);
    }

    #[test]
    fn only_event_nodes_are_opened() {
        for name in ["event0", "event23", "event1024"] {
            assert!(is_event_node(name.as_bytes()), "{name}");
        }
        for name in [
            "event", "js0", "mice", "mouse1", "event1a", "by-id", "xevent1",
        ] {
            assert!(!is_event_node(name.as_bytes()), "{name}");
        }
        let mut nodes = ["event10", "event2", "event1"];
        nodes.sort_by_key(|name| node_number(name));
        assert_eq!(nodes, ["event1", "event2", "event10"]);
    }

    #[test]
    fn inotify_events_are_split_and_their_names_unpadded() {
        let event = |mask: u32, name: &[u8], padded: usize| {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&1i32.to_ne_bytes());
            bytes.extend_from_slice(&mask.to_ne_bytes());
            bytes.extend_from_slice(&0u32.to_ne_bytes());
            bytes.extend_from_slice(&u32::try_from(padded).expect("short").to_ne_bytes());
            bytes.extend_from_slice(name);
            bytes.resize(bytes.len() + padded - name.len(), 0);
            bytes
        };
        let mut read = event(IN_CREATE, b"event7", 16);
        read.extend(event(IN_ATTRIB, b"js0", 16));
        read.extend(event(IN_Q_OVERFLOW, b"", 0));
        let events: Vec<(u32, &[u8])> = inotify_events(&read).collect();
        assert_eq!(
            events,
            [
                (IN_CREATE, &b"event7"[..]),
                (IN_ATTRIB, &b"js0"[..]),
                (IN_Q_OVERFLOW, &b""[..])
            ]
        );
        // A read cut short ends the events, and never panics.
        for end in 0..read.len() {
            let _ = inotify_events(&read[..end]).count();
        }
        assert_eq!(inotify_events(&read[..20]).count(), 0);
    }

    #[test]
    fn sysfs_bitmaps_are_read_from_the_lowest_word_at_the_end() {
        // An Xbox 360 pad on `xpad`: its `ev` and `key` capabilities.
        let types = "20000b";
        let keys = "7cdb000000000000 0 0 0 0";
        assert!(sysfs_bit(types, EV_KEY));
        assert!(sysfs_bit(types, EV_ABS));
        assert!(!sysfs_bit(types, 2));
        assert!(sysfs_bit(keys, BTN_SOUTH));
        assert!(sysfs_bit(keys, BTN_MODE));
        assert!(!sysfs_bit(keys, 0x132));
        // A keyboard has no gamepad button, and its higher words are left
        // out.
        assert!(!sysfs_bit("1000000000007 ff9f207ac14057ff", BTN_SOUTH));
        assert!(!sysfs_bit("not hex", 0));
    }

    #[test]
    fn a_request_of_another_size_is_refused_before_it_is_sent() {
        let file = File::open("/proc/self/stat").expect("open a file");
        let mut short = [0u8; 4];
        let error = read_request(&file, EVIOCGID, &mut short).expect_err("the wrong size");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        // The right size goes to the driver, which takes no such request:
        // `ENOTTY` (`asm-generic/errno-base.h`: `#define ENOTTY 25`).
        let mut id = input_id::default();
        let error = read_request(&file, EVIOCGID, &mut id).expect_err("no device");
        assert_eq!(error.raw_os_error(), Some(25), "{error}");
    }

    #[test]
    fn every_axis_request_is_its_axis_s() {
        // EVIOCGABS(abs) is EVIOCGABS(0) plus the axis's code.
        for &(code, request) in &AXES {
            assert_eq!(request, EVIOCGABS_X + c_ulong::from(code), "{code:#x}");
        }
        assert_eq!(INPUT_DIR_C.to_str(), Ok(INPUT_DIR));
    }

    /// A pad whose "device" is one end of a socket pair: the test writes the
    /// kernel's records into the other end.
    fn fake_pad(layout: Layout) -> (Pad, UnixStream) {
        let (device, kernel) = UnixStream::pair().expect("a socket pair");
        device.set_nonblocking(true).expect("non-blocking");
        let pad = Pad {
            id: PAD,
            node: "event99".to_string(),
            file: File::from(OwnedFd::from(device)),
            keys: [0; KEY_BYTES],
            axes: [0; AXIS_BYTES],
            layout,
            decoder: Decoder::default(),
            state: GamepadState::new("fake pad".to_string()),
        };
        (pad, kernel)
    }

    fn send(kernel: &mut UnixStream, records: &[Record]) {
        let mut all = Vec::new();
        for &record in records {
            all.extend_from_slice(&bytes(record));
        }
        kernel.write_all(&all).expect("write the records");
    }

    #[test]
    fn a_pad_reports_what_its_device_sends_and_its_unplugging() {
        let (mut pad, mut kernel) = fake_pad(xbox());
        let mut out = Vec::new();
        assert!(pad.read(&mut out), "nothing sent yet");
        assert!(out.is_empty());

        // More than one read's worth, so that the loop reads again.
        let mut records = vec![
            record(EV_KEY, BTN_SOUTH, 1),
            record(EV_ABS, ABS_X, 32767),
            record(EV_ABS, ABS_RZ, 255),
            record(EV_SYN, SYN_REPORT, 0),
        ];
        for _ in 0..RECORDS_PER_READ {
            records.push(record(EV_ABS, ABS_Y, 0));
        }
        records.push(record(EV_KEY, nadir_sys::linux::evdev::BTN_X, 1));
        send(&mut kernel, &records);
        assert!(pad.read(&mut out));
        assert_eq!(
            out,
            [
                button(GamepadButton::A, true),
                axis(GamepadAxis::LeftX, 1.0),
                axis(GamepadAxis::RightTrigger, 1.0),
                button(GamepadButton::X, true),
            ]
        );
        assert!(pad.state.pressed(GamepadButton::X));

        // Unplugged: the device ends, which a real one does with ENODEV.
        drop(kernel);
        out.clear();
        assert!(!pad.read(&mut out));
        pad.disconnect(&mut out);
        assert_eq!(
            out,
            [
                button(GamepadButton::A, false),
                button(GamepadButton::X, false),
                axis(GamepadAxis::LeftX, 0.0),
                axis(GamepadAxis::RightTrigger, 0.0),
                Event::GamepadDisconnected(PAD),
            ]
        );
    }

    #[test]
    fn the_records_read_and_queued_after_a_lost_report_are_dropped() {
        // The stand-in has no keys and no axes, so reading its state again
        // reads nothing, and succeeds.
        let (mut pad, mut kernel) = fake_pad(xbox());
        let mut records = vec![
            record(EV_KEY, BTN_SOUTH, 1),
            record(EV_SYN, SYN_REPORT, 0),
            record(EV_SYN, SYN_DROPPED, 0),
            record(EV_KEY, BTN_EAST, 1),
            record(EV_SYN, SYN_REPORT, 0),
            // Older than the state read again: in the same read, and past it
            // in the queue.
            record(EV_KEY, nadir_sys::linux::evdev::BTN_X, 1),
            record(EV_SYN, SYN_REPORT, 0),
        ];
        for value in 0..RECORDS_PER_READ * 2 {
            records.push(record(EV_KEY, BTN_TL, i32::from(value % 2 == 0)));
        }
        records.push(record(EV_KEY, BTN_START, 1));
        send(&mut kernel, &records);
        let mut out = Vec::new();
        assert!(pad.read(&mut out));
        assert_eq!(out, [button(GamepadButton::A, true)]);
        // What comes after is read as usual.
        send(&mut kernel, &[record(EV_KEY, BTN_MODE, 1)]);
        out.clear();
        assert!(pad.read(&mut out));
        assert_eq!(out, [button(GamepadButton::Guide, true)]);
    }

    #[test]
    fn a_lost_report_on_a_device_that_cannot_be_read_again_unplugs_it() {
        let (mut pad, mut kernel) = fake_pad(xbox());
        // With a key, the state is read with `EVIOCGKEY`, which a socket
        // does not take, as a device that went away would not.
        pad.keys[usize::from(BTN_SOUTH / 8)] |= 1 << (BTN_SOUTH % 8);
        send(
            &mut kernel,
            &[
                record(EV_SYN, SYN_DROPPED, 0),
                record(EV_KEY, BTN_SOUTH, 1),
                record(EV_SYN, SYN_REPORT, 0),
            ],
        );
        let mut out = Vec::new();
        // The pad is taken as unplugged, with nothing of the lost report
        // applied.
        assert!(!pad.read(&mut out));
        assert!(out.is_empty());
    }

    #[cfg(feature = "dev")]
    #[test]
    fn reading_a_pad_allocates_nothing() {
        let (mut pad, mut kernel) = fake_pad(xbox());
        let mut out = Vec::with_capacity(64);
        send(
            &mut kernel,
            &[
                record(EV_KEY, BTN_SOUTH, 1),
                record(EV_ABS, ABS_RX, -32768),
                record(EV_ABS, ABS_HAT0Y, -1),
                record(EV_SYN, SYN_REPORT, 0),
            ],
        );
        let start = crate::thread_alloc_counts();
        assert!(pad.read(&mut out));
        assert!(pad.read(&mut out), "and again, with nothing to read");
        assert_eq!(crate::thread_alloc_counts().since(start).allocations, 0);
        assert_eq!(out.len(), 3);
    }
}
