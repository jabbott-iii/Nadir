---
name: build-language-packs
description: "Translate an app's user-facing text into Mandarin Chinese, Hindi, Spanish, Arabic, French, Bengali, Portuguese, Indonesian, and Urdu, as checked language packs."
argument-hint: "[languages, such as \"ar ur\"; empty means all nine] [path or area of the app]"
disable-model-invocation: true
---

# Build language packs

Requested: `$ARGUMENTS` (empty means all nine languages, for the whole app).

This skill translates the text an application shows its users into nine languages: Mandarin
Chinese, Hindi, Spanish, Standard Arabic, French, Bengali, Portuguese, Indonesian, and Urdu. It
builds a language pack for each one: every user-facing string in the app's own localization
format, translated, with the plural forms, punctuation, script, and text direction the language
needs, and registered so that the app offers the language. Run it again after the source text
changes, and it translates only what was added or changed, keeping the corrections reviewers made.

The translations it writes are drafts. It checks everything a program can check, from
placeholders to plural forms, and gives each language's reviewer a list of the strings that need a
fluent speaker's judgment. It never marks a translation as reviewed.

Bold names are values from the "Project details" of the adopted rules:

- **Where text lives**, **Project name**, and **Names to spell exactly** from `writing.md`;
- **Git access**, **Done checks**, **Never commit**, and **Scopes** from `commits.md`;
- **Languages** from `android.md`, `swift.md`, `web-front-end.md`, `kotlin.md`, `rust.md`, and
  `lua.md`, **Localization** from `flutter.md`, and **Resources** from `kotlin-multiplatform.md`,
  which say which languages the app ships, where its strings live, and who reviews translations;
- **Check commands**, **Dependency policy**, **Design system**, and **Test layout** from the
  language rules (`android.md`, `kotlin-multiplatform.md`, `swift.md`, `flutter.md`, `kotlin.md`,
  `rust.md`, and `lua.md`), and **Build and dependencies**, **Design tokens**, and **Test tools**
  from `web-front-end.md`;
- the **Docs folder** from `roadmap-and-docs.md`, where it is adopted.

Where a value doesn't exist, because a rule isn't adopted or its value still reads
`[FILL IN: …]`, work from what the repository has, and say so in the report. `writing.md` governs
the text in the source language: the source strings, the language sheets, and the report. Each
translation follows its own language's sheet (step 4) instead.

## The nine languages

Start from these tags, and settle each language's variant with the user in step 4. A tag is a
BCP 47 language tag, the standard way to name a language, such as `pt-BR` for Portuguese as
written in Brazil. Each platform spells tags its own way in folder and file names.

| Language | Tag | Script and direction | Plural categories | Numbers by default |
| --- | --- | --- | --- | --- |
| Mandarin Chinese | `zh-Hans` | Simplified Chinese characters, left to right | `other` | `1,234,567.89` |
| Hindi | `hi` | Devanagari, left to right | `one` (0 and 1), `other` | `12,34,567.89` |
| Spanish | `es` | Latin, left to right | `one`, `many`, `other` | `1.234.567,89`, but `1234` with four digits, for `es`; `1,234,567.89` for `es-419` |
| Standard Arabic | `ar` | Arabic, right to left | `zero`, `one`, `two`, `few`, `many`, `other` | `1,234,567.89` for `ar` and `ar-AE`; `١٬٢٣٤٬٥٦٧٫٨٩` for `ar-EG` and `ar-SA` |
| French | `fr` | Latin, left to right | `one` (0 and 1), `many`, `other` | `1 234 567,89`, grouped with a narrow no-break space (U+202F) |
| Bengali | `bn` | Bengali, left to right | `one` (0 and 1), `other` | `১২,৩৪,৫৬৭.৮৯` |
| Portuguese | `pt-BR` | Latin, left to right | `one` (0 and 1 in Brazil, only 1 in Portugal), `many`, `other` | `1.234.567,89` in Brazil; `1 234 567,89` in Portugal |
| Indonesian | `id` | Latin, left to right | `other` | `1.234.567,89` |
| Urdu | `ur` | Arabic, usually in the Nastaliq style, right to left | `one`, `other` | `1,234,567.89` for `ur`; `۱٬۲۳۴٬۵۶۷٫۸۹` for `ur-IN` |

- **Plural categories** are the names that the Unicode Common Locale Data Repository (CLDR) gives
  the forms a phrase takes with a count. A message with a count needs one variant for each of its
  language's categories: six in Arabic, and one in Chinese and Indonesian. In Spanish, French, and
  Portuguese, `many` covers exact millions, which take "de" before the noun:
  `1 000 000 de fichiers`. The counts in parentheses are whole numbers; the platform's plural rules
  place fractions.
- **Numbers** come from the user's locale, through the platform's formatter, never from a
  translation. The last column shows what that formatter produces. Spain's format (`es`) groups
  only numbers of five digits or more, Bengali has its own digits, Hindi and Bengali group digits
  in the Indian style, and Arabic's digits depend on the region.
- ICU (International Components for Unicode, the library that most platforms use for CLDR's data)
  78.3, with CLDR 48, gave these values in November 2026. A platform's own data may be older, so
  check the plural categories it uses, for example in Node.js:

  ```bash
  node -e '
  for (const tag of process.argv.slice(1)) {
    const rules = new Intl.PluralRules(tag);
    console.log(tag, rules.resolvedOptions().pluralCategories.join(" "));
  }' zh-Hans hi es ar fr bn pt-BR id ur
  ```

  ```text
  zh-Hans other
  hi one other
  es one many other
  ar zero one two few many other
  fr one many other
  bn one other
  pt-BR one many other
  id other
  ur one other
  ```

What the tags don't tell you:

- **Chinese:** "Mandarin" names a spoken language. On screen, an app shows written Chinese in one
  of two scripts: Simplified (`zh-Hans`), used in mainland China and Singapore, or Traditional
  (`zh-Hant`), used in Taiwan, Hong Kong, and Macau. ICU rewrites the ISO 639-3 codes for Mandarin
  (`cmn`) and Standard Arabic (`arb`) as `zh` and `ar`, which are the codes platforms expect.
- **Arabic:** write Modern Standard Arabic, the written standard across Arabic-speaking
  countries. The regional dialects differ from it and from each other.
- **Hindi and Urdu:** close as spoken languages, but written in different scripts, with different
  formal vocabularies. Translate each one from the source, and never transliterate one into the
  other.
- **Urdu** is normally printed in Nastaliq, a calligraphic style of the Arabic script in which each
  word slopes down from right to left. It needs a font of its own (step 6).
- **Indonesian:** until Android 15, the constructor of Android's `Locale` converted `id` to the
  obsolete code `in`; apps that target and run on Android 15 or later get `id`. Check which
  folder name the app's Android build reads, on the oldest and the newest Android version it
  supports.

For a language outside the nine, ask the user first, and gather the same facts from the
platform's locale data before you start: its tag, script, direction, plural categories, and
number formats.

## Ground rules

These hold for the whole run, and override anything a later step seems to allow:

- **Every translation you write is a draft.** Only a fluent reviewer, named in the **Languages**
  or **Localization** value or by the user, approves one. Never mark a translation reviewed,
  final, or signed off, and never call one correct because it reads well to you.
- **Say what the source says.** Add no claim, drop no condition, and soften no warning. Where the
  source can be read two ways, find out which meaning it has, from its context or from the user,
  before you translate it.
- **Leave alone what isn't for people:** keys, identifiers, placeholders, markup, URLs, file
  paths, commands, API fields, analytics event names, and log messages that only developers read.
  Keep the **Project name** and the **Names to spell exactly** as they are, unless their owner
  publishes a translated name.
- **Ask before decisions that are the user's.** Stop and ask before you:
  - choose a language's variant or its form of address (step 4);
  - add a localization library or tool, under the **Dependency policy** or
    **Build and dependencies**;
  - add a font, which brings a license and adds to the app's download size;
  - translate legal, medical, financial, or safety text, which needs a professional translator.
    Draft it only if the user asks, and flag every such string for that review;
  - change what the source text says;
  - let unreviewed translations ship where the format would otherwise hold them back (step 8);
  - or translate anything outside the app, such as store listings, a website, or documentation.
- **Keep the source the source.** Change the source catalog only to fix what blocks translation
  (step 3), as a change of its own, reviewed before any translation builds on it.
- **Keep personal data out** of examples, test strings, and screenshots.

## 1. Set the scope

- **Languages:** the arguments name them by English name or by tag (`zh-Hans`, `hi`, `es`, `ar`,
  `fr`, `bn`, `pt-BR`, `id`, and `ur`). Empty, or `all`, means all nine.
- **Part of the app:** a path or an area after the languages, such as one screen or module, limits
  the run to the strings there. Without one, the run covers the whole app.

Run `git status` before you start. If the tree has uncommitted changes you didn't make, leave them
alone and tell the user, because translations mixed into them can't be reviewed or reverted on
their own. Then tell the user in two or three sentences which languages and which part of the app
you will translate, in what order, and make a task list whose last item is "Verify and report".

## 2. Find the text and how the app localizes it

1. Read the **Languages**, **Localization**, or **Resources** value, and the language sheets that
   earlier runs wrote (step 4), if there are any.
2. Find the localization system the app already uses, and its catalogs: the files that hold the
   app's strings, one set for each language. These are the common systems:

   | App | Strings live in | Counts use | Checked by |
   | --- | --- | --- | --- |
   | Android | `res/values*/strings.xml` | `<plurals>` | the build, and Android Lint's translation checks |
   | Compose Multiplatform | `composeResources/values*/strings.xml` | `<plurals>` | the build, on every target |
   | iOS and macOS | String Catalogs (`.xcstrings`), or `.strings` and `.stringsdict` files in older apps | plural variations | the Xcode build |
   | Flutter | Application Resource Bundle (ARB) files, in the folder `l10n.yaml` names | ICU `plural` messages | `flutter gen-l10n` |
   | Web | JSON files for a library such as i18next or FormatJS, or a translated copy of each page | key suffixes such as `_one` and `_other` in i18next, ICU `plural` messages, or `Intl.PluralRules` | the library's own tools |
   | Java Virtual Machine (JVM): Kotlin and Java | `.properties` resource bundles, such as `messages_es.properties` | ICU4J `plural` messages; the JDK's own classes can't express CLDR's rules | the app's tests |
   | gettext (C, Python, PHP, and others) | `.po` files, compiled to `.mo` | `msgstr[0]`, `msgstr[1]`, and so on, picked by the `Plural-Forms` header | `msgfmt --check` |
   | Mozilla's Fluent | `.ftl` files | selectors on CLDR's plural categories | the Fluent parser |
   | Lua, and apps with a system of their own | a table or file for each language | often nothing yet: add plural selection from CLDR's rules | the app's tests |

   An ICU `plural` message looks like `{count, plural, one {# file} other {# files}}`, where `#`
   stands for the number.
3. If the app has no localization system, and its strings are written into the code, stop.
   Moving them into a catalog comes first, as a change of its own that leaves every screen as it
   was. It needs a library or a format, which is the user's choice: propose one that fits the
   platform, and continue once the user approves it and the change has landed.
4. List every string that users see, or hear through a screen reader, starting from the
   user-facing text that **Where text lives** names:
   - labels, buttons, menus, titles, and hint text in fields;
   - messages, errors shown to users, confirmations, and empty states;
   - notifications, and the emails and text messages the app sends;
   - accessibility text: content descriptions, `aria-label` values, alternative text, and
     announcements;
   - text drawn into images, and units written out in words.

   Search the code for strings that escaped the catalog, in the calls that put text on screen.
   Count the strings and words in scope, so the user knows the size of the job.

## 3. Fix what blocks translation

Some source strings can't be translated well as they are. Find them, show them to the user with a
fix for each, and make the approved fixes in the source before you translate anything:

| Problem | Why it fails | Fix |
| --- | --- | --- |
| A sentence built from pieces: `"Deleted " + n + " files"` | Word order differs between languages, and each piece is translated without the others | One message with placeholders |
| A count handled with `n == 1 ? "file" : "files"` | English has two forms; Arabic has six, and Chinese one | A plural message |
| One string used for two meanings, such as "Open" as a button and as a status | Many languages need a different word for each | Two keys |
| A string with no context, such as "Post" or "Free" | A noun or a verb? "No cost" or "available"? | A comment for translators: where the string appears, what each placeholder holds, and any length limit |
| Placeholders with no names or positions: `%s sent %s` | A translation can't reorder them | Positional (`%1$s`) or named placeholders |
| A date, number, amount of money, or list written into the string | Their formats differ by locale | Placeholders that the platform's formatters fill |
| A sentence that changes with a person's grammatical gender | Hindi, Urdu, Arabic, French, Spanish, and Portuguese mark gender in verbs or adjectives | A `select` message, or wording without gender |
| "Tap the button on the right" | Arabic and Urdu mirror the layout | Name the control instead |
| Text inside an image | The catalog can't translate it | Text in the layout, over or beside the image |

Make these fixes as a commit of their own, with every screen looking the same in the source
language, so a reviewer can check them apart from the translations.

## 4. Decide each language's variant and style

Ask in one message, for the languages in scope, with a proposed default for each, so the user can
accept it in a word:

| Language | Variant to choose | Formal and familiar "you" |
| --- | --- | --- |
| Mandarin Chinese | Simplified (`zh-Hans`), Traditional (`zh-Hant`), or both | 您 and 你 |
| Hindi | One pack, `hi` | आप and तुम |
| Spanish | One pack under `es`, in a neutral Spanish, or packs for Spain (`es-ES`) and Latin America (`es-419`) | usted and tú |
| Standard Arabic | How to address a reader whose gender the app doesn't know: gender-neutral wording, such as the verbal noun حفظ ("saving") for **Save**, or masculine forms | No formal and familiar pair; "you" and commands change with the reader's gender instead |
| French | France (`fr`), and whether Canada (`fr-CA`) needs a pack of its own | vous and tu |
| Bengali | One pack, `bn`, for Bangladesh and India (`bn-IN`) | আপনি and তুমি |
| Portuguese | Brazil (`pt-BR`), Portugal (`pt-PT`), or both | você in Brazil; ask the reviewer for Portugal |
| Indonesian | One pack, `id` | Anda and kamu |
| Urdu | Pakistan (`ur`), or India (`ur-IN`) too, which changes the default digits | آپ and تم |

Propose Simplified Chinese, one Spanish pack under `es`, Brazilian Portuguese, and gender-neutral
Arabic. For the form of address, propose the one that matches the source's tone, and the formal
one where the source is neutral. Ask in the same message:

- who reviews each language, if the **Languages** or **Localization** value doesn't say;
- the glossary: the product's own terms, and whether each is translated or kept as it is;
- which strings stay out of the drafts, such as legal text for a professional translator;
- and where the language sheets go.

Then write a **language sheet** for each language: a Markdown file named by its tag, such as
`ar.md`, in a `translations/` folder inside the **Docs folder**, or in `docs/translations/` where
there is none, unless the user picks another place. Each sheet holds:

- the variant, the tags the pack ships under, and the reviewer;
- the form of address, and how the pack handles gender;
- the glossary: each product term with its translation, or "keep as is", and the operating
  system's own words for its features, such as Settings and Share, as the system shows them in
  that language;
- the punctuation and typography the pack follows (step 5);
- the source commit the pack was last brought up to date with, so a later run knows what changed;
- and the review list (step 8).

## 5. Translate

Translate one language at a time, and check it (step 7) before you start the next. For each
string:

1. **Read its context first:** the comment for translators, where the code shows the string (a
   button, a title, an error), and any length limit. Read the strings around it on the same
   screen, so the terms agree.
2. **Translate the meaning, in the sheet's style.** Use the glossary's term every time, the same
   word for the same thing across the app, and the operating system's words for its own features.
3. **Keep every placeholder** exactly as it is written, as many times as the source has it, in
   every plural form, and move it to where the language's word order needs it. Never translate
   the inside of a placeholder, or a message format's keywords (`plural`, `select`, `one`,
   `other`, and `#`).
4. **Give each count its language's plural categories** from the table, no more and no fewer,
   even when two forms read the same. Chinese and Indonesian get only `other`. Keep any exact case
   the source has, such as `=0` for "No files".
5. **Keep markup and escapes valid.** Translate the text inside a tag, but not the tag. Escape
   what the format requires: French apostrophes (`l'application`) need `\'` in Android's
   `strings.xml`, and in ICU message syntax an apostrophe can start quoted text, so check each
   message with the format's own parser (step 7).
6. **Follow the language's typography:**
   - Chinese uses full-width punctuation (`，`, `。`, `：`, and `？`) and no spaces between
     characters. The sheet records whether a space separates Chinese from Latin letters and
     digits.
   - Hindi and Bengali end a sentence with the danda (`।`, U+0964).
   - Arabic uses the Arabic comma (`،`), semicolon (`؛`), and question mark (`؟`).
   - Urdu uses those three, and ends a sentence with the Urdu full stop (`۔`, U+06D4).
   - Spanish opens questions and exclamations with `¿` and `¡`.
   - French, as written in France, puts a no-break space before `:`, `;`, `!`, and `?`, and inside
     guillemets (`« … »`), so a line break never strands the mark. The sheet says whether that
     space is U+00A0 or the narrow U+202F.
   - Spanish, French, and Portuguese capitalize only the first word and proper names in headings
     and labels: don't copy English title case into them.
   - The Chinese, Devanagari, Arabic, and Bengali scripts have no capital letters, so a style that
     relies on capitals for emphasis, such as CSS `text-transform: uppercase`, does nothing in
     them.
7. **Watch the length.** Translations run longer than English, most of all in short strings. The
   W3C article
   [Text size in translation](https://www.w3.org/International/articles/article-text-size.en),
   citing IBM, gives 200–300% of the English length for strings of up to 10 characters, falling to
   about 130% for strings over 70. Chinese often needs fewer characters, but each one takes more
   room, in width and height, than a Latin letter. When a translation is over a length limit,
   shorten its wording without losing meaning. If it still doesn't fit, flag the layout in the
   review list rather than cut meaning.
8. **Flag what you aren't sure of.** Wordplay, slang, idioms, cultural references, and terms
   with no settled translation go into the review list (step 8), with your best draft and the
   reason. Never leave a string in the source language to avoid a hard one, unless the glossary
   keeps it as is.

## 6. Make the app show each language

1. **Register the language** where the platform lists the languages an app supports, as its rule
   describes: Android's locale config (`generateLocaleConfig`), the Xcode project's
   **Localizations** list, Flutter's `supportedLocales` and the Xcode project for its iOS app, or
   the web library's list of locales. Add each pack and its registration in the same change.
2. **Check the fallback:** which pack a user in each nearby locale gets, such as a user in `es-MX`
   with an `es` pack, or in `pt-PT` with only a `pt-BR` pack, and whether a string missing from a
   pack shows in the source language in the middle of a translated screen.
3. **Name each language in itself** in a language picker, such as العربية or বাংলা, from the
   platform's locale data where it has an API for it (`Intl.DisplayNames` in JavaScript, for
   example), so readers find their language without reading the current one.
4. **Format for the user's locale.** Numbers, dates, times, money, units, and lists ("A, B, and
   C") come from the platform's formatters at run time, never from a catalog. Check which locale
   the formatters receive. On some platforms it is the user's own locale, and on others the
   supported locale the app matched it to, so a reader in Mexico who gets the `es` pack may get
   Spain's formats.
5. **Check the fonts.** The fonts in the **Design system** or the **Design tokens** must cover
   Chinese characters, Devanagari, Bengali, and Arabic. A font without them falls back to the
   system's fonts, or shows empty boxes where the letters should be. Many systems' fonts don't
   provide Nastaliq, so ask whether the app should bundle a Nastaliq font for Urdu. Adding a font,
   such as one from Google's Noto family under the SIL Open Font License, is the user's decision,
   and a font for Chinese characters adds megabytes to the app.
6. **Leave room for the script.** Devanagari and Bengali place vowel signs above and below the
   letters, and Nastaliq sets each word on a slanting line, so both need more line height than
   Latin text, and fixed heights clip them. Check with the longest translations.
7. **Lay out Arabic and Urdu right to left:**
   - Mirror the layout with the platform's start and end, as its rule describes: logical
     properties in CSS, `leading` and `trailing` in SwiftUI, the directional classes in Flutter,
     and `start` and `end` on Android.
   - Mirror icons that point a direction, such as back and forward arrows. Leave logos, text, and
     icons that show real objects as they are.
   - Set the direction at the root from the language: on the web, `lang` and `dir="rtl"` on
     `<html>`.
   - Isolate text that may run the other way. A name, an address, a number, or a file name inside
     an Arabic sentence can show in the wrong order with the punctuation around it. Wrap each
     placeholder that may hold such text in two invisible Unicode controls, first strong isolate
     (FSI, U+2068) before it and pop directional isolate (PDI, U+2069) after it, or use the
     platform's tool, such as `<bdi>` in HTML or `BidiFormatter` on Android.
   - Store text in reading order, and let the Unicode bidirectional algorithm display it. Never
     reverse a string by hand.
8. **Test the layout before review** with the pseudolocales the platform rules name, which fake a
   translation: on Android, `en-XA` lengthens and accents the text, and `ar-XB` turns it right to
   left; an Xcode scheme's options offer double-length and right-to-left pseudolanguages.

## 7. Check every pack

Run these checks on each language, and fix what fails before you start the next:

1. **Every file parses:** XML and JSON files, String Catalogs included, with a parser
   (`writing.md` gives the JSON command), and `.po` files with `msgfmt --check`, which also
   catches a translation whose placeholders don't match an entry flagged `c-format`.
2. **The keys match:** every source key is in the pack, the pack has no key the source lacks, and
   no translation is empty.
3. **The placeholders match:** the same placeholders, by name or position and by type, in every
   translation and every plural form.
4. **The plural forms match** the language's categories in the platform's data (see the command
   under "The nine languages"). gettext uses its own scheme instead: each `.po` file's
   `Plural-Forms` header gives the number of forms and the formula that picks one. Check it in
   every file. gettext 0.21's `msginit` wrote it only for Spanish, French, and Portuguese, and left
   it out for the other six languages. These formulas match CLDR's categories for every whole
   number, except for CLDR's `many`:

   | Languages | `Plural-Forms` |
   | --- | --- |
   | Chinese, Indonesian | `nplurals=1; plural=0;` |
   | Hindi, Bengali, French, Brazilian Portuguese | `nplurals=2; plural=(n > 1);` |
   | Spanish, European Portuguese, Urdu | `nplurals=2; plural=(n != 1);` |
   | Arabic | `nplurals=6; plural=(n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5);` |

   None of them has a form for `many`, so in Spanish, French, and Portuguese the exact millions
   take the `other` form, and the reviewer checks how they read.
5. **The messages compile** with the app's own generator or compiler, where it has one, such as
   `flutter gen-l10n`. Read every error and warning it prints.
6. **Each string is in its script:** Chinese characters, Devanagari, Arabic, or Bengali for those
   languages, checked with a pattern such as `/\p{Script=Arabic}/u` in JavaScript. This catches a
   string left in the source language or pasted in from the wrong one. A translation identical to
   its source is a finding, unless the glossary keeps the term as is.
7. **The lengths fit** every limit the comments for translators give.
8. **The app builds and its tests pass:** run the **Check commands** and the **Done checks**. A
   test that compares text with the source language should set its locale, so it doesn't fail on
   a machine set to another language.
9. **Look at every screen in scope** in each language, by running the app or through the
   screenshot or preview tests the **Test layout** or **Test tools** name: at a large text size,
   and right to left for Arabic and Urdu. Look for clipped or overlapping text, empty boxes where
   a font has no letter, layouts that didn't mirror, and text that runs the wrong way.
10. **Translate back.** Translate each draft back into the source language without looking at the
    source, and compare the meanings. Where they differ, fix the draft or flag it. This check
    catches a changed meaning, but not unnatural wording, which only the reviewer can judge.

## 8. Hand off the review

Each language sheet's review list holds:

- the pack's status: "draft, not reviewed", until the reviewer or the user records otherwise;
- every flagged string, with its key, its source, the draft, and the reason: an uncertain
  meaning, legal or safety text, a length problem, a cultural reference, or a term with no settled
  translation;
- and, on a later run, every string added or changed since the last review.

Where a format records a translation's state, give each draft its unreviewed state. In the XML
Localization Interchange File Format (XLIFF), which Xcode and many translation tools exchange,
that is `state="needs-review-translation"` in version 1.2, and `translated` rather than
`reviewed` or `final` in version 2.0. Where a format has no state, such as ARB, the sheet is the
record. Check what a state does when the app runs before you use it: gettext's `msgfmt` leaves
entries marked `#, fuzzy` out of the compiled catalog unless it runs with `--use-fuzzy`, so the
app shows the source text in their place. Whether drafts ship before review is the user's
decision.

When a reviewer corrects a translation, their version wins: keep it on every later run, and add
what it teaches to the glossary. On a later run, translate only the strings added or changed
since the commit the sheet records (`git diff <commit> -- <source catalog>`), add them to the
review list, and update the commit.

## 9. Commit only as the project allows

Follow the **Git access** level in `commits.md`, and leave the changes uncommitted unless the user
asked for a commit and that level allows it. Either way, end the report with copy-ready commit
messages, as `commits.md` describes, in this order, using the **Scopes**:

1. the source fixes from step 3, if any: a `refactor` when no screen changes;
2. the support every language needs, such as fonts, right-to-left layout, and the language
   picker: a `feat`, or a `fix` where it corrects a layout that was already broken;
3. one `feat` for each language pack, with its registration and its sheet in the same commit,
   because the app can't offer the language without them.

Never commit the **Never commit** paths, or build output such as catalogs that the build compiles.

## 10. Report

End with a summary that contains:

- **Languages:** each pack, with its tags, its number of strings, and where its files are.
- **Decisions:** each language's variant, form of address, and glossary choices, and where its
  sheet is.
- **Source changes:** the fixes from step 3, made or waiting for approval.
- **Checks:** each check from step 7, with its real result, and each one you couldn't run, and why.
- **Review:** each language's reviewer, and how many strings its review list flags. Every pack
  stays a draft until its reviewer approves it.
- **Not done:** fonts, legal text, store listings, and anything else waiting for the user, and the
  **Languages** or **Localization** value, if it no longer lists what the app ships, for the user
  to update.
- **Commit messages:** in `text` blocks, as `commits.md` describes, with no attribution lines.
