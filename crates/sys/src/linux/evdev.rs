//! Linux input event codes: the kernel's numbers for keys.
//!
//! Source: `<linux/input-event-codes.h>`. Each constant quotes its `#define`.
//!
//! Only the codes that `nadir-platform` maps to a key are declared, in the
//! header's order. The type is that of the `code` field of the kernel's
//! `struct input_event`, `__u16`.
//!
//! These are also what an X server reports, shifted: on a server whose
//! keyboard rules are `evdev`, which is every current Linux desktop and
//! XWayland, an X keycode is one of these codes plus 8.

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
