//! The settings beside the images: `*.sprite.jsonc` sidecars, and folders'
//! `atlas.jsonc`. Both are JSONC objects whose keys are checked: a key the
//! cook does not know is an error, not ignored.

use nadir_assets::AssetPath;
use nadir_codec::Limits;
use nadir_codec::json::{self, Value};

/// How an image is cut, from its sidecar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Layout {
    /// One frame: the whole image.
    Whole,
    /// A sheet of frames of this width and height, row by row.
    Frames(u32, u32),
    /// A tile set of tiles of this width and height: its own page.
    Tiles(u32, u32),
}

/// An image's sidecar: what its absence means too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sprite {
    pub(crate) layout: Layout,
    /// Whether each frame is trimmed to the pixels it shows.
    pub(crate) trim: bool,
    /// The pivot, in a frame's untrimmed pixels.
    pub(crate) pivot: Option<(f64, f64)>,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            layout: Layout::Whole,
            trim: true,
            pivot: None,
        }
    }
}

/// The object `bytes` holds, as JSONC.
fn object(bytes: &[u8]) -> Result<Vec<(String, Value)>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8".to_string())?;
    match json::parse(text, Limits::default()).map_err(|error| error.to_string())? {
        Value::Object(members) => Ok(members),
        _ => Err("it is not an object".to_string()),
    }
}

/// Two whole numbers of at least 1, such as `[16, 16]`.
fn size(key: &str, value: &Value) -> Result<(u32, u32), String> {
    let whole = |value: &Value| {
        value
            .as_i64()
            .and_then(|number| u32::try_from(number).ok())
            .filter(|&number| number >= 1)
    };
    match value.as_array() {
        Some([w, h]) => match (whole(w), whole(h)) {
            (Some(w), Some(h)) => Ok((w, h)),
            _ => Err(format!("`{key}` must be two whole numbers of 1 or more")),
        },
        _ => Err(format!("`{key}` must be two whole numbers of 1 or more")),
    }
}

/// Reads a sidecar.
///
/// ```jsonc
/// { "frames": [12, 20], "pivot": [6, 20], "trim": false }   // a sheet
/// { "tiles": [16, 16] }                                       // a tile set
/// ```
///
/// # Errors
///
/// What is wrong with it, for [`CookError::Config`](crate::CookError).
pub(crate) fn sprite(bytes: &[u8]) -> Result<Sprite, String> {
    let mut sprite = Sprite::default();
    let mut tiles = None;
    let mut frames = None;
    let mut set_trim = false;
    for (key, value) in object(bytes)? {
        match key.as_str() {
            "frames" => frames = Some(size("frames", &value)?),
            "tiles" => tiles = Some(size("tiles", &value)?),
            "trim" => {
                sprite.trim = value.as_bool().ok_or("`trim` must be true or false")?;
                set_trim = true;
            }
            "pivot" => {
                let number = |value: &Value| value.as_f64().filter(|n| n.is_finite());
                sprite.pivot = match value.as_array() {
                    Some([x, y]) => number(x).zip(number(y)),
                    _ => None,
                };
                if sprite.pivot.is_none() {
                    return Err("`pivot` must be two numbers".to_string());
                }
            }
            other => {
                return Err(format!(
                    "`{other}` is not a key of a sprite: `frames`, `tiles`, `trim` or `pivot`"
                ));
            }
        }
    }
    sprite.layout = match (frames, tiles) {
        (Some(_), Some(_)) => return Err("`frames` and `tiles` cannot both be given".to_string()),
        (Some((w, h)), None) => Layout::Frames(w, h),
        (None, Some((w, h))) => {
            if set_trim || sprite.pivot.is_some() {
                return Err(
                    "a tile set is its own page, as it is: it takes no `trim` or `pivot`"
                        .to_string(),
                );
            }
            sprite.trim = false;
            Layout::Tiles(w, h)
        }
        (None, None) => Layout::Whole,
    };
    Ok(sprite)
}

/// Reads a folder's `atlas.jsonc`: the group its images join, if it names
/// one.
///
/// ```jsonc
/// { "group": "characters" }
/// ```
///
/// # Errors
///
/// What is wrong with it, for [`CookError::Config`](crate::CookError). A
/// group's name must be lowercase, and make asset paths
/// `atlas/<group>.kbj`.
pub(crate) fn folder(bytes: &[u8]) -> Result<Option<String>, String> {
    let mut group = None;
    for (key, value) in object(bytes)? {
        match key.as_str() {
            "group" => {
                let name = value.as_str().ok_or("`group` must be a string")?;
                group = Some(group_name(name)?);
            }
            other => {
                return Err(format!(
                    "`{other}` is not a key of a folder's settings: `group`"
                ));
            }
        }
    }
    Ok(group)
}

/// Checks a group's name: lowercase, and one that makes asset paths.
pub(crate) fn group_name(name: &str) -> Result<String, String> {
    if name != name.to_lowercase() {
        return Err(format!("the group `{name}` must be lowercase"));
    }
    // The name itself, so that it has no empty part, then the files it
    // names.
    for path in [name.to_string(), format!("atlas/{name}.kbj")] {
        match AssetPath::new(&path) {
            Ok(checked) if checked.as_str() == path => {}
            Ok(_) => return Err(format!("the group `{name}` must be lowercase")),
            Err(error) => return Err(format!("the group `{name}` cannot name a file: {error}")),
        }
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sidecar_says_how_an_image_is_cut() {
        assert_eq!(sprite(b"{}"), Ok(Sprite::default()));
        assert_eq!(
            sprite(b"// a walk\n{ \"frames\": [12, 20], \"pivot\": [6, 19.5], }"),
            Ok(Sprite {
                layout: Layout::Frames(12, 20),
                trim: true,
                pivot: Some((6.0, 19.5)),
            })
        );
        assert_eq!(
            sprite(br#"{ "trim": false }"#),
            Ok(Sprite {
                trim: false,
                ..Sprite::default()
            })
        );
        assert_eq!(
            sprite(br#"{ "tiles": [16, 8] }"#),
            Ok(Sprite {
                layout: Layout::Tiles(16, 8),
                trim: false,
                pivot: None,
            })
        );
    }

    #[test]
    fn a_wrong_sidecar_says_what_is_wrong() {
        for (text, error) in [
            (&b"[]"[..], "it is not an object"),
            (b"\xff", "it is not UTF-8"),
            (
                br#"{ "frame": [1, 1] }"#,
                "`frame` is not a key of a sprite: `frames`, `tiles`, `trim` or `pivot`",
            ),
            (
                br#"{ "frames": [0, 20] }"#,
                "`frames` must be two whole numbers of 1 or more",
            ),
            (
                br#"{ "frames": [12] }"#,
                "`frames` must be two whole numbers of 1 or more",
            ),
            (
                br#"{ "frames": [12, 2.5] }"#,
                "`frames` must be two whole numbers of 1 or more",
            ),
            (
                br#"{ "tiles": [-16, 16] }"#,
                "`tiles` must be two whole numbers of 1 or more",
            ),
            (
                br#"{ "tiles": [16, 4294967296] }"#,
                "`tiles` must be two whole numbers of 1 or more",
            ),
            (br#"{ "trim": 0 }"#, "`trim` must be true or false"),
            (br#"{ "pivot": [1, "2"] }"#, "`pivot` must be two numbers"),
            (br#"{ "pivot": [1, 2, 3] }"#, "`pivot` must be two numbers"),
            (
                br#"{ "frames": [1, 1], "tiles": [1, 1] }"#,
                "`frames` and `tiles` cannot both be given",
            ),
            (
                br#"{ "tiles": [8, 8], "trim": true }"#,
                "a tile set is its own page, as it is: it takes no `trim` or `pivot`",
            ),
            (
                br#"{ "tiles": [8, 8], "pivot": [0, 0] }"#,
                "a tile set is its own page, as it is: it takes no `trim` or `pivot`",
            ),
        ] {
            assert_eq!(
                sprite(text),
                Err(error.to_string()),
                "{}",
                String::from_utf8_lossy(text)
            );
        }
        assert!(sprite(b"{ \"trim\": tru }").is_err());
    }

    #[test]
    fn a_folder_names_its_group() {
        assert_eq!(folder(b"{}"), Ok(None));
        assert_eq!(
            folder(br#"{ "group": "ui/menus" }"#),
            Ok(Some("ui/menus".to_string()))
        );
        for (text, error) in [
            (
                &br#"{ "group": "Sprites" }"#[..],
                "the group `Sprites` must be lowercase",
            ),
            (
                "{ \"group\": \"Été\" }".as_bytes(),
                "the group `Été` must be lowercase",
            ),
            (br#"{ "group": 1 }"#, "`group` must be a string"),
            (
                br#"{ "pages": 2 }"#,
                "`pages` is not a key of a folder's settings: `group`",
            ),
            (
                br#"{ "group": "" }"#,
                "the group `` cannot name a file: not a valid asset path: it is empty",
            ),
            (
                br#"{ "group": "ui/" }"#,
                "the group `ui/` cannot name a file: not a valid asset path: it is absolute, or has an empty part",
            ),
            (
                br#"{ "group": "../up" }"#,
                "the group `../up` cannot name a file: not a valid asset path: it has a `.` or `..` part",
            ),
            (
                br#"{ "group": "con" }"#,
                "the group `con` cannot name a file: not a valid asset path: a part is a Windows device name",
            ),
        ] {
            assert_eq!(
                folder(text),
                Err(error.to_string()),
                "{}",
                String::from_utf8_lossy(text)
            );
        }
    }
}
