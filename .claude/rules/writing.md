# Writing standards

These rules cover everything written for people to read in this repository: documentation,
READMEs, code comments and doc comments, user-facing text in code (errors, log messages, help
text, and interface copy), free text in data and config files, commit messages, and pull request
descriptions. **Where text lives**, under "Project details" below, lists the paths for this
repository.

Apply them to text you write or edit. Don't rewrite existing files only to make them conform; fix
a passage when you are already changing it. The one possible exception is British spelling, which
**Existing British spellings** settles.

## Project details

A repository fills in this section when it adopts the rule, and the rest of the rule refers to
each value by its bold name. If a value still reads `[FILL IN: …]`, the rule isn't set up here
yet. Work out that value from the repository itself, and tell the user which values are missing
so they can fill them in.

- **Where text lives:** `docs/`, `README.md` and each crate's `README.md`, `AGENTS.md`,
  `CLAUDE.md`, `CONTRIBUTING.md`, `SECURITY.md`, `diagrams/`, the templates in `.github/`, the
  demo page in `site/`, the feature summaries in `docs/features.jsonc`, the rules and skills in
  `.claude/`, and in code: doc comments and comments, log and error messages, the output of the
  cook and `xtask`, and the sandbox's menu text.
- **Project name:** Nadir. Its facade crate is `nadir_engine`, which games use as `nadir` in code.
- **Names to spell exactly:** LDtk, Aseprite, OpenGL, Metal, Win32, Cocoa, AppKit, X11, Wayland,
  XWayland, EGL, ALSA, PipeWire, WASAPI, CoreAudio, XInput, evdev, Mesa, llvmpipe, Perfetto,
  CodeQL, jabbott.dev, docs.rs, and the formats PNG, QOI, QOA, WAV, BMFont, and JSONC.
- **Specialty:** game engines: real-time graphics through OpenGL and Metal, OS windows and input,
  audio mixing, entity component systems (ECS), and binary file formats.
- **Code block tags:** `rust`, `toml`, `bash`, `text`, `jsonc`, `json`, and `markdown`, and
  `mermaid` in `diagrams/`.
- **Sources of truth:** the code, the real output of the `cargo xtask` commands, the roadmap's
  ticks and notes in `docs/roadmap.md`, `docs/decisions.md`, and the specs in
  `docs/subsystems/`.
- **Preview command:** `cargo doc --no-deps --all-features` for doc comments, and
  `cargo xtask site` for the demo page; otherwise, preview the changed Markdown.
- **Commit rule:** `commits.md` in this folder.
- **Existing British spellings:** "report": fix them only in text you are changing, and mention
  the rest in your summary (`AGENTS.md`, rule 6: the smallest complete change).
- **Formal documents:** `LICENSE`, `NOTICE`, and the license header at the top of each source
  file, which stay exactly as their licenses give them.

## English: spelling and word choice

- Use American English, never British English.
  - Spelling: -or not -our (behavior, color), -ize and -ization not -ise and -isation (optimize,
    normalization), -yze not -yse (analyze), -er not -re (center), "license" for both noun and
    verb, a single "l" before suffixes (canceled, labeled, modeling, signaled), "gray",
    "catalog", "judgment".
  - Words: "toward", "among", "while", "learned", and "percent", not "towards", "amongst",
    "whilst", "learnt", or "per cent".
  - For British spellings in text you aren't otherwise changing, follow **Existing British
    spellings**. Either way, leave identifiers, language keywords (CSS `forwards`), quoted
    names, and URLs exactly as they are.
- Spell names the way their owners do: GitHub, GitHub Actions, JavaScript, TypeScript, Node.js,
  npm, Rust, Cargo, crates.io, Kotlin, Gradle, Android, iOS, macOS, Linux, Windows, PostgreSQL,
  SQLite, Docker, Kubernetes, Cloudflare, LinkedIn. Spell the **Project name** and the **Names to
  spell exactly** as the project details give them.
- Watch these common errors: its/it's, their/there/they're, affect/effect, then/than, lose/loose,
  lead/led, principal/principle, complement/compliment, ensure/insure.
- Use the one-word form for a noun and the two-word form for a verb: "the setup" but "set up the
  server". The same goes for login/log in, backup/back up, checkout/check out, shutdown/shut
  down, and rollout/roll out.
- Hyphenate a compound modifier before a noun ("a zero-dependency engine", "an open-source tool")
  but not after it ("the engine has zero dependencies", "the tool is open source"). Don't
  hyphenate after an "-ly" adverb ("a fully signed binary").
- Use "front end" and "back end" as nouns and "front-end" and "back-end" as adjectives.
- Prefer plain words: "use" (not "utilize" or "leverage"), "to" (not "in order to"), "because"
  (not "due to the fact that"), "for example" (not "e.g."), "that is" (not "i.e."), "about" (not
  "approximately").
- Cut filler and hedges: "basically", "simply", "just", "very", "really", "actually", "quite".
  Don't call something "easy" or "simple"; the reader decides that.
- Don't use marketing language without evidence: "blazing fast", "seamless", "robust",
  "cutting-edge", "revolutionary". State the measurement instead.
- Use inclusive, literal terms: allowlist/denylist, primary/replica, placeholder (not "dummy"),
  "quick check" (not "sanity check"), "they" for a person whose pronouns you don't know.

## English: grammar and punctuation

- Write in the active voice and present tense: "The build script writes the archive", not "The
  archive will be written by the build script". Use the passive only when the actor is unknown or
  doesn't matter.
- Keep one idea per sentence, and split sentences longer than about 25 words.
- Use the serial (Oxford) comma: "Windows, macOS, and Linux". It removes ambiguity in long
  technical lists. Existing text may omit it; don't change it only to add one.
- Keep lists parallel: every item starts with the same part of speech. Items that are full
  sentences end with a period; fragments don't. Don't mix the two in one list.
- Make every pronoun's referent clear. Write "This change breaks the cache", not "This breaks the
  cache", when the previous sentence holds more than one noun.
- Dashes:
  - Use an em dash (—) with no spaces for a break in a sentence.
  - Use an en dash (–) for ranges: "2–4 MB", "Monday–Friday".
  - Use a hyphen (-) only inside compound words.
  - Use a colon, not a spaced hyphen, between a title and a subtitle: "Version 2.0: What
    Changed".
- In American prose, periods and commas go inside closing quotation marks. When the exact
  characters matter (a command, a value, a key), use code formatting so the punctuation stays
  outside it.
- Contractions such as "it's", "don't", and "isn't" are fine, except in **Formal documents**.
  Avoid awkward ones such as "it'll" or "should've".
- Capitalization:
  - Titles of whole documents, such as a post title or a release name: title case ("Deep Dive:
    Architecture of a Zero-Dependency 2D Engine").
  - Section headings: sentence case ("What ships today").
  - Text a design shows in capitals: write it in normal case and let styling (CSS
    `text-transform`, for example) do the uppercasing. Screen readers may spell out text typed in
    capitals letter by letter.

## English: numbers, dates, and units

- In prose, spell out zero through nine and use numerals from 10 up. Always use numerals for
  versions, measurements, percentages, sizes, counts in tables, and anything technical.
- Put a space between a number and its unit: "16 MB", "60 Hz", "4 ms", "500 ms". Don't add a
  space before "%" or "°". Use the standard symbols: ns, µs, ms, s, Hz, KB, MB, GB, fps, px.
- Use thousands separators in prose ("10,000 records") but never in versions, years, ports, or
  code.
- Write versions with a "v" when they name a release ("v1.2.0"), and match the project's own
  tags.
- Dates in prose: "November 7, 2026". Dates in file names, metadata, data files, and
  machine-readable attributes: ISO 8601 ("2026-11-07"), with a time zone offset when there is a
  time.

## Technical writing

### Audience and readability

The audience ranges from college students and junior engineers to senior engineers. Write so a
student can follow every sentence and a senior engineer finds nothing vague or wrong. Make the
text as readable as possible without losing any of its meaning.

- Assume the reader can program and has used Git and a terminal, but doesn't know this project or
  its **Specialty**. Don't explain programming basics such as variables, loops, or functions.
- Keep jargon to a minimum:
  - Use a plain phrase when it says the same thing just as exactly: "runs at the same time", not
    "is concurrent", unless the distinction matters.
  - Use a technical term only when it carries meaning that a plain phrase can't, or when the
    reader needs the term to search for it, read the docs, or use the tool.
- Explain each technical term a student might not know the first time it appears. A short clause
  or a few words in parentheses are usually enough: "a spatial hash (a grid that sorts objects by
  position, so nearby objects can be found quickly)". After that, use the term without explaining
  it again.
- Spell out every acronym on first use, followed by the acronym in parentheses: "entity component
  system (ECS)". Skip this only for universal ones: HTML, CSS, API, URL, CLI, OS, CPU, GPU.
- Never trade accuracy for simplicity:
  - Keep the exact number, the real limit, the correct term, and every condition or caveat that
    changes the meaning.
  - If a simpler sentence would be wrong or misleading, keep the precise wording and add an
    explanation next to it.
  - Don't remove details a senior engineer needs; explain them so everyone else can follow too.
- Explain why, not only what. When describing a design choice, say what problem it solves or what
  it trades away, so a reader new to the field can follow the reasoning.
- Avoid insider shorthand:
  - Slang: "footgun", "yak shave", "bikeshed", "dogfood", "LGTM".
  - Internal nicknames and private abbreviations.
  - Bare references to records or tasks. When citing "ADR-0011" or "M2.6", say in the same
    sentence what it decided or what it is.
- Introduce one new idea at a time, building on what has already been explained. Give abstract
  ideas a concrete example. Use an analogy only if it is accurate, and add it next to the real
  explanation rather than in place of it.
- Don't make readers feel behind: leave out "obviously", "of course", "trivially", "as you know",
  and "it's well known that".
- Keep paragraphs short (about three to five sentences) and about one topic. Write headings that
  let a reader skim the page and find the part they need.

### Content

- Lead with the point. The first sentence of a document or section says what changed or what the
  reader gets; background comes after.
- Be specific and checkable: version numbers, commit hashes, links to the release, the pull
  request, or the architecture decision record (ADR), and numbers with units. Say what hardware
  and OS a measurement came from.
- Keep shipped and planned work apart. Say plainly what works today and what doesn't yet, and
  never describe a planned feature in the present tense.
- Write instructions in the imperative, one action per numbered step, with the expected result
  after the action. Name interface elements in **bold**, exactly as they appear on screen.
- Use code formatting (backticks) for file names, paths, commands, flags, environment variables,
  config keys, package, function, and type names, and literal values. Don't use it for product
  names or for emphasis.
- Put code in fenced blocks with a language tag from **Code block tags**. Leave out shell prompts
  (`$`), and keep command output in a separate block.
- Use descriptive link text: "the [v1.2.0 release notes](…)", never "click here" or a bare
  "here". Link the first mention of something, not every mention.
- Use tables to compare three or more attributes across items, with a header row and short cells.
  Use lists for sequences and sets; use prose for reasoning.
- Write alt text that says what an image shows and why it matters, without "image of" or
  "screenshot of". Give purely decorative images `alt=""`.

### Markdown structure

- Give each document one `#` title, start its sections at `##`, and don't skip levels. When the
  renderer prints the title itself (for example, a site layout that renders the front matter
  `title`), start the document's own headings at `##`.
- Put a blank line before every list, table, and fenced block; some Markdown renderers need it.
  Escape `|` inside table cells.
- Write summaries that other tools reuse (a front matter `description`, a package or repository
  description) as one or two sentences of plain text with no Markdown, 160 characters or fewer.
  Cards, search snippets, and AI summaries show them on their own, so make them stand alone.
- Don't rename or move a published document, or change any value its URL is built from: links to
  it break.

### Text in structured files

- In YAML, put free-text values in double quotes: summaries, descriptions, titles, and anything
  else written as a sentence. Unquoted, a colon followed by a space (`IDE: the`) starts a new key
  and breaks the file. A space followed by `#` starts a comment and silently cuts the value off.
- Inside double quotes, escape `"` as `\"` and `\` as `\\`.
- Also quote YAML values that must stay text but look like a number or a yes/no value. Under YAML
  1.1, which Ruby's parser and PyYAML follow, an unquoted `1.10` becomes the number 1.1, and
  `yes`, `no`, `on`, and `off` become true or false.
- A parse error in a config or data file can stop a whole build, so check every file you changed
  (see "Before finishing").

### Commit messages and pull requests

Follow the **Commit rule**. If it is "none", write the summary line in the imperative mood ("add",
"fix", "remove") in 72 characters or fewer, explain what changed and why in the body, and apply
this rule's spelling and grammar.

## Before finishing

1. Read the rendered result, not only the source. Run the **Preview command** and read each
   changed page; if it is "none", preview the changed Markdown.
2. If you changed a YAML, JSON, or TOML file, or any front matter, check that it still parses.
   This command checks YAML files and the front matter of other files, and prints `YAML OK` or the
   first file that fails, with its line and column. It needs PyYAML (`python3 -m pip install
   pyyaml`). Replace the example paths with the files you changed:

   ```bash
   python3 - config.yml docs/page.md <<'EOF'
   import sys, yaml
   for f in sys.argv[1:]:
       t = open(f, encoding="utf-8").read()
       if not f.endswith((".yml", ".yaml")):
           if not t.startswith("---"):
               continue
           t = t.split("\n---", 1)[0]
       try:
           yaml.safe_load(t)
       except yaml.YAMLError as e:
           sys.exit(f"{f}: {e}")
   print("YAML OK")
   EOF
   ```

   For JSON, run `python3 -m json.tool path/to/file.json > /dev/null`. For TOML, run
   `python3 -c 'import sys, tomllib; tomllib.load(open(sys.argv[1], "rb"))' path/to/file.toml`
   (Python 3.11 or later). Both print nothing when the file parses.
3. Proofread changed prose for spelling, names, version numbers, and dates, and confirm every
   claim against the **Sources of truth**.
4. Do the two-reader check. Reread the draft as a second-year computer science student: is any
   term undefined, or any step skipped? Then reread it as a senior engineer: is anything vague,
   oversimplified, or wrong? Fix both.
