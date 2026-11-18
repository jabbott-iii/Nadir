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

//! The save file, `<slot>.ksav`, version 1: a game's saved entities and
//! resources, which nadir-scene writes and reads (roadmap M9.4, ADR-0068).
//!
//! ```text
//! header (24 bytes):
//!   "KSAV" | version u16 (1) | game_version_len u16 | crc32 u32
//!   | saved_at i64 | payload_size u32
//! game version: game_version_len bytes of UTF-8
//! payload: payload_size bytes of KBJ, to the end of the file
//! ```
//!
//! Numbers are little-endian. `saved_at` is when the game saved, in
//! seconds since 1970-01-01 00:00:00 UTC; `crc32` is the CRC-32
//! ([`nadir_core::hash::crc32`]) of every byte after it, from `saved_at` to
//! the end of the file, so that a damaged timestamp or game version is
//! caught as a damaged payload is.
//!
//! The payload, version 1, is a [`SaveData`], which [`read`] checks and
//! [`SaveData::to_value`] writes:
//!
//! ```text
//! { "next_id": 12,
//!   "globals": { "Score": { "points": 40 } },
//!   "entities": [ { "id": 3, "prefab": "slime",
//!                   "components": { "Health": { "current": 4, "max": 12 } } } ] }
//! ```
//!
//! # The checks
//!
//! Each in the order given, the first that fails the error:
//!
//! - **[`header`]**, of the container: the magic; a whole header; the
//!   version, from 1 to [`VERSION`] (an older one is read, for its reader
//!   to migrate the payload: none is older yet); the game version within
//!   the file; a payload of at most [`Limits::max_decoded_bytes`], which
//!   ends where the file does; the CRC-32; the game version UTF-8.
//! - **[`parse`]:** those, then the payload's KBJ, decoded within the
//!   limits. A KBJ error is [`ErrorKind::BadPayload`], at its byte in the
//!   file.
//! - **[`read`]**, of version 1's payload: an object of `next_id`,
//!   `globals` and `entities`, and no other member; `next_id` a save id;
//!   `globals` an object of type names ([`prefab::is_component_name`]);
//!   `entities` an array of objects, each of an `id`, a save id no other
//!   entity has, `components`, an object of type names, and, if the entity
//!   was spawned of a prefab, its `prefab` ([`prefab::is_name`]), and no
//!   other member. What the values mean is checked where the game reflects
//!   them.
//!
//! A save id is a `u64`, written as reflection writes one (ADR-0020): an
//! integer, or, past what an `i64` holds, a string of its digits
//! ([`id_value`], [`read_id`]).
//!
//! # Example
//!
//! ```
//! use nadir_codec::json::Value;
//! use nadir_codec::{Limits, ksav};
//!
//! let mut data = ksav::SaveData::new(2);
//! let mut hero = ksav::SavedEntity::new(1);
//! hero.components.push(("Health".to_string(), Value::Int(12)));
//! data.entities.push(hero);
//!
//! let bytes = ksav::encode("1.0.3", 1_790_000_000, &data.to_value(), Limits::default())?;
//! assert!(bytes.starts_with(b"KSAV"));
//! let (header, payload) = ksav::parse(&bytes, Limits::default())?;
//! assert_eq!((header.version, header.game_version.as_str()), (1, "1.0.3"));
//! assert_eq!(header.saved_at, 1_790_000_000);
//! assert_eq!(ksav::read(&payload)?, data);
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```

use std::fmt;

use nadir_core::hash::crc32;

use crate::json::Value;
use crate::{CodecError, Limits, kbj, prefab};

/// The first four bytes of a save file.
pub const MAGIC: [u8; 4] = *b"KSAV";

/// The version this module writes, and the newest it reads.
pub const VERSION: u16 = 1;

/// The size of the header, before the game version, in bytes.
pub const HEADER_SIZE: usize = 24;

/// Where the bytes the CRC-32 covers start: just after it.
const CHECKED_FROM: usize = 12;

/// A save file's header, read with [`header`] or [`parse`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Header {
    /// The version of the format the file was written in: from 1 to
    /// [`VERSION`].
    pub version: u16,
    /// The version of the game that wrote it, as the game gives it.
    pub game_version: String,
    /// When it was written, in seconds since 1970-01-01 00:00:00 UTC.
    pub saved_at: i64,
    /// How many bytes its payload is.
    pub payload_size: u32,
}

impl Header {
    /// Where the payload starts, in bytes from the start of the file.
    pub fn payload_offset(&self) -> usize {
        HEADER_SIZE + self.game_version.len()
    }
}

/// What is wrong with a save file. See [`CodecError::Ksav`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// The file does not start with [`MAGIC`].
    NotKsav,
    /// The version is 0, or newer than [`VERSION`].
    Version {
        /// The version the file gives.
        found: u16,
    },
    /// The payload is larger than [`Limits::max_decoded_bytes`].
    PayloadTooLarge {
        /// Its size, as the header gives it.
        size: u32,
        /// The limit.
        limit: usize,
    },
    /// Bytes follow the payload.
    TrailingBytes {
        /// How many.
        extra: usize,
    },
    /// The CRC-32 of the bytes after it is not the header's: the file is
    /// damaged.
    Damaged {
        /// The header's.
        expected: u32,
        /// The bytes'.
        found: u32,
    },
    /// The game version is not UTF-8.
    BadGameVersion,
    /// The payload is not KBJ that can be read within the limits.
    BadPayload {
        /// What KBJ found wrong.
        kind: kbj::ErrorKind,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { expected } => write!(f, "the file ends before {expected}"),
            Self::NotKsav => write!(f, "not a save file: it does not start with `KSAV`"),
            Self::Version { found } => write!(
                f,
                "version {found} of the save file is not supported: this reads 1 to {VERSION}"
            ),
            Self::PayloadTooLarge { size, limit } => {
                write!(
                    f,
                    "the payload of {size} bytes is over the limit of {limit}"
                )
            }
            Self::TrailingBytes { extra } => {
                write!(f, "{extra} bytes follow the payload")
            }
            Self::Damaged { expected, found } => write!(
                f,
                "the file is damaged: its CRC-32 is {found:#010x}, not {expected:#010x}"
            ),
            Self::BadGameVersion => write!(f, "the game version is not UTF-8"),
            Self::BadPayload { kind } => write!(f, "the payload is not KBJ: {kind}"),
        }
    }
}

/// Why a save file cannot be written. See [`CodecError::KsavEncode`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeErrorKind {
    /// The game version is longer than 65,535 bytes.
    GameVersionTooLong {
        /// Its length.
        len: usize,
    },
    /// The payload's KBJ is larger than [`Limits::max_decoded_bytes`], or
    /// than a `u32` holds, so that the file could not be read back.
    PayloadTooLarge {
        /// Its size.
        size: usize,
        /// The limit.
        limit: usize,
    },
}

impl fmt::Display for EncodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GameVersionTooLong { len } => write!(
                f,
                "the game version is {len} bytes long, over the 65,535 a save file holds"
            ),
            Self::PayloadTooLarge { size, limit } => {
                write!(
                    f,
                    "the payload's {size} bytes are over the limit of {limit}"
                )
            }
        }
    }
}

/// What is wrong with a save file's payload. See [`CodecError::KsavData`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataErrorKind {
    /// A value is not of the type its member needs.
    WrongType {
        /// What it should be: "an object", "a string" and so on.
        expected: &'static str,
    },
    /// An object lacks a member.
    MissingMember {
        /// The member.
        name: &'static str,
    },
    /// An object has a member the format does not have.
    UnknownMember {
        /// The member.
        name: String,
    },
    /// A save id is not a `u64` as reflection writes one: an integer of 0
    /// or more, or, past what an `i64` holds, a string of its digits.
    BadId,
    /// Two entities have one save id.
    DuplicateId {
        /// The id.
        id: u64,
    },
    /// A prefab's name is not lowercase letters, digits and `_`.
    BadPrefabName,
    /// A component's or a resource's type name is not a letter then
    /// letters, digits and `_`.
    BadTypeName {
        /// The name.
        name: String,
    },
}

impl fmt::Display for DataErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongType { expected } => write!(f, "expected {expected}"),
            Self::MissingMember { name } => write!(f, "`{name}` is missing"),
            Self::UnknownMember { name } => write!(f, "`{name}` is not a member it has"),
            Self::BadId => write!(
                f,
                "a save id is an integer of 0 or more, or a string of its digits past an i64"
            ),
            Self::DuplicateId { id } => write!(f, "another entity has the save id {id}"),
            Self::BadPrefabName => write!(
                f,
                "a prefab's name is lowercase letters, digits and `_`, one at least"
            ),
            Self::BadTypeName { name } => write!(
                f,
                "`{name}` is not a type's name: a letter, then letters, digits and `_`"
            ),
        }
    }
}

/// A save file's payload, version 1: the counter that gives out save ids,
/// the resources saved, and the entities saved.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SaveData {
    /// The save id the game gives out next.
    pub next_id: u64,
    /// The resources saved, by type name, in the order written.
    pub globals: Vec<(String, Value)>,
    /// The entities saved, in the order written.
    pub entities: Vec<SavedEntity>,
}

impl SaveData {
    /// A save with no resources and no entities, whose counter gives out
    /// `next_id` next.
    pub fn new(next_id: u64) -> Self {
        Self {
            next_id,
            globals: Vec::new(),
            entities: Vec::new(),
        }
    }

    /// The payload as a [`Value`], as [`read`] reads it: `next_id`,
    /// `globals` and `entities`, in that order, and each entity's `id`,
    /// `prefab` if it has one, and `components`.
    pub fn to_value(&self) -> Value {
        let entities = self
            .entities
            .iter()
            .map(|entity| {
                let mut members = vec![("id".to_string(), id_value(entity.id))];
                if let Some(prefab) = &entity.prefab {
                    members.push(("prefab".to_string(), Value::String(prefab.clone())));
                }
                members.push((
                    "components".to_string(),
                    Value::Object(entity.components.clone()),
                ));
                Value::Object(members)
            })
            .collect();
        Value::Object(vec![
            ("next_id".to_string(), id_value(self.next_id)),
            ("globals".to_string(), Value::Object(self.globals.clone())),
            ("entities".to_string(), Value::Array(entities)),
        ])
    }
}

/// One entity of a save: its save id, the prefab it was spawned of, if
/// any, and its components saved.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SavedEntity {
    /// Its save id, which no other entity of the save has.
    pub id: u64,
    /// The prefab it was spawned of, which a load spawns it of again, its
    /// components applied over the prefab's.
    pub prefab: Option<String>,
    /// Its components, by type name, in the order written.
    pub components: Vec<(String, Value)>,
}

impl SavedEntity {
    /// An entity of save id `id`, of no prefab, with no components.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            prefab: None,
            components: Vec::new(),
        }
    }
}

/// A save id as a [`Value`]: an integer, or, past what an `i64` holds, a
/// string of its digits (ADR-0020).
pub fn id_value(id: u64) -> Value {
    match i64::try_from(id) {
        Ok(int) => Value::Int(int),
        Err(_) => Value::String(id.to_string()),
    }
}

/// The save id `value` writes, as [`id_value`] writes one: an integer of 0
/// or more, or a string of the digits of one past what an `i64` holds,
/// with no leading zero. `None` for anything else.
pub fn read_id(value: &Value) -> Option<u64> {
    match value {
        Value::Int(int) => u64::try_from(*int).ok(),
        Value::String(digits) => {
            let id: u64 = digits.parse().ok()?;
            (i64::try_from(id).is_err() && *digits == id.to_string()).then_some(id)
        }
        _ => None,
    }
}

/// Writes a save file: version [`VERSION`]'s header, with `game_version`
/// and `saved_at` (seconds since 1970-01-01 00:00:00 UTC), and `payload`
/// as KBJ within `limits`, so that the file reads back with the same
/// limits.
///
/// # Errors
///
/// [`CodecError::KbjEncode`] if `payload` cannot be KBJ within `limits`,
/// and [`CodecError::KsavEncode`] if the game version is longer than
/// 65,535 bytes or the payload larger than
/// [`Limits::max_decoded_bytes`].
pub fn encode(
    game_version: &str,
    saved_at: i64,
    payload: &Value,
    limits: Limits,
) -> Result<Vec<u8>, CodecError> {
    let encode_error = |kind| CodecError::KsavEncode { kind };
    let Ok(game_version_len) = u16::try_from(game_version.len()) else {
        return Err(encode_error(EncodeErrorKind::GameVersionTooLong {
            len: game_version.len(),
        }));
    };
    let kbj = kbj::encode(payload, limits)?;
    let too_large = || {
        encode_error(EncodeErrorKind::PayloadTooLarge {
            size: kbj.len(),
            limit: limits.max_decoded_bytes,
        })
    };
    let payload_size = u32::try_from(kbj.len()).map_err(|_| too_large())?;
    if kbj.len() > limits.max_decoded_bytes {
        return Err(too_large());
    }
    let mut bytes = Vec::with_capacity(HEADER_SIZE + game_version.len() + kbj.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&game_version_len.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&saved_at.to_le_bytes());
    bytes.extend_from_slice(&payload_size.to_le_bytes());
    bytes.extend_from_slice(game_version.as_bytes());
    bytes.extend_from_slice(&kbj);
    let crc = crc32(&bytes[CHECKED_FROM..]);
    bytes[8..CHECKED_FROM].copy_from_slice(&crc.to_le_bytes());
    Ok(bytes)
}

/// Reads a save file's header from the whole file, `bytes`, and checks the
/// file as the module says, but for its payload's KBJ: for a list of saves
/// that shows when each was written, and by which version of the game.
///
/// # Errors
///
/// A [`CodecError::Ksav`] at the byte of the field that is wrong:
/// [`ErrorKind::NotKsav`], [`ErrorKind::UnexpectedEnd`],
/// [`ErrorKind::Version`], [`ErrorKind::PayloadTooLarge`],
/// [`ErrorKind::TrailingBytes`], [`ErrorKind::Damaged`] or
/// [`ErrorKind::BadGameVersion`].
pub fn header(bytes: &[u8], limits: Limits) -> Result<Header, CodecError> {
    let error = |offset, kind| CodecError::Ksav { offset, kind };
    let magic_len = bytes.len().min(MAGIC.len());
    if bytes[..magic_len] != MAGIC[..magic_len] {
        return Err(error(0, ErrorKind::NotKsav));
    }
    let Some(fixed) = bytes.get(..HEADER_SIZE) else {
        return Err(error(
            bytes.len(),
            ErrorKind::UnexpectedEnd {
                expected: "the header's end",
            },
        ));
    };
    let u16_at = |at: usize| u16::from_le_bytes([fixed[at], fixed[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]]);
    let version = u16_at(4);
    if version == 0 || version > VERSION {
        return Err(error(4, ErrorKind::Version { found: version }));
    }
    let game_version_len = usize::from(u16_at(6));
    let expected_crc = u32_at(8);
    let mut saved_at = [0; 8];
    saved_at.copy_from_slice(&fixed[12..20]);
    let saved_at = i64::from_le_bytes(saved_at);
    let payload_size = u32_at(20);

    let payload_offset = HEADER_SIZE + game_version_len;
    let Some(game_version) = bytes.get(HEADER_SIZE..payload_offset) else {
        return Err(error(
            bytes.len(),
            ErrorKind::UnexpectedEnd {
                expected: "the game version's end",
            },
        ));
    };
    if payload_size as usize > limits.max_decoded_bytes {
        return Err(error(
            20,
            ErrorKind::PayloadTooLarge {
                size: payload_size,
                limit: limits.max_decoded_bytes,
            },
        ));
    }
    let present = bytes.len() - payload_offset;
    let payload_size_usize = payload_size as usize;
    if present < payload_size_usize {
        return Err(error(
            bytes.len(),
            ErrorKind::UnexpectedEnd {
                expected: "the payload's end",
            },
        ));
    }
    if present > payload_size_usize {
        return Err(error(
            payload_offset + payload_size_usize,
            ErrorKind::TrailingBytes {
                extra: present - payload_size_usize,
            },
        ));
    }
    let found_crc = crc32(&bytes[CHECKED_FROM..]);
    if found_crc != expected_crc {
        return Err(error(
            8,
            ErrorKind::Damaged {
                expected: expected_crc,
                found: found_crc,
            },
        ));
    }
    let Ok(game_version) = std::str::from_utf8(game_version) else {
        return Err(error(HEADER_SIZE, ErrorKind::BadGameVersion));
    };
    Ok(Header {
        version,
        game_version: game_version.to_string(),
        saved_at,
        payload_size,
    })
}

/// Reads a save file: its [`header`], then its payload's KBJ, within
/// `limits`. The payload is the version's the header gives, which
/// [`read`] checks once it is of [`VERSION`].
///
/// # Errors
///
/// As [`header`], and [`ErrorKind::BadPayload`] at the byte in the file
/// where the payload's KBJ cannot be read.
pub fn parse(bytes: &[u8], limits: Limits) -> Result<(Header, Value), CodecError> {
    let header = header(bytes, limits)?;
    let start = header.payload_offset();
    let payload = kbj::decode(&bytes[start..], limits).map_err(|kbj_error| match kbj_error {
        CodecError::Kbj { offset, kind } => CodecError::Ksav {
            offset: start + offset,
            kind: ErrorKind::BadPayload { kind },
        },
        other => other,
    })?;
    Ok((header, payload))
}

/// Reads version [`VERSION`]'s payload from its value, and checks it as the
/// module says.
///
/// # Errors
///
/// [`CodecError::KsavData`], naming where, such as `entities[2].id`, if the
/// value is not a save's payload.
pub fn read(payload: &Value) -> Result<SaveData, CodecError> {
    let error = |at: String, kind| CodecError::KsavData { at, kind };
    let members = object(payload, "", &["next_id", "globals", "entities"])?;
    let next_id = member(members, "", "next_id")?;
    let next_id = read_id(next_id).ok_or_else(|| error("next_id".into(), DataErrorKind::BadId))?;
    let globals = typed_members(member(members, "", "globals")?, "globals")?;
    let entities = member(members, "", "entities")?.as_array().ok_or_else(|| {
        error(
            "entities".into(),
            DataErrorKind::WrongType {
                expected: "an array",
            },
        )
    })?;
    let mut ids = std::collections::BTreeSet::new();
    let mut saved = Vec::with_capacity(entities.len());
    for (index, entity) in entities.iter().enumerate() {
        let at = format!("entities[{index}]");
        let fields = object(entity, &at, &["id", "prefab", "components"])?;
        let id_at = format!("{at}.id");
        let id = read_id(member(fields, &at, "id")?)
            .ok_or_else(|| error(id_at.clone(), DataErrorKind::BadId))?;
        if !ids.insert(id) {
            return Err(error(id_at, DataErrorKind::DuplicateId { id }));
        }
        let prefab = match fields.iter().find(|(name, _)| name == "prefab") {
            Some((_, value)) => match value.as_str() {
                Some(name) if prefab::is_name(name) => Some(name.to_string()),
                Some(_) => return Err(error(format!("{at}.prefab"), DataErrorKind::BadPrefabName)),
                None => {
                    return Err(error(
                        format!("{at}.prefab"),
                        DataErrorKind::WrongType {
                            expected: "a string",
                        },
                    ));
                }
            },
            None => None,
        };
        let components = typed_members(
            member(fields, &at, "components")?,
            &format!("{at}.components"),
        )?;
        saved.push(SavedEntity {
            id,
            prefab,
            components,
        });
    }
    Ok(SaveData {
        next_id,
        globals,
        entities: saved,
    })
}

/// The members of `value`, an object at `at` that has only members named
/// in `known`.
fn object<'v>(
    value: &'v Value,
    at: &str,
    known: &[&str],
) -> Result<&'v [(String, Value)], CodecError> {
    let members = value.as_object().ok_or_else(|| CodecError::KsavData {
        at: at.to_string(),
        kind: DataErrorKind::WrongType {
            expected: "an object",
        },
    })?;
    if let Some((name, _)) = members
        .iter()
        .find(|(name, _)| !known.contains(&name.as_str()))
    {
        return Err(CodecError::KsavData {
            at: at.to_string(),
            kind: DataErrorKind::UnknownMember { name: name.clone() },
        });
    }
    Ok(members)
}

/// The member `name` of `members`, an object at `at`.
fn member<'v>(
    members: &'v [(String, Value)],
    at: &str,
    name: &'static str,
) -> Result<&'v Value, CodecError> {
    members
        .iter()
        .find(|(member, _)| member == name)
        .map(|(_, value)| value)
        .ok_or_else(|| CodecError::KsavData {
            at: at.to_string(),
            kind: DataErrorKind::MissingMember { name },
        })
}

/// `value`, an object at `at` of values by type name, as a list.
fn typed_members(value: &Value, at: &str) -> Result<Vec<(String, Value)>, CodecError> {
    let members = value.as_object().ok_or_else(|| CodecError::KsavData {
        at: at.to_string(),
        kind: DataErrorKind::WrongType {
            expected: "an object",
        },
    })?;
    if let Some((name, _)) = members
        .iter()
        .find(|(name, _)| !prefab::is_component_name(name))
    {
        return Err(CodecError::KsavData {
            at: at.to_string(),
            kind: DataErrorKind::BadTypeName { name: name.clone() },
        });
    }
    Ok(members.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> SaveData {
        let mut data = SaveData::new(u64::MAX);
        data.globals
            .push(("Score".to_string(), Value::Object(vec![])));
        let mut slime = SavedEntity::new(3);
        slime.prefab = Some("slime".to_string());
        slime.components.push(("Health".to_string(), Value::Int(4)));
        data.entities.push(slime);
        let mut far = SavedEntity::new(1 << 63);
        far.components.push(("Target".to_string(), Value::Null));
        data.entities.push(far);
        data
    }

    #[test]
    fn a_save_reads_back_as_it_was_written() {
        let data = data();
        let bytes = encode("0.4.0", -1, &data.to_value(), Limits::default()).unwrap();
        let (header, payload) = parse(&bytes, Limits::default()).unwrap();
        assert_eq!(header.version, VERSION);
        assert_eq!(header.game_version, "0.4.0");
        assert_eq!(header.saved_at, -1);
        assert_eq!(header.payload_offset(), 29);
        assert_eq!(header.payload_size as usize, bytes.len() - 29);
        assert_eq!(read(&payload).unwrap(), data);
        // The ids past an i64 are strings of their digits.
        let ids = payload.get("entities").and_then(Value::as_array).unwrap();
        assert_eq!(
            ids[1].get("id"),
            Some(&Value::String("9223372036854775808".into()))
        );
        assert_eq!(
            payload.get("next_id"),
            Some(&Value::String(u64::MAX.to_string()))
        );
    }

    #[test]
    fn ids_are_read_only_as_they_are_written() {
        assert_eq!(read_id(&Value::Int(0)), Some(0));
        assert_eq!(read_id(&Value::Int(-1)), None);
        assert_eq!(read_id(&Value::String("12".into())), None, "fits an i64");
        assert_eq!(
            read_id(&Value::String("09223372036854775808".into())),
            None,
            "a leading zero"
        );
        assert_eq!(read_id(&Value::String("18446744073709551616".into())), None);
        assert_eq!(read_id(&Value::Float(1.0)), None);
        for id in [0, 1, i64::MAX as u64, 1 << 63, u64::MAX] {
            assert_eq!(read_id(&id_value(id)), Some(id), "{id}");
        }
    }

    #[test]
    fn every_bit_after_the_crc_is_checked() {
        let bytes = encode("v", 7, &data().to_value(), Limits::default()).unwrap();
        for at in CHECKED_FROM..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[at] ^= 0x10;
            assert!(
                header(&damaged, Limits::default()).is_err(),
                "a bit changed at byte {at} was not seen"
            );
        }
    }

    #[test]
    fn what_cannot_be_read_back_is_not_written() {
        let long = "v".repeat(65_536);
        assert_eq!(
            encode(&long, 0, &Value::Null, Limits::default()),
            Err(CodecError::KsavEncode {
                kind: EncodeErrorKind::GameVersionTooLong { len: 65_536 }
            })
        );
        let limits = Limits {
            max_decoded_bytes: 4,
            ..Limits::default()
        };
        assert_eq!(
            encode("", 0, &Value::Null, limits),
            Err(CodecError::KsavEncode {
                kind: EncodeErrorKind::PayloadTooLarge { size: 5, limit: 4 }
            })
        );
        assert!(encode("", 0, &Value::Null, Limits::default()).is_ok());
    }
}
