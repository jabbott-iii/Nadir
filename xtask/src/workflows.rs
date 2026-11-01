//! `cargo xtask workflows`: the GitHub Actions workflows keep the rules of
//! `docs/04-quality.md` ("Supply-chain hygiene for CI itself") and
//! ADR-0032.
//!
//! In each file of `.github/workflows`:
//!
//! - every action is one the table [`WORKFLOWS`] allows for that file, and
//!   a file with no row fails;
//! - every action is pinned by a full commit SHA, with its tag in a comment
//!   on the same line, as Dependabot keeps it;
//! - the token's default permissions, at the top of the file, are none or
//!   `contents: read`, and a job grants `write` only on the permissions
//!   the table names for that job;
//! - nothing runs on `pull_request_target`, which gives a pull request's
//!   code the base repository's token;
//! - every `actions/checkout` sets `persist-credentials: false`, so the
//!   token is not left in `.git/config` for later steps.
//!
//! It reads the files line by line, not as YAML: the workflows are written
//! in block style, one key a line. A `uses:` written another way is not
//! seen, and permissions written in flow style (`{ pages: write }`) fail.

use std::fs;
use std::path::Path;

use crate::check::Report;
use crate::error::Error;

/// Where the workflows are, relative to the workspace root.
const DIR: &str = ".github/workflows";

/// What one workflow may use.
struct Workflow {
    /// Its file name in [`DIR`].
    file: &'static str,
    /// The actions it may use, as `owner/repo` or `owner/repo/path`.
    actions: &'static [&'static str],
    /// The jobs that may grant `write`, each with the permissions it may.
    writes: &'static [(&'static str, &'static [&'static str])],
}

const CHECKOUT: &str = "actions/checkout";
const CACHE: &str = "actions/cache";

/// Every workflow, and what it may use. A new workflow adds its row here,
/// as the docs say it may.
const WORKFLOWS: &[Workflow] = &[
    Workflow {
        file: "ci.yml",
        actions: &[
            CHECKOUT,
            CACHE,
            "actions/upload-artifact",
            "actions/download-artifact",
        ],
        writes: &[],
    },
    // ADR-0032.
    Workflow {
        file: "codeql.yml",
        actions: &[
            CHECKOUT,
            "github/codeql-action/init",
            "github/codeql-action/analyze",
        ],
        writes: &[("analyze", &["security-events"])],
    },
    // ADR-0029.
    Workflow {
        file: "pages.yml",
        actions: &[
            CHECKOUT,
            CACHE,
            "actions/upload-pages-artifact",
            "actions/deploy-pages",
        ],
        writes: &[("deploy", &["pages", "id-token"])],
    },
    // ADR-0036.
    Workflow {
        file: "release.yml",
        actions: &[
            CHECKOUT,
            "actions/upload-artifact",
            "actions/download-artifact",
        ],
        writes: &[("release", &["contents"]), ("image", &["packages"])],
    },
];

/// Runs the check on the workspace at `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let dir = root.join(DIR);
    let read_error = |source| Error::Read {
        path: dir.clone(),
        source,
    };
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).map_err(read_error)? {
        let path = entry.map_err(read_error)?.path();
        let is_workflow = path
            .extension()
            .is_some_and(|extension| extension == "yml" || extension == "yaml");
        if !is_workflow {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        match fs::read_to_string(&path) {
            Ok(text) => files.push((name, text)),
            Err(source) => return Err(Error::Read { path, source }),
        }
    }
    files.sort();
    let mut report = Report::default();
    for (name, text) in &files {
        check(name, text, &mut report);
    }
    report
        .notes
        .push(format!("workflows checked: {}", files.len()));
    Ok(report)
}

/// Checks the workflow `name`, whose text is `text`.
fn check(name: &str, text: &str, report: &mut Report) {
    let Some(workflow) = WORKFLOWS.iter().find(|workflow| workflow.file == name) else {
        report.violations.push(format!(
            "{DIR}/{name}: no row in the table of what each workflow may use \
             (`WORKFLOWS` in xtask/src/workflows.rs)"
        ));
        return;
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut at = |line: usize, problem: String| {
        report
            .violations
            .push(format!("{DIR}/{name}:{}: {problem}", line + 1));
    };

    let mut defaults = None;
    // Whether the lines are under `jobs:`, and the job they are in.
    let (mut in_jobs, mut job) = (false, None);
    // The indentation of the keys of the `permissions:` block they are in.
    let mut block: Option<usize> = None;
    for (index, line) in lines.iter().enumerate() {
        let code = without_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        let (indent, key) = item(code);
        if block.is_some_and(|keys| indent < keys) {
            block = None;
        }
        if indent == 0 {
            in_jobs = code.starts_with("jobs:");
            job = None;
        } else if in_jobs && indent == 2 {
            job = key.strip_suffix(':');
        }

        if code.contains("pull_request_target") {
            at(
                index,
                "`pull_request_target` runs with the base repository's token: use `pull_request`"
                    .to_string(),
            );
        }

        if let Some(value) = key.strip_prefix("permissions:") {
            if indent == 0 {
                defaults = Some(index);
                for problem in default_permissions(&lines, index) {
                    at(index, problem);
                }
            }
            match value.trim() {
                "" => block = Some(indent + 1),
                "{}" => {}
                value if indent > 0 => at(
                    index,
                    format!(
                        "`permissions: {value}`: name each permission on a line of its own, \
                         as this check reads them"
                    ),
                ),
                _ => {}
            }
            continue;
        }

        // A permission of a job. The defaults are checked above.
        if let (Some(_), Some(job)) = (block, job) {
            if let Some((permission, level)) = key.split_once(':')
                && level.trim() == "write"
            {
                let permission = permission.trim();
                let allowed = workflow
                    .writes
                    .iter()
                    .any(|(name, writes)| *name == job && writes.contains(&permission));
                if !allowed {
                    at(
                        index,
                        format!("the job `{job}` may not grant `{permission}: write`"),
                    );
                }
            }
            continue;
        }

        let Some(reference) = key.strip_prefix("uses:") else {
            continue;
        };
        let reference = reference.trim().trim_matches(|c| c == '"' || c == '\'');
        if reference.starts_with("./") {
            continue;
        }
        let Some((action, pin)) = reference.split_once('@') else {
            at(
                index,
                format!("`{reference}` is not `owner/repo@<commit SHA>`"),
            );
            continue;
        };
        if !workflow.actions.contains(&action) {
            at(
                index,
                format!("`{action}` is not an action this workflow may use (docs/04-quality.md)"),
            );
        }
        if !(pin.len() == 40
            && pin
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
        {
            at(
                index,
                format!("`{action}@{pin}` is not pinned by a full commit SHA"),
            );
        } else if !tag_comment(line) {
            at(
                index,
                format!("`{action}` has no comment naming its tag, such as `# v1.2.3`"),
            );
        }
        if action == CHECKOUT && !keeps_no_credentials(&lines, index) {
            at(
                index,
                "`actions/checkout` does not set `persist-credentials: false`".to_string(),
            );
        }
    }
    if defaults.is_none() {
        report.violations.push(format!(
            "{DIR}/{name}: no `permissions:` at the top, so the token gets the repository's \
             default permissions"
        ));
    }
}

/// What is wrong with the top-level `permissions:` at `lines[index]`: it
/// must be `{}`, or a block that grants `contents: read` and nothing else.
fn default_permissions(lines: &[&str], index: usize) -> Vec<String> {
    let value = without_comment(lines[index])
        .trim_start_matches("permissions:")
        .trim();
    match value {
        "{}" => return Vec::new(),
        "" => {}
        other => {
            return vec![format!(
                "the default permissions are `{other}`: make them `{{}}` or `contents: read`"
            )];
        }
    }
    let mut problems = Vec::new();
    for line in &lines[index + 1..] {
        let code = without_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        if !code.starts_with(' ') {
            break;
        }
        if code.trim() != "contents: read" {
            problems.push(format!(
                "the default permissions grant `{}`: grant it in the job that needs it",
                code.trim()
            ));
        }
    }
    problems
}

/// Whether the `uses:` line has a comment naming a tag, such as `# v7.0.1`.
fn tag_comment(line: &str) -> bool {
    line.split_once(" #").is_some_and(|(_, comment)| {
        let mut chars = comment.trim_start().chars();
        chars.next() == Some('v') && chars.next().is_some_and(|c| c.is_ascii_digit())
    })
}

/// A line's key, without the dash that starts a list item, and the column
/// the key starts at: 8 for `        uses: x`, `      - uses: x` and
/// `    -   uses: x`.
fn item(line: &str) -> (usize, &str) {
    let text = line.trim_start();
    let key = text
        .strip_prefix('-')
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        .map_or(text, str::trim_start);
    (line.len() - key.len(), key)
}

/// Whether a line starts an item of a list.
fn is_item(line: &str) -> bool {
    let text = line.trim_start();
    text.strip_prefix('-')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// Whether the step whose `uses:` is at `lines[index]` sets
/// `persist-credentials: false`. The step's keys start at the column its
/// `uses:` does, and it ends at the first line indented less, which is the
/// next item of its list or what follows the list.
fn keeps_no_credentials(lines: &[&str], index: usize) -> bool {
    let (column, _) = item(without_comment(lines[index]));
    // The line that starts the step, which may be a line or two above
    // `uses:`.
    let start = (0..=index)
        .rev()
        .find(|&at| {
            let code = without_comment(lines[at]);
            is_item(code) && item(code).0 == column
        })
        .unwrap_or(index);
    for line in &lines[start + 1..] {
        let code = without_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        if code.len() - code.trim_start().len() < column {
            break;
        }
        if code.trim() == "persist-credentials: false" {
            return true;
        }
    }
    false
}

/// `line` without a comment: from a `#` at its start or after a space. A
/// `#` inside a quoted string after a space is taken as a comment too; the
/// workflows have none.
fn without_comment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    line.split_once(" #")
        .map_or(line, |(code, _)| code)
        .trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";

    fn violations(name: &str, text: &str) -> Vec<String> {
        let mut report = Report::default();
        check(name, text, &mut report);
        report.violations
    }

    fn ci(steps: &str) -> String {
        format!(
            "name: CI\non:\n  push:\npermissions:\n  contents: read\njobs:\n  ci:\n    \
             runs-on: ubuntu-latest\n    steps:\n{steps}"
        )
    }

    fn checkout() -> String {
        format!(
            "      - name: Check out\n        uses: actions/checkout@{SHA} # v7.0.1\n        \
             with:\n          persist-credentials: false\n"
        )
    }

    #[test]
    fn the_repository_s_workflows_keep_the_rules() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let report = run(&root).expect("the workflows are readable");
        assert_eq!(report.violations, Vec::<String>::new());
        // Every row of the table is a workflow that exists.
        for workflow in WORKFLOWS {
            assert!(
                root.join(DIR).join(workflow.file).is_file(),
                "{} has a row but no file",
                workflow.file
            );
        }
    }

    #[test]
    fn a_pinned_allowed_action_with_its_tag_passes() {
        assert_eq!(violations("ci.yml", &ci(&checkout())), Vec::<String>::new());
        let cache = format!("      - uses: \"actions/cache@{SHA}\" # v6.1.0\n        with:\n");
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + &cache))),
            Vec::<String>::new()
        );
        // A local action is part of the repository.
        let local = "      - uses: ./.github/actions/setup\n";
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + local))),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_tag_or_short_pin_fails() {
        let tag = "      - uses: actions/cache@v6\n";
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + tag))),
            ["\
.github/workflows/ci.yml:14: `actions/cache@v6` is not pinned by a full commit SHA"]
        );
        let short = "      - uses: actions/cache@3d3c42e # v6.1.0\n";
        assert_eq!(violations("ci.yml", &ci(&(checkout() + short))).len(), 1);
        let upper = format!(
            "      - uses: actions/cache@{} # v6.1.0\n",
            SHA.to_uppercase()
        );
        assert_eq!(violations("ci.yml", &ci(&(checkout() + &upper))).len(), 1);
        let bare = "      - uses: actions/cache\n";
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + bare))),
            [".github/workflows/ci.yml:14: `actions/cache` is not `owner/repo@<commit SHA>`"]
        );
    }

    #[test]
    fn a_pin_without_its_tag_fails() {
        let untagged = format!("      - uses: actions/cache@{SHA}\n");
        let commented = format!("      - uses: actions/cache@{SHA} # the cache\n");
        for steps in [untagged, commented] {
            assert_eq!(
                violations("ci.yml", &ci(&(checkout() + &steps))),
                [
                    ".github/workflows/ci.yml:14: `actions/cache` has no comment naming its tag, \
                  such as `# v1.2.3`"
                ]
            );
        }
    }

    #[test]
    fn an_action_the_workflow_may_not_use_fails() {
        let other = format!("      - uses: some/action@{SHA} # v1.0.0\n");
        assert_eq!(
            violations("ci.yml", &ci(&(checkout() + &other))),
            [
                ".github/workflows/ci.yml:14: `some/action` is not an action this workflow may \
              use (docs/04-quality.md)"
            ]
        );
        // The Pages actions are the demo page's alone.
        let deploy = format!("      - uses: actions/deploy-pages@{SHA} # v5.0.1\n");
        assert_eq!(violations("ci.yml", &ci(&(checkout() + &deploy))).len(), 1);
        assert_eq!(
            violations("pages.yml", &ci(&(checkout() + &deploy))),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_workflow_with_no_row_fails() {
        assert_eq!(
            violations("deploy.yml", &ci(&checkout())),
            [
                ".github/workflows/deploy.yml: no row in the table of what each workflow may use \
              (`WORKFLOWS` in xtask/src/workflows.rs)"
            ]
        );
    }

    #[test]
    fn only_the_release_s_publishing_jobs_write() {
        let job = |name: &str, permission: &str| {
            ci(&checkout())
                + &format!(
                    "  {name}:\n    runs-on: ubuntu-latest\n    permissions:\n      \
                     {permission}: write\n    steps:\n      - run: true\n"
                )
        };
        assert_eq!(
            violations("release.yml", &job("release", "contents")),
            Vec::<String>::new()
        );
        assert_eq!(
            violations("release.yml", &job("image", "packages")),
            Vec::<String>::new()
        );
        for (name, permission) in [
            ("build", "contents"),
            ("crates", "contents"),
            ("release", "packages"),
            ("image", "contents"),
            ("crates", "id-token"),
        ] {
            assert_eq!(
                violations("release.yml", &job(name, permission)),
                [format!(
                    ".github/workflows/release.yml:17: the job `{name}` may not grant \
                     `{permission}: write`"
                )]
            );
        }
        // Nor may it use the cache, which a release does not trust.
        let cache = format!("      - uses: actions/cache@{SHA} # v6.1.0\n");
        assert_eq!(
            violations("release.yml", &ci(&(checkout() + &cache))).len(),
            1
        );
    }

    #[test]
    fn a_checkout_that_keeps_its_credentials_fails() {
        let kept =
            format!("      - name: Check out\n        uses: actions/checkout@{SHA} # v7.0.1\n");
        assert_eq!(
            violations("ci.yml", &ci(&kept)),
            [
                ".github/workflows/ci.yml:11: `actions/checkout` does not set \
              `persist-credentials: false`"
            ]
        );
        // The setting in the next step does not count for this one.
        let next = format!(
            "{kept}      - name: Other\n        with:\n          persist-credentials: false\n"
        );
        assert_eq!(violations("ci.yml", &ci(&next)).len(), 1);
        // A `true` does not count, and nor does a comment.
        let true_ = format!("{kept}        with:\n          persist-credentials: true\n");
        assert_eq!(violations("ci.yml", &ci(&true_)).len(), 1);
        let comment = format!("{kept}        # persist-credentials: false\n");
        assert_eq!(violations("ci.yml", &ci(&comment)).len(), 1);
        // `with:` above `uses:` counts.
        let above = format!(
            "      - with:\n          persist-credentials: false\n        uses: \
             actions/checkout@{SHA} # v7.0.1\n"
        );
        assert_eq!(violations("ci.yml", &ci(&above)), Vec::<String>::new());
    }

    #[test]
    fn the_default_permissions_are_none_or_reading_the_contents() {
        let none = ci(&checkout()).replace("permissions:\n  contents: read\n", "permissions: {}\n");
        assert_eq!(violations("ci.yml", &none), Vec::<String>::new());
        let missing = ci(&checkout()).replace("permissions:\n  contents: read\n", "");
        assert_eq!(
            violations("ci.yml", &missing),
            [
                ".github/workflows/ci.yml: no `permissions:` at the top, so the token gets the \
              repository's default permissions"
            ]
        );
        let all = ci(&checkout()).replace(
            "permissions:\n  contents: read\n",
            "permissions: read-all\n",
        );
        assert_eq!(
            violations("ci.yml", &all),
            [
                ".github/workflows/ci.yml:4: the default permissions are `read-all`: make them \
              `{}` or `contents: read`"
            ]
        );
        let more = ci(&checkout()).replace(
            "permissions:\n  contents: read\n",
            "permissions:\n  contents: read\n  actions: read\n",
        );
        assert_eq!(
            violations("ci.yml", &more),
            [
                ".github/workflows/ci.yml:4: the default permissions grant `actions: read`: grant \
              it in the job that needs it"
            ]
        );
    }

    #[test]
    fn a_job_writes_only_what_the_table_names_for_it() {
        let job = |name: &str, permission: &str| {
            format!(
                "  {name}:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: read\n      \
                 {permission}: write\n    steps:\n      - run: true\n"
            )
        };
        let scan = ci(&checkout()) + &job("analyze", "security-events");
        assert_eq!(violations("codeql.yml", &scan), Vec::<String>::new());
        assert_eq!(
            violations("ci.yml", &scan),
            [
                ".github/workflows/ci.yml:18: the job `analyze` may not grant `security-events: write`"
            ]
        );
        // The job, not the workflow: the demo page's build may not write
        // where its deploy may.
        let deploy = ci(&checkout()) + &job("deploy", "pages");
        assert_eq!(violations("pages.yml", &deploy), Vec::<String>::new());
        let build = ci(&checkout()) + &job("build", "pages");
        assert_eq!(
            violations("pages.yml", &build),
            [".github/workflows/pages.yml:18: the job `build` may not grant `pages: write`"]
        );
        // Any permission, even one GitHub adds later.
        let new = ci(&checkout()) + &job("ci", "code-quality");
        assert_eq!(violations("ci.yml", &new).len(), 1);
        // Written all at once, a job's permissions cannot be read.
        for value in ["write-all", "read-all", "{ pages: write }"] {
            let inline = ci(&checkout())
                + &format!("  ci:\n    permissions: {value}\n    steps:\n      - run: true\n");
            assert_eq!(violations("ci.yml", &inline).len(), 1, "{value}");
        }
        let none = ci(&checkout()) + "  ci:\n    permissions: {}\n";
        assert_eq!(violations("ci.yml", &none), Vec::<String>::new());
        // A key that is not in a `permissions:` block may say `write`.
        let input = ci(&checkout()) + "        with:\n          mode: write\n";
        assert_eq!(violations("ci.yml", &input), Vec::<String>::new());
        // A block ends where the job's next key starts.
        let after = ci(&checkout())
            + "  ci:\n    permissions:\n      contents: read\n    env:\n      mode: write\n";
        assert_eq!(violations("ci.yml", &after), Vec::<String>::new());
    }

    #[test]
    fn list_items_are_read_however_they_are_spaced() {
        let wide = "      -   uses: some/action@v1\n";
        assert_eq!(violations("ci.yml", &ci(&(checkout() + wide))).len(), 2);
        assert_eq!(item("      -   uses: x"), (10, "uses: x"));
        assert_eq!(item("      - uses: x"), (8, "uses: x"));
        assert_eq!(item("        uses: x"), (8, "uses: x"));
        assert_eq!(item("        -x: y"), (8, "-x: y"));
        // A checkout written so keeps its setting.
        let checkout = format!(
            "      -   name: Check out\n          uses: actions/checkout@{SHA} # v7.0.1\n          \
             with:\n            persist-credentials: false\n      -   run: true\n"
        );
        assert_eq!(violations("ci.yml", &ci(&checkout)), Vec::<String>::new());
        let kept =
            format!("      -   uses: actions/checkout@{SHA} # v7.0.1\n      -   run: true\n");
        assert_eq!(violations("ci.yml", &ci(&kept)).len(), 1);
    }

    #[test]
    fn pull_request_target_fails() {
        let text = ci(&checkout()).replace("on:\n  push:\n", "on:\n  pull_request_target:\n");
        assert_eq!(
            violations("ci.yml", &text),
            [
                ".github/workflows/ci.yml:3: `pull_request_target` runs with the base repository's \
              token: use `pull_request`"
            ]
        );
        // In a comment, it is only words.
        let comment = ci(&checkout()).replace("on:\n", "on:\n  # never pull_request_target\n");
        assert_eq!(violations("ci.yml", &comment), Vec::<String>::new());
    }

    #[test]
    fn comments_are_left_out() {
        assert_eq!(without_comment("  # uses: x@y"), "");
        assert_eq!(without_comment("  key: value # note"), "  key: value");
        assert_eq!(without_comment("  key: a#b"), "  key: a#b");
        assert!(tag_comment("uses: a/b@c # v1.2.3"));
        assert!(tag_comment("uses: a/b@c   #  v12"));
        assert!(!tag_comment("uses: a/b@c # version 1"));
        assert!(!tag_comment("uses: a/b@c"));
    }
}
