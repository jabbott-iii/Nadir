//! The demo page's HTML: `site/index.html` with its placeholders filled
//! from the build, the recordings and the features. Everything taken from
//! them is escaped here.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::features::{Feature, Status};

/// The commit the page was built from, and the workspace at it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Build {
    /// The full commit ID.
    pub(crate) commit: String,
    /// The commit's date, `YYYY-MM-DD`.
    pub(crate) date: String,
    /// The first line of its message.
    pub(crate) subject: String,
    /// The pinned toolchain, such as `1.99.0`.
    pub(crate) toolchain: String,
    /// The workspace's packages.
    pub(crate) crates: usize,
    /// The packages from outside the workspace.
    pub(crate) third_party: usize,
    /// The repository's web address, with no `/` at the end.
    pub(crate) repository: String,
}

impl Build {
    /// The commit's first 7 characters.
    pub(crate) fn short(&self) -> &str {
        self.commit.get(..7).unwrap_or(&self.commit)
    }
}

/// A scene of the sandbox the page shows.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Scene {
    /// Its `--scene` name, which names its files.
    pub(crate) name: &'static str,
    /// Its tab.
    pub(crate) title: &'static str,
    /// What it shows.
    pub(crate) caption: &'static str,
    /// The frame its poster image is.
    pub(crate) poster_frame: u32,
}

/// What a capture saw: the sandbox's `capture.json`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Capture {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) fps: u32,
    pub(crate) frames: u32,
    pub(crate) ticks: u64,
    pub(crate) world_hash: String,
    pub(crate) mean_sprites: f64,
    pub(crate) mean_batches: f64,
    pub(crate) mean_tiles: f64,
    pub(crate) mean_tick_ms: f64,
    pub(crate) longest_tick_ms: f64,
    pub(crate) renderer: String,
}

/// A scene, and what its capture saw.
#[derive(Debug, PartialEq)]
pub(crate) struct Recording {
    pub(crate) scene: &'static Scene,
    pub(crate) capture: Capture,
}

/// The sandbox, packed for download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Download {
    /// The file's name, in `downloads/`.
    pub(crate) file: String,
    /// Its size.
    pub(crate) bytes: u64,
    /// Its SHA-256, in hex.
    pub(crate) sha256: String,
    /// The oldest glibc the sandbox runs on, such as `2.34`, if it names
    /// one.
    pub(crate) glibc: Option<String>,
}

/// A milestone of the roadmap, and how many of its tasks are ticked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Milestone {
    /// Its ID, such as `M5`.
    pub(crate) id: String,
    /// Its title, such as `Content pipeline`.
    pub(crate) title: String,
    pub(crate) done: usize,
    pub(crate) total: usize,
}

/// Everything the page shows.
pub(crate) struct Page<'a> {
    pub(crate) build: &'a Build,
    pub(crate) recordings: &'a [Recording],
    pub(crate) download: &'a Download,
    pub(crate) milestones: &'a [Milestone],
    /// The features, each with how far it has come, in the file's order.
    pub(crate) features: &'a [(Feature, Status)],
}

/// The page: `template` with each `{{name}}` replaced.
///
/// # Errors
///
/// Names a placeholder the page has no value for, or one left open.
pub(crate) fn render(template: &str, page: &Page) -> Result<String, String> {
    fill(template, &values(page))
}

/// `template` with each `{{name}}` replaced by its value.
fn fill(template: &str, values: &BTreeMap<&str, String>) -> Result<String, String> {
    let mut out = String::with_capacity(template.len() * 2);
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| "the template has a `{{` with no `}}`".to_string())?;
        let name = &after[..end];
        let value = values
            .get(name)
            .ok_or_else(|| format!("the template's `{{{{{name}}}}}` has no value"))?;
        out.push_str(value);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The value of every placeholder, as HTML.
fn values(page: &Page) -> BTreeMap<&'static str, String> {
    let build = page.build;
    let count = |status| {
        page.features
            .iter()
            .filter(|(_, other)| *other == status)
            .count()
    };
    let (shipped, in_progress, planned) = (
        count(Status::Shipped),
        count(Status::InProgress),
        count(Status::Planned),
    );
    let done: usize = page.milestones.iter().map(|milestone| milestone.done).sum();
    let total: usize = page
        .milestones
        .iter()
        .map(|milestone| milestone.total)
        .sum();
    let third_party = match build.third_party {
        0 => "None".to_string(),
        n => n.to_string(),
    };
    // Each build's media and download have the same names, so their
    // addresses carry the commit, as the stylesheet's and script's do: a
    // page and the files it names stay together through the caches.
    let version = build.short();
    let poster = page.recordings.first().map_or(String::new(), |recording| {
        format!("media/{}.png?v={version}", recording.scene.name)
    });
    let mut values = BTreeMap::new();
    let mut set = |name, value: String| {
        values.insert(name, value);
    };
    set(
        "description",
        escape(
            "Nadir, a 2D top-down game engine in Rust with no third-party crates: \
             its scenes as the latest build draws them, and its features.",
        ),
    );
    set(
        "lede",
        escape(
            "A 2D top-down game engine for desktop, written in Rust from the ground up: \
             its own OS bindings, OpenGL renderer, ECS, collision, codecs and tools, with \
             no third-party crates. What plays below is the engine itself, drawn frame by \
             frame in CI from the commit named beside it, published when that commit \
             passed CI on main.",
        ),
    );
    set("poster", escape(&poster));
    set("commit", escape(&build.commit));
    set("commit_short", escape(build.short()));
    set(
        "commit_url",
        escape(&format!("{}/commit/{}", build.repository, build.commit)),
    );
    set("commit_date", escape(&build.date));
    set("commit_subject", escape(&build.subject));
    set("repository_url", escape(&build.repository));
    set("toolchain", escape(&build.toolchain));
    set("crates", build.crates.to_string());
    set("third_party", third_party);
    set("tasks_done", done.to_string());
    set("tasks_total", total.to_string());
    set("scene_tabs", scene_tabs(page.recordings));
    set("scenes", scenes(page.recordings, version));
    set("milestones", milestones(page.milestones));
    set("download", download(page.download, version));
    set(
        "feature_filters",
        feature_filters(page.features.len(), shipped, in_progress, planned),
    );
    set(
        "features_lede",
        escape(&format!(
            "{shipped} shipped, {in_progress} in progress and {planned} planned. Each \
             feature is marked from the roadmap's ticks at {}, so the list moves as the \
             tasks land.",
            build.short()
        )),
    );
    set("features", features(page.features));
    values
}

/// The scenes' tabs.
fn scene_tabs(recordings: &[Recording]) -> String {
    let mut html = String::new();
    for (index, recording) in recordings.iter().enumerate() {
        let name = escape(recording.scene.name);
        let first = index == 0;
        let _ = writeln!(
            html,
            "            <button class=\"chip{}\" type=\"button\" role=\"tab\" id=\"tab-{name}\" \
             aria-controls=\"scene-{name}\" aria-selected=\"{first}\"{}>{}</button>",
            if first { " is-active" } else { "" },
            if first { "" } else { " tabindex=\"-1\"" },
            escape(recording.scene.title),
        );
    }
    trim_line_end(html)
}

/// Each scene: its video, what it shows, and what its capture saw. Each is
/// labelled by its title, and becomes a tab panel only when `nadir.js`
/// shows the tabs: without it, the scenes show one under the other.
/// `version` goes on the media's addresses.
fn scenes(recordings: &[Recording], version: &str) -> String {
    let mut html = String::new();
    for (index, recording) in recordings.iter().enumerate() {
        let scene = recording.scene;
        let capture = &recording.capture;
        let name = escape(scene.name);
        let seconds = f64::from(capture.frames) / f64::from(capture.fps.max(1));
        let _ = write!(
            html,
            "        <figure class=\"nadir-scene\" id=\"scene-{name}\" \
             aria-labelledby=\"title-{name}\" data-scene=\"{name}\">\n\
             \x20         <div class=\"nadir-screen\">\n\
             \x20           <video class=\"nadir-video\" width=\"{w}\" height=\"{h}\" \
             poster=\"media/{name}.png?v={version}\" preload=\"{preload}\" muted loop playsinline \
             controls{autoplay}>\n\
             \x20             <source src=\"media/{name}.webm?v={version}\" type=\"video/webm\">\n\
             \x20             <p>This browser cannot play WebM (VP9) video.</p>\n\
             \x20           </video>\n\
             \x20         </div>\n\
             \x20         <figcaption class=\"nadir-scene__caption\">\n\
             \x20           <h2 class=\"nadir-scene__title\" id=\"title-{name}\">{title}</h2>\n\
             \x20           <p class=\"nadir-scene__text\">{caption}</p>\n\
             \x20           <dl class=\"spec nadir-stats\">\n\
             \x20             <div><dt>Frames</dt><dd>{frames} in {seconds:.0} s, a tick each</dd></div>\n\
             \x20             <div><dt>Sprites a frame</dt><dd>{sprites}</dd></div>\n\
             \x20             <div><dt>Sprite batches</dt><dd>{batches}</dd></div>\n\
             \x20             <div><dt>Tiles a frame</dt><dd>{tiles}</dd></div>\n\
             \x20             <div><dt>Mean tick</dt><dd>{tick:.3} ms <small>longest {longest:.3}</small></dd></div>\n\
             \x20             <div><dt>World hash</dt><dd class=\"nadir-mono\">{hash}</dd></div>\n\
             \x20           </dl>\n\
             \x20           <p class=\"nadir-scene__note\">Drawn offscreen at {cw} × {ch} by \
             {renderer} on the CI runner, and encoded at twice the size.</p>\n\
             \x20         </figcaption>\n\
             \x20       </figure>\n",
            w = capture.width * 2,
            h = capture.height * 2,
            preload = if index == 0 { "auto" } else { "none" },
            autoplay = if index == 0 { " autoplay" } else { "" },
            title = escape(scene.title),
            caption = escape(scene.caption),
            frames = thousands(u64::from(capture.frames)),
            sprites = thousands(capture.mean_sprites.round() as u64),
            batches = one_place(capture.mean_batches),
            tiles = thousands(capture.mean_tiles.round() as u64),
            tick = capture.mean_tick_ms,
            longest = capture.longest_tick_ms,
            hash = escape(&capture.world_hash),
            cw = capture.width,
            ch = capture.height,
            renderer = escape(&capture.renderer),
            version = escape(version),
        );
    }
    trim_line_end(html)
}

/// Each milestone, with a bar of its ticked tasks.
fn milestones(milestones: &[Milestone]) -> String {
    let mut html = String::new();
    for milestone in milestones {
        let state = if milestone.done == milestone.total {
            "done"
        } else if milestone.done > 0 {
            "active"
        } else {
            "todo"
        };
        let percent = (milestone.done * 100)
            .checked_div(milestone.total)
            .unwrap_or(100);
        let _ = writeln!(
            html,
            "          <li class=\"nadir-milestone\" data-state=\"{state}\"><span \
             class=\"nadir-milestone__id\">{}</span><span class=\"nadir-milestone__name\">{}\
             </span><span class=\"nadir-milestone__count\">{}/{}</span><span \
             class=\"nadir-milestone__bar\" aria-hidden=\"true\"><span style=\"width: \
             {percent}%\"></span></span></li>",
            escape(&milestone.id),
            escape(&milestone.title),
            milestone.done,
            milestone.total,
        );
    }
    trim_line_end(html)
}

/// The download button and what it gets. `version` goes on its address.
fn download(download: &Download, version: &str) -> String {
    let file = escape(&download.file);
    let glibc = download.glibc.as_ref().map_or(String::new(), |glibc| {
        format!(" and glibc {} or newer", escape(glibc))
    });
    format!(
        "        <div class=\"nadir-download\">\n\
         \x20         <a class=\"btn btn--amber btn--block\" href=\"downloads/{file}?v={version}\" \
         download=\"{file}\">\
         <svg class=\"icon\" viewBox=\"0 0 24 24\" width=\"18\" height=\"18\" aria-hidden=\"true\" \
         focusable=\"false\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" \
         stroke-linecap=\"square\"><path d=\"M12 4v11M7 10l5 5 5-5M5 20h14\"/></svg>\
         <span>Download for Linux</span></a>\n\
         \x20         <p class=\"nadir-download__meta\">The sandbox, as this build drew \
         it: x86_64, for X11 with OpenGL 3.3{glibc} &middot; {size} &middot; <span \
         class=\"nadir-mono\">{file}</span></p>\n\
         \x20         <p class=\"nadir-download__meta\">SHA-256 <span class=\"nadir-mono \
         nadir-hash\">{sha}</span></p>\n\
         \x20         <p class=\"nadir-download__meta\">Unpack it and run \
         <code>./sandbox</code>; its README names the scenes and keys. Windows and \
         macOS builds need their windows (roadmap M1.5, M1.6), their OpenGL contexts \
         (M2.6, M2.7) and a build on those systems.</p>\n\
         \x20       </div>",
        size = escape(&megabytes(download.bytes)),
        sha = escape(&download.sha256),
        version = escape(version),
    )
}

/// The status filters, with how many features each shows.
fn feature_filters(all: usize, shipped: usize, in_progress: usize, planned: usize) -> String {
    let filters = [
        ("all", "All", all),
        ("shipped", "Shipped", shipped),
        ("in-progress", "In progress", in_progress),
        ("planned", "Planned", planned),
    ];
    let mut html = String::new();
    for (key, label, count) in filters {
        let active = key == "all";
        let _ = writeln!(
            html,
            "          <button class=\"chip{}\" type=\"button\" data-status-filter=\"{key}\" \
             aria-pressed=\"{active}\">{label} <span class=\"chip__count\">{count}</span></button>",
            if active { " is-active" } else { "" },
        );
    }
    trim_line_end(html)
}

/// A card for each feature.
fn features(features: &[(Feature, Status)]) -> String {
    let mut html = String::new();
    for (feature, status) in features {
        let key = status_key(*status);
        let tasks: Vec<String> = feature
            .tasks
            .iter()
            .map(|id| format!("<li class=\"tag\">{}</li>", escape(id)))
            .collect();
        let _ = write!(
            html,
            "        <article class=\"nadir-feature panel\" data-status=\"{key}\">\n\
             \x20         <p class=\"kicker kicker--sm\">{area} <span class=\"kicker__sep\">//</span> \
             <span class=\"nadir-status nadir-status--{key}\">{status}</span></p>\n\
             \x20         <h3 class=\"nadir-feature__name\">{name}</h3>\n\
             \x20         <p class=\"nadir-feature__summary\">{summary}</p>\n\
             \x20         <ul class=\"tag-list nadir-feature__tasks\" aria-label=\"Roadmap tasks\">{tasks}</ul>\n\
             \x20       </article>\n",
            area = escape(&feature.area),
            status = status.name(),
            name = escape(&feature.name),
            summary = escape(&feature.summary),
            tasks = tasks.join(""),
        );
    }
    trim_line_end(html)
}

/// The status as the page's filters and classes name it.
fn status_key(status: Status) -> &'static str {
    match status {
        Status::Shipped => "shipped",
        Status::InProgress => "in-progress",
        Status::Planned => "planned",
    }
}

/// `text` with the characters HTML gives a meaning replaced.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// `n` with commas between its thousands.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `x` to one decimal place, or none when it is whole.
fn one_place(x: f64) -> String {
    let text = format!("{x:.1}");
    text.strip_suffix(".0").map_or(text.clone(), String::from)
}

/// A size in megabytes, to one decimal place.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// `html` without the line break after its last line.
fn trim_line_end(mut html: String) -> String {
    while html.ends_with('\n') {
        html.pop();
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD: Scene = Scene {
        name: "field",
        title: "The field",
        caption: "A person <walks>.",
        poster_frame: 1,
    };

    fn build() -> Build {
        Build {
            commit: "0123456789abcdef".to_string(),
            date: "2026-10-31".to_string(),
            subject: "M5.2 codec: KLZ & KBJ".to_string(),
            toolchain: "1.99.0".to_string(),
            crates: 18,
            third_party: 0,
            repository: "https://github.com/jabbott-iii/Nadir".to_string(),
        }
    }

    fn capture() -> Capture {
        Capture {
            width: 480,
            height: 270,
            fps: 60,
            frames: 600,
            ticks: 600,
            world_hash: "cba9d2bc0882b423".to_string(),
            mean_sprites: 10_203.0,
            mean_batches: 2.0,
            mean_tiles: 2210.4,
            mean_tick_ms: 0.0041,
            longest_tick_ms: 0.045,
            renderer: "llvmpipe (LLVM 20.1.2, 256 bits)".to_string(),
        }
    }

    fn feature(name: &str, tasks: &[&str]) -> Feature {
        Feature {
            name: name.to_string(),
            area: "Graphics".to_string(),
            summary: "Draws \"things\".".to_string(),
            tasks: tasks.iter().map(|id| (*id).to_string()).collect(),
            ticked: Vec::new(),
        }
    }

    #[test]
    fn placeholders_are_filled_and_an_unknown_or_open_one_fails() {
        let values = BTreeMap::from([("a", "1".to_string()), ("b", "<b>".to_string())]);
        assert_eq!(
            fill("x{{a}}y{{b}}{{a}}", &values),
            Ok("x1y<b>1".to_string())
        );
        assert_eq!(
            fill("no placeholders", &values),
            Ok("no placeholders".to_string())
        );
        assert_eq!(
            fill("{{c}}", &values),
            Err("the template's `{{c}}` has no value".to_string())
        );
        assert_eq!(
            fill("{{a", &values),
            Err("the template has a `{{` with no `}}`".to_string())
        );
    }

    #[test]
    fn text_is_escaped_and_numbers_are_grouped() {
        assert_eq!(escape("a<b>&\"c'"), "a&lt;b&gt;&amp;&quot;c&#39;");
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(10_203), "10,203");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(one_place(17.66), "17.7");
        assert_eq!(one_place(2.0), "2");
        assert_eq!(megabytes(1_260_000), "1.3 MB");
    }

    #[test]
    fn the_template_in_the_repository_fills_with_everything_escaped() {
        let template = include_str!("../../../site/index.html");
        let build = build();
        let recordings = [Recording {
            scene: &FIELD,
            capture: capture(),
        }];
        let download = Download {
            file: "nadir-sandbox-linux-x86_64.tar.gz".to_string(),
            bytes: 1_260_000,
            sha256: "ab".repeat(32),
            glibc: Some("2.34".to_string()),
        };
        let milestones = [
            Milestone {
                id: "M4".to_string(),
                title: "Simulation".to_string(),
                done: 8,
                total: 8,
            },
            Milestone {
                id: "M5".to_string(),
                title: "Content pipeline".to_string(),
                done: 2,
                total: 8,
            },
        ];
        let features = [
            (feature("Tilemaps", &["M4.5"]), Status::Shipped),
            (feature("Pack files", &["M5.7"]), Status::Planned),
        ];
        let page = Page {
            build: &build,
            recordings: &recordings,
            download: &download,
            milestones: &milestones,
            features: &features,
        };
        let html = render(template, &page).expect("every placeholder has a value");
        assert!(!html.contains("{{"), "a placeholder is left");
        // The scenes become tab panels only when the script shows the tabs.
        assert!(!html.contains("tabpanel"));
        for expected in [
            "<a class=\"nadir-mono\" href=\"https://github.com/jabbott-iii/Nadir/commit/\
             0123456789abcdef\">0123456</a>",
            "M5.2 codec: KLZ &amp; KBJ",
            "<dd>10 of 16 tasks</dd>",
            "<dd>None</dd>",
            "A person &lt;walks&gt;.",
            "<dd>10,203</dd>",
            "<dd>2,210</dd>",
            "<dd>0.004 ms <small>longest 0.045</small></dd>",
            "by llvmpipe (LLVM 20.1.2, 256 bits) on the CI runner, and encoded at twice the size",
            "src=\"media/field.webm?v=0123456\" type=\"video/webm\"",
            "poster=\"media/field.png?v=0123456\" preload=\"auto\" muted loop playsinline \
             controls autoplay>",
            "content=\"https://jabbott.dev/Nadir/media/field.png?v=0123456\"",
            "<figure class=\"nadir-scene\" id=\"scene-field\" aria-labelledby=\"title-field\" \
             data-scene=\"field\">",
            "<h2 class=\"nadir-scene__title\" id=\"title-field\">The field</h2>",
            "for X11 with OpenGL 3.3 and glibc 2.34 or newer",
            "data-state=\"active\"",
            "style=\"width: 25%\"",
            "href=\"downloads/nadir-sandbox-linux-x86_64.tar.gz?v=0123456\" \
             download=\"nadir-sandbox-linux-x86_64.tar.gz\">",
            "1.3 MB",
            "Draws &quot;things&quot;.",
            "<span class=\"nadir-status nadir-status--planned\">Planned</span>",
            "data-status-filter=\"shipped\" aria-pressed=\"false\">Shipped <span \
             class=\"chip__count\">1</span>",
            "1 shipped, 0 in progress and 1 planned.",
            "/blob/0123456789abcdef/docs/roadmap.md",
        ] {
            assert!(html.contains(expected), "missing {expected:?}");
        }
    }

    #[test]
    fn a_download_that_names_no_glibc_says_nothing_of_it() {
        let html = download(
            &Download {
                file: "a.tar.gz".to_string(),
                bytes: 1,
                sha256: "0".repeat(64),
                glibc: None,
            },
            "abc",
        );
        assert!(html.contains("for X11 with OpenGL 3.3 &middot;"), "{html}");
        assert!(html.contains("href=\"downloads/a.tar.gz?v=abc\""), "{html}");
    }

    #[test]
    fn a_milestone_with_no_tasks_counts_as_done() {
        let html = milestones(&[Milestone {
            id: "M0".to_string(),
            title: "Empty".to_string(),
            done: 0,
            total: 0,
        }]);
        assert!(html.contains("data-state=\"done\""), "{html}");
        assert!(html.contains("width: 100%"), "{html}");
    }
}
