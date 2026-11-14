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

//! The inspector: the world's entities, filtered, and the components of the
//! one selected, each field edited through `Reflect`; a component added or
//! removed by its name, the entity despawned, or saved as a prefab
//! (`debug-tools.md`, "Tools"; roadmap M7.4, ADR-0062).

use std::fmt::{self, Write as _};

use nadir_codec::json::{self, Value};
use nadir_codec::{Limits, prefab};
use nadir_core::log::warn;
use nadir_core::{Color, Key};
use nadir_ecs::{Entity, World};
use nadir_scene::{Reflect, ReflectContext, ReflectKind, Registration, TypeRegistry};

use crate::Ui;
use crate::id::Id;

/// The entities a page of the list shows.
const PAGE: usize = 16;

/// How much a drag changes a number, for each UI unit the pointer moves.
const DRAG_PER_UNIT: f32 = 0.1;

/// The longest name a saved prefab may have, in bytes.
const MAX_PREFAB_NAME: usize = 64;

/// What the inspector asks of its caller, this frame.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InspectorResponse {
    /// "Save as prefab" was clicked, on an entity alive, with a name a
    /// prefab may have: the caller writes [`Inspector::prefab`] into
    /// `<name>.jsonc`, the name [`Inspector::prefab_name`].
    pub save_prefab: bool,
}

/// A text field's edit, kept from frame to frame until it is applied, on
/// Enter or a press elsewhere, or dropped, on Escape or when the field is
/// not drawn in a frame: the window closed or collapsed, say, which takes
/// the keyboard from it.
struct TextEdit {
    key: Id,
    text: String,
    /// The UI's frame its field was last drawn in.
    frame: u64,
}

/// The inspector's window: a list of the world's entities, a page at a
/// time, filtered by their numbers and the names of their components, and
/// the components of the one clicked, which [`Inspector::show`] edits
/// through their [`Reflect`] impls. It finds the component types in the
/// world's [`TypeRegistry`], a resource.
///
/// - **The list:** each entity as its slot and generation, `3v1`, then the
///   names of its registered components. The filter keeps the entities
///   that match each of its words, ignoring case: a word matches an
///   entity's number, or the name of a component it has.
/// - **Editing:** a struct's fields are rows, a struct inside one a tree
///   node; a list's elements and an object's members (a value reflected
///   whole, such as a sprite) are rows under a node; a boolean is a
///   checkbox; a float a drag; a whole number, a string or an asset's path a
///   text field, applied on Enter or a press elsewhere, dropped on Escape;
///   a `Color` a color edit, as is a string that writes a color among the
///   members of a value reflected whole; an enum's variant a combo; a pair
///   of numbers a row of two. A change is applied to the component at once, through
///   [`Reflect::apply_value`]; one it refuses is not set, and the note
///   says why, as the log does in full.
/// - **Changes of structure** go through the world's commands, and so show
///   after the next tick or frame: "Despawn", "Add" (a component of a type
///   it lacks, made from the JSONC typed beside it, `{}` at first: `0` for
///   a number's newtype, `[0, 0]` for a position's, `"Down"` for an enum;
///   one the type refuses is not added, and the note says why), and each
///   component's "Remove".
/// - **Save as prefab:** with a name the prefab reader takes, lowercase
///   letters, digits and `_`, it asks the caller to write
///   [`Inspector::prefab`].
///
/// A frame with no component open allocates nothing once its buffers have
/// grown; an open one builds its values ([`Reflect::to_value`]) each frame.
pub struct Inspector {
    filter: String,
    page: usize,
    selected: Option<Entity>,
    /// Every entity alive, then those the filter keeps: made each frame.
    entities: Vec<Entity>,
    /// The types the selected entity lacks, for the combo that adds one.
    lacking: Vec<&'static str>,
    /// Which of them the combo shows.
    add: usize,
    /// The data "Add" makes it from, as JSONC.
    add_data: String,
    prefab_name: String,
    note: String,
    edit: Option<TextEdit>,
    /// A value's text, for a field not being edited; an entity's number,
    /// for the filter.
    scratch: String,
}

impl Default for Inspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector {
    /// An inspector with nothing selected and no filter.
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            page: 0,
            selected: None,
            entities: Vec::new(),
            lacking: Vec::new(),
            add: 0,
            add_data: String::from("{}"),
            prefab_name: String::new(),
            note: String::new(),
            edit: None,
            scratch: String::new(),
        }
    }

    /// The entity whose components it shows, if one was clicked; it may
    /// have been despawned since.
    pub fn selected(&self) -> Option<Entity> {
        self.selected
    }

    /// Shows `entity`'s components, or none. An edit not yet applied is
    /// dropped.
    pub fn select(&mut self, entity: Option<Entity>) {
        if entity != self.selected {
            self.edit = None;
        }
        self.selected = entity;
    }

    /// The name typed for a prefab, which "Save as prefab" saves the
    /// selected entity as.
    pub fn prefab_name(&self) -> &str {
        &self.prefab_name
    }

    /// Shows `note` under the selected entity's buttons, until the next:
    /// where a prefab was written, say, or why not.
    pub fn set_note(&mut self, note: impl fmt::Display) {
        set_note(&mut self.note, note);
    }

    /// Fills a window with the entity list and the selected entity's
    /// components, from the world's [`TypeRegistry`]. `cx` reads and
    /// writes the values: one that can write and read asset handles, and
    /// that has the resources the game's components need, such as the
    /// facade's `loading_context`. Its errors and warnings name the
    /// component and the field, under `cx`'s source.
    ///
    /// # Panics
    ///
    /// If a registered type is not registered with `world`, or its
    /// components or the registry are borrowed mutably elsewhere.
    pub fn show(
        &mut self,
        world: &World,
        ui: &mut Ui<'_>,
        cx: &mut ReflectContext<'_>,
    ) -> InspectorResponse {
        let mut response = InspectorResponse::default();
        let frame = ui.frame_number();
        // An edit whose field was not drawn in the frame before has lost the
        // keyboard: the window was closed or collapsed meanwhile.
        if self
            .edit
            .as_ref()
            .is_some_and(|edit| edit.frame + 1 < frame)
        {
            self.edit = None;
        }
        if !world.contains_resource::<TypeRegistry>() {
            ui.label("The world has no type registry.");
            return response;
        }
        let registry = world.resource::<TypeRegistry>();
        self.list(world, &registry, ui);
        ui.separator();
        self.entity(world, &registry, ui, cx, &mut response);
        // A field not drawn this frame has let go of the keyboard.
        if self.edit.as_ref().is_some_and(|edit| edit.frame != frame) {
            self.edit = None;
        }
        response
    }

    /// The selected entity as the prefab [`Inspector::prefab_name`], as
    /// JSONC text: a comment line, then `{"name": ..., "components":
    /// {...}}`, each component of a type in the world's [`TypeRegistry`]
    /// that it has, in the registry's order, as [`Reflect::to_value`]
    /// writes it with `cx`, but those whose names `leave_out` lists. `None`
    /// if no entity alive is selected, or the world has no registry.
    ///
    /// `cx` writes the values: one that writes asset handles as their
    /// paths, and has the resources the game's components need, such as
    /// the facade's `reflect_context`.
    pub fn prefab(
        &self,
        world: &World,
        leave_out: &[&str],
        cx: &mut ReflectContext<'_>,
    ) -> Option<String> {
        let entity = self.selected.filter(|&entity| world.is_alive(entity))?;
        if !world.contains_resource::<TypeRegistry>() {
            return None;
        }
        let registry = world.resource::<TypeRegistry>();
        let mut components = Vec::new();
        for registration in registry.iter() {
            let name = registration.name();
            if leave_out.contains(&name) {
                continue;
            }
            registration.inspect(world, entity, &mut |component| {
                let value = cx.key(name, |cx| component.to_value(cx));
                components.push((name.to_string(), value));
            });
        }
        let prefab = Value::Object(vec![
            ("name".to_string(), Value::String(self.prefab_name.clone())),
            ("components".to_string(), Value::Object(components)),
        ]);
        Some(format!(
            "// The prefab `{}`, saved by the inspector from entity {}.\n{}\n",
            self.prefab_name,
            Number(entity),
            json::to_string_pretty(&prefab)
        ))
    }

    /// The filter, the count, a page of entities and the page's buttons.
    fn list(&mut self, world: &World, registry: &TypeRegistry, ui: &mut Ui<'_>) {
        if ui.text_input("Filter", &mut self.filter).changed {
            self.page = 0;
        }
        world.entities_into(&mut self.entities);
        let alive = self.entities.len();
        if self.filter.trim().is_empty() {
            ui.label(format_args!("{alive} entities"));
        } else {
            let (filter, scratch) = (&self.filter, &mut self.scratch);
            self.entities
                .retain(|&entity| matches(world, registry, entity, filter, scratch));
            ui.label(format_args!("{} of {alive} entities", self.entities.len()));
        }
        let shown = self.entities.len();
        let pages = shown.div_ceil(PAGE).max(1);
        self.page = self.page.min(pages - 1);
        let start = self.page * PAGE;
        let end = (start + PAGE).min(shown);
        let mut clicked = None;
        for &entity in &self.entities[start..end] {
            let row = Row {
                world,
                registry,
                entity,
                selected: self.selected == Some(entity),
            };
            if ui.button(format_args!("{row}##{}", Number(entity))).clicked {
                clicked = Some(entity);
            }
        }
        if pages > 1 {
            let page = &mut self.page;
            ui.horizontal(|ui| {
                if ui.button("<##previous").clicked {
                    *page = page.saturating_sub(1);
                }
                if ui.button(">##next").clicked {
                    *page = (*page + 1).min(pages - 1);
                }
                ui.label(format_args!("{}-{end} of {shown}", start + 1));
            });
        }
        if clicked.is_some() {
            self.select(clicked);
        }
    }

    /// The selected entity: its buttons, the note, and its components.
    fn entity(
        &mut self,
        world: &World,
        registry: &TypeRegistry,
        ui: &mut Ui<'_>,
        cx: &mut ReflectContext<'_>,
        response: &mut InspectorResponse,
    ) {
        let Some(entity) = self.selected else {
            ui.label("Click an entity to inspect it.");
            return;
        };
        if !world.is_alive(entity) {
            ui.label(format_args!(
                "Entity {} is gone: click another.",
                Number(entity)
            ));
            return;
        }
        ui.horizontal(|ui| {
            ui.label(format_args!("Entity {}", Number(entity)));
            if ui.button("Despawn").clicked {
                world.commands().despawn(entity);
                set_note(
                    &mut self.note,
                    format_args!("Despawned {}.", Number(entity)),
                );
            }
        });
        ui.horizontal(|ui| {
            ui.text_input("##prefab", &mut self.prefab_name);
            if ui.button("Save as prefab").clicked {
                if is_prefab_name(&self.prefab_name) {
                    response.save_prefab = true;
                } else {
                    set_note(
                        &mut self.note,
                        "Name the prefab first: lowercase letters, digits and _.",
                    );
                }
            }
        });
        self.lacking.clear();
        for registration in registry.iter() {
            if !has(world, registration, entity) {
                self.lacking.push(registration.name());
            }
        }
        if !self.lacking.is_empty() {
            self.add = self.add.min(self.lacking.len() - 1);
            let (add, data, lacking, note) = (
                &mut self.add,
                &mut self.add_data,
                &self.lacking,
                &mut self.note,
            );
            ui.horizontal(|ui| {
                ui.combo("##add", add, lacking);
                ui.text_input("##data", data);
                if ui.button("Add").clicked
                    && let Some(registration) =
                        lacking.get(*add).and_then(|name| registry.get(name))
                {
                    add_component(world, registration, entity, data, cx, note);
                }
            });
        }
        if !self.note.is_empty() {
            ui.label(&self.note);
        }
        // In a scope of their own, so that a component named as one of the
        // inspector's own widgets ("Filter", "Add") has an identity of its
        // own, as two widgets of one text in two scopes do.
        let (edit, scratch, note) = (&mut self.edit, &mut self.scratch, &mut self.note);
        ui.with_id("nadir::components", |ui| {
            for registration in registry.iter() {
                let name = registration.name();
                let key = Id::ROOT.with((entity.index(), entity.generation().get(), name));
                let mut remove = false;
                let mut editor = Editor {
                    cx: &mut *cx,
                    edit: &mut *edit,
                    scratch: &mut *scratch,
                    note: &mut *note,
                };
                registration.inspect_mut(world, entity, &mut |component| {
                    ui.tree_node(name, |ui| {
                        remove = ui.button("Remove").clicked;
                        editor.within(Step::Key(name), |editor| {
                            edit_component(ui, component, key, editor);
                        });
                    });
                });
                if remove {
                    registration.remove(&mut world.commands(), entity);
                    set_note(note, format_args!("Removed {name}."));
                }
            }
        });
    }
}

/// Gives `entity` a component of `registration`'s type, made from `data`,
/// JSONC, or an empty object if it is blank, through the world's commands;
/// or says in `note` why not.
fn add_component(
    world: &World,
    registration: &Registration,
    entity: Entity,
    data: &str,
    cx: &mut ReflectContext<'_>,
    note: &mut String,
) {
    let name = registration.name();
    let value = if data.trim().is_empty() {
        Value::Object(Vec::new())
    } else {
        match json::parse(data, Limits::default()) {
            Ok(value) => value,
            Err(error) => {
                refuse(note, format_args!("{name} is not added: its data: {error}"));
                return;
            }
        }
    };
    let added = cx.key(name, |cx| {
        registration.insert(&mut world.commands(), entity, &value, cx)
    });
    match added {
        Ok(()) => set_note(note, format_args!("Added {name}.")),
        Err(error) => refuse(note, format_args!("{name} is not added: {error}")),
    }
}

/// Whether `entity` has a component of `registration`'s type.
fn has(world: &World, registration: &Registration, entity: Entity) -> bool {
    registration.inspect(world, entity, &mut |_| {})
}

/// Whether `entity` matches each word of `filter`: its number, or the name
/// of a component it has, holds the word, ignoring case. `scratch` holds
/// its number.
fn matches(
    world: &World,
    registry: &TypeRegistry,
    entity: Entity,
    filter: &str,
    scratch: &mut String,
) -> bool {
    scratch.clear();
    let _ = write!(scratch, "{}", Number(entity));
    filter.split_whitespace().all(|word| {
        contains_ignoring_case(scratch, word)
            || registry.iter().any(|registration| {
                contains_ignoring_case(registration.name(), word)
                    && has(world, registration, entity)
            })
    })
}

/// Whether `text` holds `word`, ignoring the case of ASCII letters.
fn contains_ignoring_case(text: &str, word: &str) -> bool {
    word.is_empty()
        || text
            .as_bytes()
            .windows(word.len())
            .any(|part| part.eq_ignore_ascii_case(word.as_bytes()))
}

/// Whether `name` may name a saved prefab, and its file: a name the prefab
/// reader and the cook take ([`prefab::is_name`]: lowercase ASCII letters,
/// digits and `_`), of at most [`MAX_PREFAB_NAME`] bytes.
fn is_prefab_name(name: &str) -> bool {
    name.len() <= MAX_PREFAB_NAME && prefab::is_name(name)
}

fn set_note(note: &mut String, text: impl fmt::Display) {
    note.clear();
    let _ = write!(note, "{text}");
}

/// Says in `note` why something was not done, and logs it: a window cuts a
/// long note at its edge, and the log keeps it whole.
fn refuse(note: &mut String, text: impl fmt::Display) {
    set_note(note, text);
    warn!("{note}");
}

/// An entity as the list shows it: `3v1`, its slot and its generation.
struct Number(Entity);

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.0.index(), self.0.generation())
    }
}

/// A row of the list: a mark if it is selected, its number, then the names
/// of its registered components.
struct Row<'a> {
    world: &'a World,
    registry: &'a TypeRegistry,
    entity: Entity,
    selected: bool,
}

impl fmt::Display for Row<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = if self.selected { '>' } else { ' ' };
        write!(f, "{mark} {}", Number(self.entity))?;
        for registration in self.registry.iter() {
            if has(self.world, registration, self.entity) {
                write!(f, " {}", registration.name())?;
            }
        }
        Ok(())
    }
}

/// What a row of an editor shows before or after its control.
#[derive(Clone, Copy, Hash)]
enum Label<'a> {
    Name(&'a str),
    Index(usize),
}

impl fmt::Display for Label<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Label::Name(name) => f.write_str(name),
            Label::Index(index) => write!(f, "{index}"),
        }
    }
}

/// One step of the context's path, to the field at hand.
enum Step<'a> {
    Field(&'static str),
    Key(&'a str),
    Index(usize),
}

/// What editing a component's values needs besides the UI and the values.
struct Editor<'e, 'c> {
    cx: &'e mut ReflectContext<'c>,
    edit: &'e mut Option<TextEdit>,
    scratch: &'e mut String,
    note: &'e mut String,
}

impl Editor<'_, '_> {
    /// Runs `f` with the context's path a step further: errors and
    /// warnings then name the field.
    fn within<R>(&mut self, step: Step<'_>, f: impl FnOnce(&mut Editor<'_, '_>) -> R) -> R {
        let Editor {
            cx,
            edit,
            scratch,
            note,
        } = self;
        let run = |cx: &mut ReflectContext<'_>| {
            f(&mut Editor {
                cx,
                edit,
                scratch,
                note,
            })
        };
        match step {
            Step::Field(name) => cx.field(name, run),
            Step::Key(key) => cx.key(key, run),
            Step::Index(index) => cx.index(index, run),
        }
    }

    /// Sets `value` to `new`, or says in the note why not.
    fn apply(&mut self, value: &mut dyn Reflect, new: &Value) {
        if let Err(error) = value.apply_value(new, self.cx) {
            refuse(self.note, format_args!("Not set: {error}"));
        }
    }
}

/// A component's rows, inside its node: a struct's fields; a value
/// reflected whole as an object, such as a sprite, its members; anything
/// else, one row.
fn edit_component(
    ui: &mut Ui<'_>,
    component: &mut dyn Reflect,
    key: Id,
    editor: &mut Editor<'_, '_>,
) {
    match component.kind() {
        ReflectKind::Struct => edit_fields(ui, component, key, editor),
        ReflectKind::Primitive | ReflectKind::Handle => {
            let current = component.to_value(editor.cx);
            let new = match &current {
                Value::Object(members) => edit_members(ui, members, key, Strings::Guess, editor),
                _ => edit_json(
                    ui,
                    Label::Name("value"),
                    &current,
                    key,
                    strings(component),
                    editor,
                ),
            };
            if let Some(new) = new {
                editor.apply(component, &new);
            }
        }
        ReflectKind::Enum | ReflectKind::List | ReflectKind::Option => {
            edit_value(ui, Label::Name("value"), component, key, editor);
        }
    }
}

/// A struct's fields, a row each, or a line saying it has none.
fn edit_fields(ui: &mut Ui<'_>, value: &mut dyn Reflect, key: Id, editor: &mut Editor<'_, '_>) {
    let fields = value.fields();
    if fields.is_empty() {
        ui.label("A tag: no fields.");
        return;
    }
    for (index, info) in fields.iter().enumerate() {
        let Some(field) = value.field_mut(index) else {
            continue;
        };
        editor.within(Step::Field(info.name), |editor| {
            edit_value(
                ui,
                Label::Name(info.name),
                field,
                key.with(info.name),
                editor,
            );
        });
    }
}

/// The row, or the node, of a reflected value.
fn edit_value(
    ui: &mut Ui<'_>,
    label: Label<'_>,
    value: &mut dyn Reflect,
    key: Id,
    editor: &mut Editor<'_, '_>,
) {
    match value.kind() {
        ReflectKind::Struct => {
            ui.tree_node(label, |ui| edit_fields(ui, value, key, editor));
        }
        ReflectKind::List => {
            let len = (0..)
                .take_while(|&index| value.field(index).is_some())
                .count();
            ui.tree_node(format_args!("{label} [{len}]##{label}"), |ui| {
                for index in 0..len {
                    let Some(element) = value.field_mut(index) else {
                        break;
                    };
                    editor.within(Step::Index(index), |editor| {
                        edit_value(ui, Label::Index(index), element, key.with(index), editor);
                    });
                }
            });
        }
        ReflectKind::Option => match value.field_mut(0) {
            Some(inner) => edit_value(ui, label, inner, key, editor),
            None => ui.label(format_args!("{label}: none")),
        },
        ReflectKind::Enum => {
            let variants = value.variants();
            let current = value.to_value(editor.cx);
            let new = if variants.is_empty() {
                // An enum written by hand, which lists none: its name typed.
                edit_json(ui, label, &current, key, Strings::Text, editor)
            } else {
                let mut picked = current
                    .as_str()
                    .and_then(|name| variants.iter().position(|variant| *variant == name))
                    .unwrap_or(variants.len());
                let changed = ui.combo(label, &mut picked, variants).changed;
                variants
                    .get(picked)
                    .filter(|_| changed)
                    .map(|variant| Value::String((*variant).to_string()))
            };
            if let Some(new) = new {
                editor.apply(value, &new);
            }
        }
        ReflectKind::Primitive | ReflectKind::Handle => {
            let current = value.to_value(editor.cx);
            if let Some(new) = edit_json(ui, label, &current, key, strings(value), editor) {
                editor.apply(value, &new);
            }
        }
    }
}

/// How the strings of a value's data are edited.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Strings {
    /// As text: a `String`'s, an asset path's, a newtype's around one.
    Text,
    /// As a color, when they write one: a `Color`'s.
    Color,
    /// As a whole number: a `u64`'s or a `usize`'s, which data writes as
    /// digits past an `i64`.
    Number,
    /// As a color when they write one, else as text: the members of a value
    /// reflected whole, such as a sprite's, whose types its data does not
    /// say.
    Guess,
}

/// How the strings of `value`'s own data are edited, by its type's name;
/// those of an object in it are guessed at.
fn strings(value: &dyn Reflect) -> Strings {
    match value.type_name() {
        "Color" => Strings::Color,
        "u64" | "usize" => Strings::Number,
        _ => Strings::Text,
    }
}

/// The row, or the node, of `value`, part of a reflected value's data. If
/// it changed: for an object, the member that changed, in an object, which
/// applying sets alone; otherwise the whole new value.
fn edit_json(
    ui: &mut Ui<'_>,
    label: Label<'_>,
    value: &Value,
    key: Id,
    strings: Strings,
    editor: &mut Editor<'_, '_>,
) -> Option<Value> {
    match value {
        Value::Null => {
            ui.label(format_args!("{label}: null"));
            None
        }
        &Value::Bool(on) => {
            let mut on = on;
            ui.checkbox(label, &mut on)
                .changed
                .then_some(Value::Bool(on))
        }
        &Value::Float(number) => {
            // Drawn and dragged as an `f32`: an `f64` is cut to one only
            // when it is dragged.
            let mut drawn = number as f32;
            ui.drag_f32(label, &mut drawn, DRAG_PER_UNIT)
                .changed
                .then(|| Value::Float(f64::from(drawn)))
        }
        &Value::Int(number) => text_field(ui, label, key, number, editor, whole_number),
        Value::String(text) => {
            let color = match strings {
                Strings::Color | Strings::Guess => color_of(text),
                Strings::Text | Strings::Number => None,
            };
            match color {
                Some(mut color) => ui
                    .color_edit(label, &mut color)
                    .changed
                    .then(|| color.to_value(editor.cx)),
                None if strings == Strings::Number => {
                    text_field(ui, label, key, text, editor, whole_number)
                }
                None => text_field(ui, label, key, text, editor, |text| {
                    Ok(Value::String(text.to_string()))
                }),
            }
        }
        Value::Array(items) => edit_array(ui, label, items, key, strings, editor),
        Value::Object(members) => {
            let mut new = None;
            ui.tree_node(label, |ui| {
                new = edit_members(ui, members, key, Strings::Guess, editor)
            });
            new
        }
    }
}

/// An object's members, a row each: the member that changed, in an object.
fn edit_members(
    ui: &mut Ui<'_>,
    members: &[(String, Value)],
    key: Id,
    strings: Strings,
    editor: &mut Editor<'_, '_>,
) -> Option<Value> {
    let mut new = None;
    for (name, member) in members {
        let key = key.with(name.as_str());
        if let Some(changed) = edit_json(ui, Label::Name(name), member, key, strings, editor) {
            new = Some(Value::Object(vec![(name.clone(), changed)]));
        }
    }
    new
}

/// A list's elements: up to four numbers in one row, as a position's two,
/// the label after the last, in a scope of the label's, as the numbers
/// before the last have none; others a row each under a node. The whole
/// new list, if one changed, as a list is applied whole.
fn edit_array(
    ui: &mut Ui<'_>,
    label: Label<'_>,
    items: &[Value],
    key: Id,
    strings: Strings,
    editor: &mut Editor<'_, '_>,
) -> Option<Value> {
    let numbers = (1..=4).contains(&items.len())
        && items
            .iter()
            .all(|item| matches!(item, Value::Int(_) | Value::Float(_)));
    let mut changed = None;
    if numbers {
        ui.with_id(label, |ui| {
            ui.horizontal(|ui| {
                for (index, item) in items.iter().enumerate() {
                    let shown = if index + 1 == items.len() {
                        label
                    } else {
                        Label::Name("")
                    };
                    ui.with_id(index, |ui| {
                        let key = key.with(index);
                        if let Some(new) = edit_json(ui, shown, item, key, strings, editor) {
                            changed = Some((index, new));
                        }
                    });
                }
            });
        });
    } else {
        ui.tree_node(format_args!("{label} [{}]##{label}", items.len()), |ui| {
            for (index, item) in items.iter().enumerate() {
                let shown = Label::Index(index);
                if let Some(new) = edit_json(ui, shown, item, key.with(index), strings, editor) {
                    changed = Some((index, new));
                }
            }
        });
    }
    let (index, new) = changed?;
    let mut items = items.to_vec();
    merge(&mut items[index], new);
    Some(Value::Array(items))
}

/// Applies `change`, from [`edit_json`], to `target`: an object's members
/// one by one, anything else whole.
fn merge(target: &mut Value, change: Value) {
    match (target, change) {
        (Value::Object(members), Value::Object(changes)) => {
            for (name, change) in changes {
                match members.iter_mut().find(|(member, _)| *member == name) {
                    Some((_, member)) => merge(member, change),
                    None => members.push((name, change)),
                }
            }
        }
        (target, change) => *target = change,
    }
}

/// A text field of `current`, or of the edit typed into it, which `parse`
/// makes a value of on Enter or a press elsewhere; Escape drops it. The
/// value, if one was made; a text `parse` refuses is dropped, and the note
/// says why.
fn text_field(
    ui: &mut Ui<'_>,
    label: Label<'_>,
    key: Id,
    current: impl fmt::Display,
    editor: &mut Editor<'_, '_>,
    parse: impl FnOnce(&str) -> Result<Value, String>,
) -> Option<Value> {
    let frame = ui.frame_number();
    let response = match editor.edit.as_mut().filter(|edit| edit.key == key) {
        Some(edit) => {
            edit.frame = frame;
            ui.text_input(label, &mut edit.text)
        }
        None => {
            editor.scratch.clear();
            let _ = write!(editor.scratch, "{current}");
            let response = ui.text_input(label, editor.scratch);
            if response.changed {
                *editor.edit = Some(TextEdit {
                    key,
                    text: editor.scratch.clone(),
                    frame,
                });
            }
            response
        }
    };
    if !(response.submitted || response.lost_focus) {
        return None;
    }
    let escaped = response.lost_focus && ui.key_pressed(Key::Escape);
    let edit = editor.edit.take_if(|edit| edit.key == key)?;
    if escaped {
        return None;
    }
    match parse(&edit.text) {
        Ok(value) => Some(value),
        Err(why) => {
            refuse(editor.note, format_args!("Not set: {why}"));
            None
        }
    }
}

/// A whole number typed: an integer, or digits past an `i64`, as data
/// writes a `u64` that large.
fn whole_number(text: &str) -> Result<Value, String> {
    let text = text.trim();
    if let Ok(number) = text.parse::<i64>() {
        return Ok(Value::Int(number));
    }
    if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(Value::String(text.to_string()));
    }
    Err(format!("`{text}` is not a whole number"))
}

/// The color `text` writes, if it is one as data writes them: `#rrggbb` or
/// `#rrggbbaa`.
fn color_of(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#')?;
    let hex = matches!(digits.len(), 6 | 8) && digits.bytes().all(|byte| byte.is_ascii_hexdigit());
    hex.then(|| Color::from_hex(digits).ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_matches_ignoring_case_and_an_empty_one_always() {
        assert!(contains_ignoring_case("Transform", "trans"));
        assert!(contains_ignoring_case("Transform", "FORM"));
        assert!(contains_ignoring_case("3v1", ""));
        assert!(!contains_ignoring_case("3v1", "3v12"));
        assert!(!contains_ignoring_case("Sprite", "sprites"));
    }

    #[test]
    fn a_prefab_s_name_is_lowercase_letters_digits_and_underscores() {
        assert!(is_prefab_name("slime_2_big"));
        for bad in [
            "",
            "Slime",
            "en/slime",
            "slime.kbj",
            "../up",
            "a b",
            "é",
            "slime-big",
        ] {
            assert!(!is_prefab_name(bad), "{bad:?}");
        }
        assert!(is_prefab_name(&"a".repeat(MAX_PREFAB_NAME)));
        assert!(!is_prefab_name(&"a".repeat(MAX_PREFAB_NAME + 1)));
    }

    #[test]
    fn whole_numbers_and_colors_read_as_data_writes_them() {
        assert_eq!(whole_number(" -12 "), Ok(Value::Int(-12)));
        assert_eq!(
            whole_number("18446744073709551615"),
            Ok(Value::String("18446744073709551615".to_string()))
        );
        assert!(whole_number("1.5").is_err() && whole_number("").is_err());
        assert_eq!(color_of("#ff8000"), Some(Color::rgb(255, 128, 0)));
        assert_eq!(color_of("#ff800080"), Some(Color::rgba(255, 128, 0, 128)));
        for not in [
            "ff8000",
            "#ff80",
            "#ff800",
            "#gg8000",
            "##ff8000",
            "#ff8000ff00",
        ] {
            assert_eq!(color_of(not), None, "{not:?}");
        }
    }

    #[test]
    fn a_change_merges_into_an_object_by_member_and_replaces_the_rest() {
        let object = |members: &[(&str, Value)]| {
            Value::Object(
                members
                    .iter()
                    .map(|(name, value)| ((*name).to_string(), value.clone()))
                    .collect(),
            )
        };
        let mut target = object(&[
            ("shape", object(&[("radius", Value::Float(6.0))])),
            ("layer", Value::Int(1)),
        ]);
        let change = object(&[("shape", object(&[("radius", Value::Float(7.0))]))]);
        merge(&mut target, change);
        let expected = object(&[
            ("shape", object(&[("radius", Value::Float(7.0))])),
            ("layer", Value::Int(1)),
        ]);
        assert_eq!(target, expected);
        merge(&mut target, Value::Int(3));
        assert_eq!(target, Value::Int(3));
    }
}
