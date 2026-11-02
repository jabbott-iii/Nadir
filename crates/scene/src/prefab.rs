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

//! Prefabs: named sets of component values, which may extend another
//! prefab, and the [`PrefabLibrary`] that loads them and spawns entities of
//! them (`docs/subsystems/scene.md`, "Prefabs"; ADR-0038).
//!
//! A prefab is a cooked file, `data/prefabs/<name>.kbj`, which
//! [`nadir_codec::prefab`] reads: its name, the prefab it extends, and its
//! components' values by type name. Spawning one applies, for each
//! component, its base's value, then its own, then the overrides given,
//! onto one value, which [`Registration::insert_layers`] inserts. Every
//! component is made before the entity is spawned, so a spawn that fails
//! leaves no entity.

use std::any::Any;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use nadir_assets::{Asset, AssetError, AssetServer, DecodeCtx, FinalizeCtx, Scope};
use nadir_codec::json::Value;
use nadir_codec::level::LevelFile;
pub use nadir_codec::prefab::PrefabFile;
use nadir_core::IVec2;
use nadir_ecs::{Commands, Entity};

use crate::{Layer, ReflectContext, ReflectError, Registration, TypeRegistry};

/// The most prefabs one may extend, through one another: a chain of 16
/// `extends` at most, so that a cycle or a runaway chain is an error, not a
/// hang.
pub const MAX_EXTENDS: usize = 16;

/// A prefab's cooked file, as an asset: `data/prefabs/<name>.kbj`, read
/// with [`nadir_codec::prefab::parse`]. [`PrefabLibrary::load`] loads it.
#[derive(Clone, Debug, PartialEq)]
pub struct PrefabAsset {
    file: PrefabFile,
}

impl PrefabAsset {
    /// The prefab file.
    pub fn file(&self) -> &PrefabFile {
        &self.file
    }
}

impl Asset for PrefabAsset {
    type Decoded = PrefabFile;
    const EXTENSIONS: &'static [&'static str] = &["kbj"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<PrefabFile, AssetError> {
        nadir_codec::prefab::parse(bytes, ctx.limits()).map_err(AssetError::Decode)
    }

    fn finalize(file: PrefabFile, _ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(Self { file })
    }
}

/// A prefab of a [`PrefabLibrary`]: its file, and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefab {
    source: String,
    file: PrefabFile,
}

impl Prefab {
    /// Its name.
    pub fn name(&self) -> &str {
        &self.file.name
    }

    /// Where it came from, for messages: `data/prefabs/slime.kbj`.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The prefab it extends, if any.
    pub fn extends(&self) -> Option<&str> {
        self.file.extends.as_deref()
    }

    /// Its own components' values, by type name, in its file's order.
    pub fn components(&self) -> &[(String, Value)] {
        &self.file.components
    }
}

/// The prefabs a game has loaded, by name, in order of their names
/// (ADR-0025): a world resource, which the facade adds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrefabLibrary {
    prefabs: BTreeMap<String, Prefab>,
}

/// One component a prefab spawns with, and the type that makes it.
type Made<'r> = (&'r Registration, Box<dyn Any>);

impl PrefabLibrary {
    /// No prefabs.
    pub fn new() -> Self {
        Self::default()
    }

    /// The cooked file of the prefab `name`: `data/prefabs/<name>.kbj`.
    pub fn path(name: &str) -> String {
        format!("data/prefabs/{name}.kbj")
    }

    /// The prefab `name`, if it is loaded.
    pub fn get(&self, name: &str) -> Option<&Prefab> {
        self.prefabs.get(name)
    }

    /// Every prefab loaded, in order of their names.
    pub fn iter(&self) -> impl Iterator<Item = &Prefab> {
        self.prefabs.values()
    }

    /// Adds `file`, read from `source`, as its prefab, replacing one of its
    /// name: for prefabs that do not come from the asset server, such as a
    /// test's or a tool's.
    pub fn insert(&mut self, source: impl Into<String>, file: PrefabFile) {
        let source = source.into();
        self.prefabs
            .insert(file.name.clone(), Prefab { source, file });
    }

    /// Loads the prefab `name`, and each prefab it extends, from
    /// [`PrefabLibrary::path`], in `scope`, unless they are loaded; each is
    /// read before this returns ([`AssetServer::load_now_in`]). Then checks
    /// the chain's components against `registry`, as
    /// [`PrefabLibrary::check`] does (ADR-0038).
    ///
    /// # Errors
    ///
    /// [`PrefabError::Load`] if a file cannot be loaded, a base's among
    /// them; [`PrefabError::NameMismatch`] if a file's prefab is not named
    /// after it; [`PrefabError::Cycle`] if the chain of bases comes back to
    /// a prefab in it; [`PrefabError::TooLong`] if it is longer than
    /// [`MAX_EXTENDS`]; and [`PrefabError::UnknownComponent`] if a prefab
    /// of the chain has a component `registry` has not got. The prefabs
    /// read before the error stay loaded.
    pub fn load(
        &mut self,
        server: &mut AssetServer,
        registry: &TypeRegistry,
        name: &str,
        scope: Scope,
    ) -> Result<(), PrefabError> {
        let mut chain: Vec<String> = Vec::new();
        let mut current = name.to_string();
        loop {
            check_chain(&chain, &current)?;
            if !self.prefabs.contains_key(&current) {
                let path = Self::path(&current);
                let loaded = server
                    .load_now_in::<PrefabAsset>(&path, scope, &mut FinalizeCtx::new())
                    .map_err(|error| PrefabError::Load {
                        path: path.clone(),
                        error,
                    })?;
                let file = server
                    .get(loaded)
                    .map(|asset| asset.file.clone())
                    .ok_or_else(|| PrefabError::Load {
                        path: path.clone(),
                        error: AssetError::NotFound,
                    })?;
                if file.name != current {
                    return Err(PrefabError::NameMismatch {
                        path,
                        name: file.name,
                    });
                }
                self.insert(path, file);
            }
            let base = self.prefabs[&current].file.extends.clone();
            chain.push(current);
            match base {
                Some(base) => current = base,
                None => return self.check(name, registry),
            }
        }
    }

    /// Loads, as [`PrefabLibrary::load`] does, every prefab the entities of
    /// `level` spawn.
    ///
    /// # Errors
    ///
    /// As [`PrefabLibrary::load`], for the first prefab that fails.
    pub fn load_level(
        &mut self,
        server: &mut AssetServer,
        registry: &TypeRegistry,
        level: &LevelFile,
        scope: Scope,
    ) -> Result<(), PrefabError> {
        for entity in &level.entities {
            self.load(server, registry, &entity.prefab, scope)?;
        }
        Ok(())
    }

    /// The prefab `name` and the prefabs it extends, the furthest base
    /// first and `name` last: the order their values apply in.
    ///
    /// # Errors
    ///
    /// [`PrefabError::NotLoaded`] if `name` is not loaded,
    /// [`PrefabError::MissingBase`] if a base is not,
    /// [`PrefabError::Cycle`] and [`PrefabError::TooLong`] as
    /// [`PrefabLibrary::load`] gives them.
    pub fn chain(&self, name: &str) -> Result<Vec<&Prefab>, PrefabError> {
        let mut chain: Vec<&Prefab> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut current = name.to_string();
        loop {
            check_chain(&names, &current)?;
            let Some(prefab) = self.prefabs.get(&current) else {
                return Err(match chain.last() {
                    Some(derived) => PrefabError::MissingBase {
                        prefab: derived.name().to_string(),
                        base: current,
                    },
                    None => PrefabError::NotLoaded { name: current },
                });
            };
            chain.push(prefab);
            match prefab.extends() {
                Some(base) => {
                    names.push(current);
                    current = base.to_string();
                }
                None => break,
            }
        }
        chain.reverse();
        Ok(chain)
    }

    /// Checks that every component of the prefab `name`, and of its bases,
    /// is a type of `registry`.
    ///
    /// # Errors
    ///
    /// As [`PrefabLibrary::chain`], and [`PrefabError::UnknownComponent`]
    /// for the first component `registry` has not got.
    pub fn check(&self, name: &str, registry: &TypeRegistry) -> Result<(), PrefabError> {
        for prefab in self.chain(name)? {
            for (component, _) in prefab.components() {
                if registry.get(component).is_none() {
                    return Err(PrefabError::UnknownComponent {
                        source: prefab.source.clone(),
                        prefab: prefab.name().to_string(),
                        component: component.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Spawns an entity of the prefab `name` through `commands`, with
    /// `overrides` applied over its values, and returns it.
    ///
    /// Each component's value is the furthest base's, with each derived
    /// prefab's applied onto it, then the override's: a field none of them
    /// names keeps the type's default, and a component only one of them has
    /// is made from that one alone. The overrides are read under `cx`'s
    /// source, a level's file say, and `cx` gives what the values need: the
    /// asset server and the scope to load handles in, and the facade's
    /// resources ([`ReflectContext::loading`],
    /// [`ReflectContext::with_resource`]). The entity is alive at once, and
    /// its components are added when the commands are applied.
    ///
    /// # Errors
    ///
    /// As [`PrefabLibrary::chain`]; [`PrefabError::UnknownComponent`] for
    /// a component `registry` has not got, naming the file that gave it and
    /// its prefab; and [`PrefabError::Reflect`] for a value that does not
    /// fit its type: a prefab's at `components.<Type>.<field>` of its file,
    /// an override's at `overrides.<Type>.<field>`, under `cx`'s source and
    /// path. No entity is spawned then.
    pub fn spawn(
        &self,
        name: &str,
        overrides: &[(String, Value)],
        registry: &TypeRegistry,
        commands: &mut Commands<'_>,
        cx: &mut ReflectContext<'_>,
    ) -> Result<Entity, PrefabError> {
        let made = self.make(name, overrides, registry, cx)?;
        Ok(insert(made, commands))
    }

    /// Spawns, as [`PrefabLibrary::spawn`] does, each entity of `level`,
    /// with its overrides, and returns each with its position in the level
    /// in pixels, for its transform. Every entity is made before any is
    /// spawned, so a level that fails spawns none.
    ///
    /// Load the level's prefabs first ([`PrefabLibrary::load_level`]), and
    /// give `cx` the level's file as its source and [`Scope::Level`] to load
    /// its handles in.
    ///
    /// # Errors
    ///
    /// As [`PrefabLibrary::spawn`], for the first entity that fails, whose
    /// overrides' errors are at `entities[<index>].overrides.<Type>` of the
    /// level's file.
    pub fn spawn_level(
        &self,
        level: &LevelFile,
        registry: &TypeRegistry,
        commands: &mut Commands<'_>,
        cx: &mut ReflectContext<'_>,
    ) -> Result<Vec<(Entity, IVec2)>, PrefabError> {
        let mut made = Vec::with_capacity(level.entities.len());
        cx.field("entities", |cx| {
            for (index, entity) in level.entities.iter().enumerate() {
                let components = cx.index(index, |cx| {
                    self.make(&entity.prefab, &entity.overrides, registry, cx)
                })?;
                let [x, y] = entity.position;
                made.push((components, IVec2::new(x, y)));
            }
            Ok::<(), PrefabError>(())
        })?;
        Ok(made
            .into_iter()
            .map(|(components, position)| (insert(components, commands), position))
            .collect())
    }

    /// The components of an entity of the prefab `name` with `overrides`,
    /// made, as [`PrefabLibrary::spawn`] says.
    fn make<'r>(
        &self,
        name: &str,
        overrides: &[(String, Value)],
        registry: &'r TypeRegistry,
        cx: &mut ReflectContext<'_>,
    ) -> Result<Vec<Made<'r>>, PrefabError> {
        let chain = self.chain(name)?;
        let override_source = cx.source().to_string();
        let outer = cx.at();
        // Each component, in the order the chain first names it, then the
        // overrides, with its type and its layers in the order they apply.
        // Each name is looked up as it comes, so the list holds registered
        // types only.
        let mut components: Components<'_, 'r> = Vec::new();
        for prefab in &chain {
            for (component, value) in prefab.components() {
                let Some(registration) = registry.get(component) else {
                    return Err(PrefabError::UnknownComponent {
                        source: prefab.source.clone(),
                        prefab: prefab.name().to_string(),
                        component: component.clone(),
                    });
                };
                let layer = Layer {
                    source: prefab.source(),
                    at: format!("components.{component}"),
                    value,
                };
                add_layer(&mut components, registration, layer);
            }
        }
        for (component, value) in overrides {
            let Some(registration) = registry.get(component) else {
                return Err(PrefabError::UnknownComponent {
                    source: override_source.clone(),
                    prefab: name.to_string(),
                    component: component.clone(),
                });
            };
            let at = if outer.is_empty() {
                format!("overrides.{component}")
            } else {
                format!("{outer}.overrides.{component}")
            };
            let source = override_source.as_str();
            add_layer(&mut components, registration, Layer { source, at, value });
        }
        let mut made = Vec::with_capacity(components.len());
        for (registration, layers) in &components {
            made.push((*registration, registration.make_layers(layers, cx)?));
        }
        Ok(made)
    }
}

/// A spawn's components: each type, with its layers.
type Components<'v, 'r> = Vec<(&'r Registration, Vec<Layer<'v>>)>;

/// Adds `layer` to the layers of `registration`'s type in `components`, or
/// the type, with it, after the others.
fn add_layer<'v, 'r>(
    components: &mut Components<'v, 'r>,
    registration: &'r Registration,
    layer: Layer<'v>,
) {
    let known = components
        .iter_mut()
        .find(|(known, _)| std::ptr::eq(*known, registration));
    match known {
        Some((_, layers)) => layers.push(layer),
        None => components.push((registration, vec![layer])),
    }
}

/// Spawns an entity with the components `made`, through `commands`.
fn insert(made: Vec<Made<'_>>, commands: &mut Commands<'_>) -> Entity {
    let entity = commands.spawn().id();
    for (registration, value) in made {
        registration.insert_made(commands, entity, value);
    }
    entity
}

/// Fails if `next`, after the prefabs of `chain`, would make a cycle, or a
/// chain of more than [`MAX_EXTENDS`] bases.
fn check_chain(chain: &[String], next: &str) -> Result<(), PrefabError> {
    let mut names = chain.to_vec();
    names.push(next.to_string());
    if chain.iter().any(|known| known == next) {
        return Err(PrefabError::Cycle { chain: names });
    }
    if chain.len() > MAX_EXTENDS {
        return Err(PrefabError::TooLong { chain: names });
    }
    Ok(())
}

/// Why a prefab could not be loaded or spawned.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum PrefabError {
    /// A prefab's file could not be loaded.
    Load {
        /// The file: `data/prefabs/slime.kbj`.
        path: String,
        /// Why.
        error: AssetError,
    },
    /// A prefab's file names it otherwise than its file is named.
    NameMismatch {
        /// The file.
        path: String,
        /// The name it gives.
        name: String,
    },
    /// The prefab is not loaded.
    NotLoaded {
        /// The prefab.
        name: String,
    },
    /// A prefab extends one that is not loaded.
    MissingBase {
        /// The prefab.
        prefab: String,
        /// The prefab it extends.
        base: String,
    },
    /// A prefab's chain of bases comes back to a prefab in it.
    Cycle {
        /// The chain, from the prefab asked for, ending in the one it came
        /// back to.
        chain: Vec<String>,
    },
    /// A prefab's chain of bases is longer than [`MAX_EXTENDS`].
    TooLong {
        /// The chain, from the prefab asked for, as far as it was followed.
        chain: Vec<String>,
    },
    /// A prefab, or a level's override, names a component the registry has
    /// not got: it is not reflected, or not registered.
    UnknownComponent {
        /// The file that names it.
        source: String,
        /// The prefab.
        prefab: String,
        /// The component's name.
        component: String,
    },
    /// A value does not fit its component's type.
    Reflect(ReflectError),
}

impl fmt::Display for PrefabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load { path, error } => write!(f, "{path}: {error}"),
            Self::NameMismatch { path, name } => write!(
                f,
                "{path}: the prefab is named `{name}`, not as its file is"
            ),
            Self::NotLoaded { name } => write!(f, "the prefab `{name}` is not loaded"),
            Self::MissingBase { prefab, base } => write!(
                f,
                "the prefab `{prefab}` extends `{base}`, which is not loaded"
            ),
            Self::Cycle { chain } => write!(
                f,
                "{}: a prefab cannot extend itself, through others or not",
                chain.join(" -> ")
            ),
            Self::TooLong { chain } => write!(
                f,
                "{}: a prefab extends at most {MAX_EXTENDS} others, one through another",
                chain.join(" -> ")
            ),
            Self::UnknownComponent {
                source,
                prefab,
                component,
            } => write!(
                f,
                "{source}: the prefab `{prefab}` has the component `{component}`, which is not a reflected type the game registered"
            ),
            Self::Reflect(error) => write!(f, "{error}"),
        }
    }
}

impl Error for PrefabError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load { error, .. } => Some(error),
            Self::Reflect(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ReflectError> for PrefabError {
    fn from(error: ReflectError) -> Self {
        Self::Reflect(error)
    }
}
