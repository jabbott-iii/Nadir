//! `cargo xtask features`: `docs/features.jsonc`, the engine's features as
//! the demo page lists them, covers the roadmap (ADR-0029).
//!
//! Each feature names the roadmap tasks it needs. The page shows it as
//! shipped when every one of them is ticked, in progress when some are,
//! and planned when none are. Each also lists, as `ticked`, the tasks that
//! were ticked when its summary was written. The check fails when a ticked
//! task belongs to no feature, or when a feature's `ticked` is not what
//! the roadmap ticks: so the task that ships something also describes it,
//! in a new feature or by rewriting its feature's summary.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use nadir_codec::Limits;
use nadir_codec::json::{self, Value};

use crate::check::Report;
use crate::error::Error;

/// The features, relative to the workspace root.
pub(crate) const FEATURES: &str = "docs/features.jsonc";
/// The roadmap, relative to the workspace root.
pub(crate) const ROADMAP: &str = "docs/roadmap.md";

/// A feature of the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Feature {
    /// What it is called: a few words.
    pub(crate) name: String,
    /// The part of the engine it belongs to, such as `Graphics`.
    pub(crate) area: String,
    /// What it does, in a sentence or three.
    pub(crate) summary: String,
    /// The roadmap tasks it needs, such as `M3.3`.
    pub(crate) tasks: Vec<String>,
    /// Those of `tasks` that were ticked when `summary` was written.
    pub(crate) ticked: Vec<String>,
}

/// How far a feature has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Status {
    /// Every task it needs is ticked.
    Shipped,
    /// Some are.
    InProgress,
    /// None are.
    Planned,
}

impl Status {
    /// The status as the page names it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Status::Shipped => "Shipped",
            Status::InProgress => "In progress",
            Status::Planned => "Planned",
        }
    }
}

/// A task of the roadmap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Task {
    /// Its ID, such as `M3.3`.
    pub(crate) id: String,
    /// Its title, after the ID.
    pub(crate) title: String,
    /// Whether its box is ticked.
    pub(crate) done: bool,
}

/// Runs the check on the workspace at `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let roadmap = read(root, ROADMAP)?;
    let features = read(root, FEATURES)?;
    Ok(check(&roadmap, &features))
}

/// The text of the file at `path`, relative to `root`.
pub(crate) fn read(root: &Path, path: &str) -> Result<String, Error> {
    let path = root.join(path);
    fs::read_to_string(&path).map_err(|source| Error::Read { path, source })
}

/// Checks the text of the features file against the roadmap's.
fn check(roadmap: &str, features: &str) -> Report {
    let mut report = Report::default();
    // A task the reading below misses would escape the check.
    for line in malformed(roadmap) {
        report.violations.push(format!(
            "{ROADMAP}: a line like a task's that is not one: `{line}`: write it \
             `- [ ] **M1.2 — title**`, ticked as `[x]`"
        ));
    }
    let tasks = &tasks(roadmap);
    let features = match parse(features) {
        Ok(features) => features,
        Err(problems) => {
            report.violations.extend(problems);
            return report;
        }
    };
    let mut names = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for feature in &features {
        if !names.insert(feature.name.as_str()) {
            report.violations.push(format!(
                "{FEATURES}: the feature `{}` is listed twice",
                feature.name
            ));
        }
        let mut own = BTreeSet::new();
        for id in &feature.tasks {
            if !own.insert(id.as_str()) {
                report
                    .violations
                    .push(format!("{FEATURES}: `{}` names {id} twice", feature.name));
            }
            if !tasks.iter().any(|task| task.id == *id) {
                report.violations.push(format!(
                    "{FEATURES}: `{}` needs {id}, which is not a task in {ROADMAP}",
                    feature.name
                ));
            }
            covered.insert(id.as_str());
        }
        let mut listed = BTreeSet::new();
        for id in &feature.ticked {
            if !listed.insert(id.as_str()) {
                report.violations.push(format!(
                    "{FEATURES}: `{}` lists {id} as ticked twice",
                    feature.name
                ));
            }
            if !feature.tasks.contains(id) {
                report.violations.push(format!(
                    "{FEATURES}: `{}` lists {id} as ticked, but not among its tasks",
                    feature.name
                ));
            }
        }
        let ticked: BTreeSet<&str> = feature
            .tasks
            .iter()
            .filter(|id| tasks.iter().any(|task| task.id == **id && task.done))
            .map(String::as_str)
            .collect();
        let written: BTreeSet<&str> = feature.ticked.iter().map(String::as_str).collect();
        if ticked != written && written.is_subset(&own) {
            let list = |ids: &BTreeSet<&str>| match ids.len() {
                0 => "none".to_string(),
                _ => ids.iter().copied().collect::<Vec<_>>().join(", "),
            };
            report.violations.push(format!(
                "{FEATURES}: `{}`'s summary was written with {} ticked, but the roadmap \
                 ticks {}: rewrite it for what is built now, then set `ticked`",
                feature.name,
                list(&written),
                list(&ticked)
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for task in tasks {
        if !seen.insert(task.id.as_str()) {
            report.violations.push(format!(
                "{ROADMAP}: {} is the ID of two tasks: give the new one its own",
                task.id
            ));
        }
        if task.done && !covered.contains(task.id.as_str()) {
            report.violations.push(format!(
                "{ROADMAP}: {} ({}) is ticked, but no feature in {FEATURES} needs it",
                task.id, task.title
            ));
        }
    }
    let count = |status| {
        features
            .iter()
            .filter(|feature| self::status(feature, tasks) == status)
            .count()
    };
    report.notes.push(format!(
        "features: {} shipped, {} in progress, {} planned; roadmap tasks: {} of {} ticked",
        count(Status::Shipped),
        count(Status::InProgress),
        count(Status::Planned),
        tasks.iter().filter(|task| task.done).count(),
        tasks.len()
    ));
    if !report.violations.is_empty() {
        report.notes.push(format!(
            "Every ticked task belongs to a feature in {FEATURES}, which the demo page \
             lists, and each feature's summary says what its ticked tasks built: add the \
             task to a feature or describe a new one, and keep `ticked` in step (ADR-0029)."
        ));
    }
    report
}

/// The roadmap's tasks, in order: its lines `- [x] **M3.3 — title**`
/// (or `[X]`), or with `[ ]` for a task not yet done.
pub(crate) fn tasks(roadmap: &str) -> Vec<Task> {
    roadmap.lines().filter_map(task).collect()
}

/// The roadmap's lines that look like a task's but that [`tasks`] cannot
/// read: indented, with another bullet or dash than `- ` and ` — `, more
/// spaces, an ID that is not `M<number>.<number>`, or another mark in the
/// box.
fn malformed(roadmap: &str) -> Vec<&str> {
    roadmap
        .lines()
        .filter(|line| looks_like_task(line) && task(line).is_none())
        .collect()
}

/// Whether `line` looks like a task's: a list item, indented or quoted or
/// not, with a bullet (`-`, `*`, `+`) or a number (`1.`, `1)`), spaces or
/// tabs, then a checkbox of one character, and a task ID after it, such as
/// `M5.3` or `m5.3`, bold or not.
fn looks_like_task(line: &str) -> bool {
    let item = line.trim_start_matches(|c: char| c.is_whitespace() || c == '>');
    let after_bullet = match item.strip_prefix(['-', '*', '+']) {
        Some(rest) => rest,
        None => {
            let digits = item.len() - item.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            match item[digits..].strip_prefix(['.', ')']) {
                Some(rest) if digits > 0 => rest,
                _ => return false,
            }
        }
    };
    let spaced = after_bullet.trim_start_matches([' ', '\t']);
    if spaced.len() == after_bullet.len() {
        return false;
    }
    let mut chars = spaced.chars();
    let checkbox = chars.next() == Some('[') && chars.next().is_some() && chars.next() == Some(']');
    checkbox && has_task_id(chars.as_str())
}

/// Whether `text` holds what starts a task ID: `M` or `m`, digits and `.`,
/// not inside a longer word. What follows the `.` may be a typo, such as
/// `M0.x`, which [`task`] then refuses.
fn has_task_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len()).any(|at| {
        let starts_word = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let digits = |from: usize| {
            bytes[from.min(bytes.len())..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count()
        };
        let milestone = digits(at + 1);
        starts_word
            && matches!(bytes[at], b'M' | b'm')
            && milestone > 0
            && bytes.get(at + 1 + milestone) == Some(&b'.')
    })
}

/// The task on `line`, if it is a task's line.
fn task(line: &str) -> Option<Task> {
    let rest = line.strip_prefix("- [")?;
    let (done, rest) = match rest.split_at_checked(1)? {
        ("x" | "X", rest) => (true, rest),
        (" ", rest) => (false, rest),
        _ => return None,
    };
    let rest = rest.strip_prefix("] **")?;
    let (id, title) = rest.split_once(" — ")?;
    let title = title.split_once("**")?.0;
    let well_formed = id.strip_prefix('M').is_some_and(|number| {
        number.split_once('.').is_some_and(|(milestone, task)| {
            [milestone, task]
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        })
    });
    well_formed.then(|| Task {
        id: id.to_string(),
        title: title.to_string(),
        done,
    })
}

/// How far `feature` has come, by the ticks of the roadmap's `tasks`.
pub(crate) fn status(feature: &Feature, tasks: &[Task]) -> Status {
    let done = feature
        .tasks
        .iter()
        .filter(|id| tasks.iter().any(|task| task.id == **id && task.done))
        .count();
    if done == feature.tasks.len() {
        Status::Shipped
    } else if done > 0 {
        Status::InProgress
    } else {
        Status::Planned
    }
}

/// Reads the features file: an object whose only member, `features`, is
/// an array of objects with the strings `name`, `area` and `summary`, and
/// `tasks`, an array of at least one task ID. Returns every problem found.
pub(crate) fn parse(text: &str) -> Result<Vec<Feature>, Vec<String>> {
    let root = json::parse(text, Limits::default())
        .map_err(|error| vec![format!("{FEATURES}: {error}")])?;
    let list = match root.as_object() {
        Some([(key, list)]) if key == "features" => list.as_array(),
        _ => None,
    }
    .ok_or_else(|| {
        vec![format!(
            "{FEATURES}: expected an object whose only member is the array `features`"
        )]
    })?;
    let mut features = Vec::new();
    let mut problems = Vec::new();
    for (index, entry) in list.iter().enumerate() {
        match feature(entry) {
            Ok(feature) => features.push(feature),
            Err(problem) => problems.push(format!("{FEATURES}: feature {}: {problem}", index + 1)),
        }
    }
    if problems.is_empty() {
        Ok(features)
    } else {
        Err(problems)
    }
}

/// One entry of the `features` array.
fn feature(entry: &Value) -> Result<Feature, String> {
    let members = entry
        .as_object()
        .ok_or("expected an object with `name`, `area`, `summary` and `tasks`")?;
    if let Some((key, _)) = members
        .iter()
        .find(|(key, _)| !["name", "area", "summary", "tasks", "ticked"].contains(&key.as_str()))
    {
        return Err(format!("unknown member `{key}`"));
    }
    let text = |key: &str| -> Result<String, String> {
        match entry.get(key).and_then(Value::as_str).map(str::trim) {
            Some(text) if !text.is_empty() => Ok(text.to_string()),
            _ => Err(format!("`{key}` must be a string that is not blank")),
        }
    };
    let name = text("name")?;
    let area = text("area")?;
    let summary = text("summary")?;
    let tasks: Vec<String> = entry
        .get("tasks")
        .and_then(Value::as_array)
        .and_then(|ids| ids.iter().map(|id| id.as_str().map(String::from)).collect())
        .filter(|ids: &Vec<String>| !ids.is_empty())
        .ok_or_else(|| format!("`{name}`: `tasks` must be an array of task IDs, not empty"))?;
    let ticked: Vec<String> = match entry.get("ticked") {
        None => Vec::new(),
        Some(ids) => ids
            .as_array()
            .and_then(|ids| ids.iter().map(|id| id.as_str().map(String::from)).collect())
            .ok_or_else(|| format!("`{name}`: `ticked` must be an array of task IDs"))?,
    };
    Ok(Feature {
        name,
        area,
        summary,
        tasks,
        ticked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROADMAP_TEXT: &str = "\
# Roadmap

## M0 — Foundation

- [x] **M0.1 — Workspace skeleton**
  - Do: things
- [ ] **M0.2 — core: math and color**
- [x] **M0.3 — core: handles, hashing, RNG**
- [x] Not a task: no ID

| Item | Trigger |
| :- | :- |
| Native Wayland | - [x] **M9.9 — in a table, not at the line's start** |
";

    fn roadmap() -> Vec<Task> {
        tasks(ROADMAP_TEXT)
    }

    /// A feature that needs `ids`, written with those that `ROADMAP_TEXT`
    /// ticks ticked.
    fn feature(name: &str, ids: &[&str]) -> String {
        let done: Vec<String> = roadmap()
            .into_iter()
            .filter(|task| task.done)
            .map(|task| task.id)
            .collect();
        let mut ticked: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|id| done.iter().any(|done| done == id))
            .collect();
        ticked.dedup();
        feature_written_with(name, ids, &ticked)
    }

    /// A feature that needs `ids`, written with `ticked` ticked.
    fn feature_written_with(name: &str, ids: &[&str], ticked: &[&str]) -> String {
        let list = |ids: &[&str]| {
            ids.iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "{{ \"name\": \"{name}\", \"area\": \"Core\", \"summary\": \"It works.\", \
             \"tasks\": [{}], \"ticked\": [{}] }}",
            list(ids),
            list(ticked)
        )
    }

    fn file(features: &[String]) -> String {
        format!(
            "// The features.\n{{\n  \"features\": [\n    {}\n  ]\n}}\n",
            features.join(",\n    ")
        )
    }

    #[test]
    fn the_roadmaps_tasks_are_its_checkbox_lines_with_an_id() {
        let text = format!("{ROADMAP_TEXT}- [X] **M0.4 — ticked with a capital**\n");
        let tasks = tasks(&text);
        let ids: Vec<(&str, &str, bool)> = tasks
            .iter()
            .map(|task| (task.id.as_str(), task.title.as_str(), task.done))
            .collect();
        assert_eq!(
            ids,
            [
                ("M0.1", "Workspace skeleton", true),
                ("M0.2", "core: math and color", false),
                ("M0.3", "core: handles, hashing, RNG", true),
                ("M0.4", "ticked with a capital", true),
            ]
        );
        assert_eq!(malformed(&text), Vec::<&str>::new());
    }

    #[test]
    fn a_line_like_a_tasks_that_is_not_one_fails() {
        let bad = [
            "- [x] **M5.9 - an ASCII hyphen**",
            "- [x] **M5.9 – an en dash**",
            "- [ ] **M0.x — not a number**",
            "- [?] **M0.4 — an unknown mark**",
            "  - [x] **M5.3 — indented**",
            "- [x]  **M5.3 — two spaces**",
            "* [x] **M5.3 — another bullet**",
            "-  [x] **M5.3 — two spaces after the bullet**",
            "-\t[x] **M5.3 — a tab after the bullet**",
            "1. [x] **M5.3 — a numbered item**",
            "12) [ ] **M5.3 — a numbered item with a parenthesis**",
            "- [x] M12.1 — native Wayland, not bold",
            "- [x] __M12.1 — underscores__",
            "- [x] *M12.1 — one star*",
            "- [x] **m12.1 — a small m**",
            "- [x] **[M12.1 — a link](decisions.md)**",
            "> - [x] **M12.1 — quoted**",
        ];
        let text = format!("{ROADMAP_TEXT}{}\n", bad.join("\n"));
        assert_eq!(malformed(&text), bad);
        let report = check(
            &text,
            &file(&[
                feature("Workspace", &["M0.1"]),
                feature("Math", &["M0.2", "M0.3"]),
            ]),
        );
        assert_eq!(report.violations.len(), 17, "{:?}", report.violations);
        // Not task-like: no checkbox, no space after the bullet, no task ID,
        // no bullet, or a link where the checkbox would be.
        for line in [
            "- **M5.3 — no checkbox**",
            "-[x] **M5.3 — no space**",
            "- [x] **Milestone**",
            "- [x] **ADM5.3**",
            "- [x] **M5 — a milestone**",
            "[x] **M5.3 — no bullet**",
            "1.5 [x] **M5.3 — a decimal**",
            "  - [ADR-0026](decisions.md): **M4.8** moved",
        ] {
            assert!(!looks_like_task(line), "{line}");
        }
        assert_eq!(
            report.violations[0],
            "docs/roadmap.md: a line like a task's that is not one: `- [x] **M5.9 - an ASCII \
             hyphen**`: write it `- [ ] **M1.2 — title**`, ticked as `[x]`"
        );
    }

    #[test]
    fn a_feature_is_shipped_planned_or_in_between_by_its_tasks_ticks() {
        let tasks = roadmap();
        let status = |ids: &[&str]| {
            let feature = Feature {
                name: "x".to_string(),
                area: "Core".to_string(),
                summary: "It works.".to_string(),
                tasks: ids.iter().map(|id| (*id).to_string()).collect(),
                ticked: Vec::new(),
            };
            super::status(&feature, &tasks)
        };
        assert_eq!(status(&["M0.1", "M0.3"]), Status::Shipped);
        assert_eq!(status(&["M0.1", "M0.2"]), Status::InProgress);
        assert_eq!(status(&["M0.2"]), Status::Planned);
        assert_eq!(Status::InProgress.name(), "In progress");
    }

    #[test]
    fn features_that_cover_every_ticked_task_pass() {
        let text = file(&[
            feature("Workspace", &["M0.1"]),
            feature("Math", &["M0.2", "M0.3"]),
        ]);
        let report = check(ROADMAP_TEXT, &text);
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            ["features: 1 shipped, 1 in progress, 0 planned; roadmap tasks: 2 of 3 ticked"]
        );
    }

    #[test]
    fn a_ticked_task_that_no_feature_needs_fails() {
        let report = check(ROADMAP_TEXT, &file(&[feature("Workspace", &["M0.1"])]));
        assert_eq!(
            report.violations,
            [
                "docs/roadmap.md: M0.3 (core: handles, hashing, RNG) is ticked, but no \
                 feature in docs/features.jsonc needs it"
            ]
        );
        assert!(report.notes[1].contains("add the task to a feature"));
    }

    #[test]
    fn unknown_and_repeated_tasks_and_repeated_names_fail() {
        let text = file(&[
            feature("Workspace", &["M0.1", "M0.1", "M7.7"]),
            feature("Workspace", &["M0.3"]),
        ]);
        assert_eq!(
            check(ROADMAP_TEXT, &text).violations,
            [
                "docs/features.jsonc: `Workspace` names M0.1 twice",
                "docs/features.jsonc: `Workspace` needs M7.7, which is not a task in \
                 docs/roadmap.md",
                "docs/features.jsonc: the feature `Workspace` is listed twice",
            ]
        );
    }

    #[test]
    fn a_file_of_the_wrong_shape_says_what_is_wrong_with_each_feature() {
        let wrong = [
            (
                "[]",
                "expected an object whose only member is the array `features`",
            ),
            (
                "{ \"features\": [], \"more\": 1 }",
                "expected an object whose only member is the array `features`",
            ),
            ("{ \"features\": [1", "docs/features.jsonc: "),
        ];
        for (text, message) in wrong {
            let problems = parse(text).expect_err(text);
            assert_eq!(problems.len(), 1, "{text}: {problems:?}");
            assert!(problems[0].contains(message), "{text}: {problems:?}");
        }
        let text = r#"{ "features": [
            1,
            { "name": "A", "area": "Core", "summary": " ", "tasks": ["M0.1"] },
            { "name": "B", "area": "Core", "summary": "Yes.", "tasks": [] },
            { "name": "C", "area": "Core", "summary": "Yes.", "tasks": ["M0.1", 2] },
            { "name": "D", "area": "Core", "summary": "Yes.", "tasks": ["M0.1"], "x": 1 },
            { "area": "Core", "summary": "Yes.", "tasks": ["M0.1"] },
            { "name": "E", "area": "Core", "summary": "Yes.", "tasks": ["M0.1"] }
        ] }"#;
        assert_eq!(
            parse(text).expect_err("wrong features"),
            [
                "docs/features.jsonc: feature 1: expected an object with `name`, `area`, \
                 `summary` and `tasks`",
                "docs/features.jsonc: feature 2: `summary` must be a string that is not blank",
                "docs/features.jsonc: feature 3: `B`: `tasks` must be an array of task IDs, \
                 not empty",
                "docs/features.jsonc: feature 4: `C`: `tasks` must be an array of task IDs, \
                 not empty",
                "docs/features.jsonc: feature 5: unknown member `x`",
                "docs/features.jsonc: feature 6: `name` must be a string that is not blank",
            ]
        );
        let report = check(ROADMAP_TEXT, "{");
        assert_eq!(report.violations.len(), 1);
        assert!(report.notes.is_empty());
    }

    #[test]
    fn the_names_and_summaries_are_trimmed() {
        let text = r#"{ "features": [
            { "name": " A ", "area": "Core\n", "summary": " Yes. ", "tasks": ["M0.1"] }
        ] }"#;
        let features = parse(text).expect("valid");
        assert_eq!(
            features,
            [Feature {
                name: "A".to_string(),
                area: "Core".to_string(),
                summary: "Yes.".to_string(),
                tasks: vec!["M0.1".to_string()],
                ticked: Vec::new(),
            }]
        );
    }

    #[test]
    fn a_summary_written_for_other_ticks_than_the_roadmaps_fails() {
        let check = |ticked: &[&str]| {
            let text = file(&[
                feature("Workspace", &["M0.1"]),
                feature_written_with("Math", &["M0.2", "M0.3"], ticked),
            ]);
            super::check(ROADMAP_TEXT, &text).violations
        };
        assert_eq!(check(&["M0.3"]), Vec::<String>::new());
        assert_eq!(
            check(&[]),
            [
                "docs/features.jsonc: `Math`'s summary was written with none ticked, but the \
                 roadmap ticks M0.3: rewrite it for what is built now, then set `ticked`"
            ]
        );
        assert_eq!(
            check(&["M0.2", "M0.3"]),
            [
                "docs/features.jsonc: `Math`'s summary was written with M0.2, M0.3 ticked, but \
                 the roadmap ticks M0.3: rewrite it for what is built now, then set `ticked`"
            ]
        );
        assert_eq!(
            check(&["M0.3", "M0.1"]),
            ["docs/features.jsonc: `Math` lists M0.1 as ticked, but not among its tasks"]
        );
        assert_eq!(
            check(&["M0.3", "M0.3"]),
            ["docs/features.jsonc: `Math` lists M0.3 as ticked twice"]
        );
        let text = r#"{ "features": [
            { "name": "A", "area": "Core", "summary": "Yes.", "tasks": ["M0.1"], "ticked": "M0.1" }
        ] }"#;
        assert_eq!(
            parse(text).expect_err("not an array"),
            ["docs/features.jsonc: feature 1: `A`: `ticked` must be an array of task IDs"]
        );
    }

    #[test]
    fn two_tasks_with_one_id_fail() {
        let text = format!("{ROADMAP_TEXT}- [ ] **M0.3 — a copy, not renumbered**\n");
        let report = super::check(
            &text,
            &file(&[
                feature("Workspace", &["M0.1"]),
                feature("Math", &["M0.2", "M0.3"]),
            ]),
        );
        assert_eq!(
            report.violations,
            ["docs/roadmap.md: M0.3 is the ID of two tasks: give the new one its own"]
        );
    }

    #[test]
    fn malformed_task_lines_are_reported_with_a_features_file_that_does_not_parse() {
        let text = format!("{ROADMAP_TEXT}- [x] **M5.9 - an ASCII hyphen**\n");
        let report = super::check(&text, "{");
        assert_eq!(report.violations.len(), 2, "{:?}", report.violations);
        assert!(report.violations[0].contains("M5.9 - an ASCII hyphen"));
    }

    #[test]
    fn the_workspaces_features_cover_its_roadmap() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace");
        let report = run(root).expect("the files read");
        assert_eq!(report.violations, Vec::<String>::new());
    }
}
