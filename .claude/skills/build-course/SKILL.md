---
name: build-course
description: "Build a complete course from a short description: syllabus, objectives, Markdown readings, interactive Jupyter labs, LaTeX problem sets, exams, and answer keys."
argument-hint: "[what to teach, to whom, and for how long; or an existing course's code]"
disable-model-invocation: true
---

# Build a course

Requested course: `$ARGUMENTS`

This skill builds a whole course from the description above: the syllabus, the learning
objectives, a reading for each module in Markdown, labs in Jupyter notebooks that students run
and change, problem sets and exams in LaTeX, rubrics, answer keys, and the configuration that
publishes the course as a website. It follows `teaching-materials.md`, which says which format
each piece takes and what shape it has. You design and build; the user decides what is taught,
how it is graded, and what is published.

Bold names are values from the "Project details" of the adopted rules:

- **Materials**, **Audience**, **Course structure**, **Learning objectives**, **Build tool**,
  **Notebook environment**, **LaTeX setup**, **Citation style**, **Notation**, **Assessment**,
  **License**, **Accessibility standard**, **Check commands**, and **Errata** from
  `teaching-materials.md`;
- **Git access**, **Done checks**, **Never commit**, and **Changes that travel together** from
  `commits.md`.

The text follows `writing.md`. If the repository hasn't adopted `teaching-materials.md`, say so
and stop. If `$ARGUMENTS` is empty, ask for the description. If it names a course that already
exists in the **Materials**, continue that course from its plan (see step 2) instead of starting a
new one.

## Ground rules

These hold for the whole run, and override anything a later step seems to allow:

- **The user decides what is taught and how it is graded.** Propose the learning objectives, the
  grading weights, the course policies, and where answer keys go, and build nothing that depends
  on them until the user approves. These are decisions `teaching-materials.md` reserves for the
  user.
- **Never invent a source.** Cite and assign only works you have confirmed exist. Where you
  can't confirm one, write `[citation needed: …]` and list it in the report.
- **Write the material yourself.** Don't copy text, figures, problems, or data from a textbook or
  a website. Use someone else's material only with its license, credited as the license requires.
- **Check every answer.** Work every exercise yourself, run every notebook, and compile every
  document before you call a piece done. An error in a key teaches the error.
- **Keep keys away from students.** Put solutions only where the **Assessment** says, and check
  the built course for leaks before you report it done.
- **Ask before adding a package** to the **Notebook environment** or the **LaTeX setup**, and
  before anything leaves the repository. Never put student data, or personal details such as
  office hours, in the materials.
- **Build in order, and keep the plan current.** A whole course is large. Build it in the order
  of the plan, check each piece before the next, and tick it off in the plan, so that the work
  can stop at any point and a later run can continue it.

## 1. Understand the request

1. Read the **Materials** and any course already there, so the new one matches their patterns.
2. Draw out what the description says, and list what it leaves open:
   - the subject, and what students will be able to do at the end;
   - the students: their level, and what they already know (the **Audience**);
   - the length: weeks, modules, and hours a week (the **Course structure**);
   - how it is delivered: in person, online, or self-paced;
   - the work students do: readings, labs, problem sets, projects, and exams;
   - whether students write code, in which language, and with which tools;
   - any required textbook, standard, or accreditation outcome.
3. Ask, in one message, only the questions whose answers change the design. Propose a default
   for each, so the user can accept it in a word.

## 2. Design the course, and get it approved

Before you write any course material, show the user a blueprint:

1. **The course:** its code, title, and a one-paragraph description.
2. **Prerequisites:** what students must already know, with a short self-check that shows it.
3. **Learning objectives:** about four to eight for the course, and two to five for each module,
   with IDs in the **Learning objectives** format. Write each as an action a grader can observe
   ("compute", "implement", "compare", "justify"), and cover a range of levels, from carrying out
   a method to choosing one and defending the choice.
4. **Modules,** in order: for each, its objectives, the modules it depends on, its hours, and its
   pieces in the format the "Choose the format" table of `teaching-materials.md` gives them. A
   reading is Markdown; a lab is a notebook only when students run code; a problem set with
   substantial math is LaTeX.
5. **Assessment:** each component, its weight, the objectives it covers, and how it is graded:
   automatically with nbgrader, against a key, or with a rubric. Later assessments revisit
   earlier objectives, so practice is spaced and mixed.
6. **Alignment:** a table of objectives against the modules that teach them and the assessments
   that test them. Every objective is taught before it is assessed and assessed at least once,
   and every assessment item tests a listed objective.
7. **Schedule:** the modules and the assessments, week by week.
8. **Environment:** the Python version and packages the notebooks need, and any LaTeX packages
   beyond the preamble, each marked as needing approval.
9. **Sources:** the readings and references you have confirmed, and why each is there.
10. **Policies to decide:** late work, collaboration and academic integrity, generative AI tools,
    and accommodations for disability, each with a draft for the user to change.
11. **Interactivity:** whether readers may run notebook code in the published pages (see step 3),
    which depends on where the course is hosted.

Ask the user to approve the blueprint or change it. Then save it as `PLAN.md` in the course's
folder, with the date and what the user decided, and end it with a build checklist: one line for
each piece, ticked once the piece passes its checks. `PLAN.md` is for the instructor and stays out
of the published course.

To continue an existing course, read its `PLAN.md`, confirm with the user that it still holds,
and build from the first unticked line.

## 3. Lay out the course

Create the course's folder where the **Materials** say, in this shape, adapted to the
**Course structure**:

```text
<course>/
├── README.md             the syllabus: the course hub
├── PLAN.md               the approved blueprint and build checklist, not published
├── notation.md           every symbol, with its LaTeX macro
├── references.bib        the confirmed sources, in the Citation style
├── ERRATA.md             corrections, by term
├── myst.yml              the website's configuration, for Jupyter Book 2
├── modules/
│   └── 01-<slug>/
│       ├── index.md      the reading
│       ├── lab01.py      the lab, paired with lab01.ipynb through Jupytext
│       ├── lab01.ipynb
│       └── practice.md   practice and self-check questions, with visible answers
├── assignments/          what students receive: problem sets and graded notebooks
├── exams/
├── latex/
│   ├── preamble.sty      the shared preamble
│   └── macros.tex        the notation's macros
└── solutions/            answer keys, model answers, and graded notebooks' sources
```

- Put the keys where the **Assessment** says, and change the tree to match if that isn't
  `solutions/`.
- Write the syllabus from the course hub template in `teaching-materials.md`, start the
  **Errata** and `notation.md`, and add the preamble and macros the **LaTeX setup** and the
  **Notation** name.
- Make sure Git ignores the built website (`_build/`) and LaTeX's build files, as the
  **Never commit** says.

### The website

Configure the **Build tool**. For Jupyter Book 2, write `myst.yml` in this shape:

```yaml
version: 1
project:
  title: "<CODE>: <Course title>"
  license:
    content: CC-BY-4.0
    code: MIT
  exclude:
    - "solutions/**"
    - "PLAN.md"
  math:
    '\vect': '\mathbf{#1}'
  toc:
    - file: README.md
    - title: "Module 1: <Title>"
      children:
        - file: modules/01-<slug>/index.md
        - file: modules/01-<slug>/lab01.ipynb
site:
  template: book-theme
```

- Set the licenses from the **License**, and copy each macro in `latex/macros.tex` into `math`,
  so the notation on the website matches the PDFs.
- Jupyter Book builds only the files the `toc` lists, and `exclude` keeps the keys and the plan
  out as well. Leave the paired `.py` files out of the `toc`, which lists the notebooks.
- To let readers run the code in a page, add `jupyter: true` under `project`, which runs it
  through Binder, or `jupyter:` with `lite: true`, which runs it in the browser with JupyterLite.
  The documentation marks both as beta, and the choice depends on hosting, so it is the user's.
- Preview with `jupyter book start`, and build with `jupyter book build --html`, which writes the
  site to `_build/html`. The first build downloads the site template, which took 179 MB in
  `_build/templates` in November 2026.
- If the **Build tool** or the **Check commands** don't name these commands yet, offer to add
  them to the rule's Project details, so later runs and other agents use the same ones.

## 4. Build each module

Build the modules in the plan's order. For each one:

1. **The reading** (`index.md`), in the module shape in `teaching-materials.md`: its objectives by
   ID, its prerequisites, and its time; then the content one idea at a time, each as a concept, a
   worked example, and practice; the common misconceptions, with an example that breaks each one;
   a summary; self-check questions; and further reading from the confirmed sources.
2. **The lab,** when students run code. Write it as a Jupytext `py:percent` file, in the notebook
   structure in `teaching-materials.md`: a title cell with the objectives and the time, a setup
   cell with the imports, the random seed, and the versions, short code cells that each do one
   thing between cells of explanation, exercises that check each answer with `assert`, and a
   summary. Make it interactive: give students values to change and rerun, a prediction to make
   before they run a cell, and a plot to read. Then pair it and create the notebook:

   ```bash
   jupytext --set-formats ipynb,py:percent modules/01-<slug>/lab01.py
   ```

   After any later edit to either file, run `jupytext --sync` on it.
3. **Practice,** in `practice.md`: exercises in the exercise template of `teaching-materials.md`,
   with their IDs, objectives, difficulty, and time. Start with a worked example, then a partly
   worked one, then problems to solve alone. Put hints and self-check answers in `<details>`
   blocks, which GitHub and the website both show collapsed. Number exercises by hand, as the
   template does, so their IDs stay stable.
4. **Figures:** draw them from code kept in the course, with labeled axes and units, a palette
   that readers with color vision deficiencies can tell apart, and alternative text that says
   what each shows.
5. **The rest of the course:** add the module to the syllabus's schedule, its symbols to the
   **Notation**, its sources to `references.bib` in the **Citation style**, and its files to the
   `toc`, as the **Changes that travel together** require.

Run the module's checks from step 6 before you start the next module, and tick its lines in
`PLAN.md`.

## 5. Build the assessments

- **Problem sets and exams with math:** write each in LaTeX with the `exam` class, with every
  question naming its objectives and its points, and every solution in a `solution` environment.
  One source gives both PDFs:

  ```bash
  latexmk -lualatex ps01.tex
  latexmk -lualatex -jobname=ps01-key -usepretex='\PassOptionsToClass{answers}{exam}' ps01.tex
  ```

  The first command builds the student version, and the second the key, with the solutions
  printed. Put the key's PDF only where the **Assessment** says keys go.
- **Graded notebooks:** write the source, with its solutions, where the **Assessment** says.
  Mark each graded cell with nbgrader's metadata, which a `py:percent` file writes on the cell's
  first line:

  ```python
  # %% nbgrader={"grade": false, "solution": true, "locked": false, "grade_id": "ex-1-1", "schema_version": 3}
  def distance(speed, time):
      ### BEGIN SOLUTION
      return speed * time
      ### END SOLUTION

  # %% nbgrader={"grade": true, "solution": false, "locked": true, "points": 2, "grade_id": "ex-1-1-tests", "schema_version": 3}
  assert distance(3, 4) == 12, "3 m/s for 4 s should give 12 m"
  ### BEGIN HIDDEN TESTS
  assert distance(0, 10) == 0, "standing still covers no distance"
  ### END HIDDEN TESTS
  ```

  Generate the student version, and never edit it by hand:
  - In nbgrader's course layout (`source/<assignment>/` and `release/<assignment>/`), run
    `nbgrader generate_assignment <assignment>` from the course's root, with the notebook
    environment activated, because nbgrader runs other commands from it.
  - Without a gradebook, run `jupyter nbconvert <source> --to notebook --output-dir <folder>` with
    `--Exporter.preprocessors` set to nbgrader's `ClearSolutions`, `ClearHiddenTests`, and
    `ClearOutput`.

  Either way, the student version keeps the visible tests, loses the solutions and the hidden
  tests, and has `raise NotImplementedError()` where each answer goes.
- **Everything else:** write a problem set in Markdown or a notebook when it has little math,
  and give every assessment that isn't graded automatically a rubric: its criteria, what each
  level of performance looks like, and the points for each. Publish the rubric's criteria with
  the assignment, and keep its model answers with the keys.
- **Projects:** give each one milestones, deliverables, and a rubric, and revisit objectives from
  several modules.
- **Time:** work each assessment yourself, and keep it well within the time the schedule gives
  students.

## 6. Verify

Check each module as you finish it, and the whole course at the end:

1. **Notebooks:** run every lab and every graded notebook's source from a fresh kernel, for
   example with `pytest --nbmake`, and run `nbgrader validate` on each graded source. Regenerate
   each student version into a temporary folder and `diff` it with the committed one; any
   difference means someone edited it by hand.
2. **LaTeX:** build every document's student version and its key, fix every error and every
   undefined reference or citation, and run `chktex` on each source.
3. **The website:** build it with the **Build tool**, and read it as a student will.
4. **Leaks:** search the built website and every student-facing file for nbgrader's markers
   (`BEGIN SOLUTION`, `BEGIN HIDDEN TESTS`), for a distinctive phrase from each key, and for any
   file from the keys' folder or from `PLAN.md`. Any match fails the course.
5. **Alignment:** check that every exercise and question names objective IDs that exist in the
   syllabus, and that the alignment table in `PLAN.md` still holds.
6. **Answers:** work every exercise again, and compare your answer with the key.
7. **Accessibility,** against the **Accessibility standard**: alternative text on every image,
   headings in order, readable figures, and an accessible HTML or Markdown version of every PDF
   that fails a PDF accessibility checker. The `exam` class doesn't support tagging yet.
8. **Links and sources:** follow every relative link, and confirm every citation; list each one
   you couldn't confirm.
9. **The rules:** do the checks in "Before finishing" in `teaching-materials.md` and in
   `writing.md`, and run the **Check commands** and the **Done checks**.

## 7. Commit only as the project allows

Follow the **Git access** level in `commits.md`, and leave the changes uncommitted unless the user
asked for a commit and that level allows it. Either way, end the report with copy-ready commit
messages, as `commits.md` describes: one for the course's layout and syllabus, then one for each
module, and one for each assessment with its key. Commit a key only where the **Assessment**
allows it, and never commit the **Never commit** paths, such as the built website and PDFs that
the **Build tool** doesn't publish.

## 8. Report

End with a summary that contains:

- **Course:** its code, title, and folder, and the date the user approved its plan.
- **Built:** each module and assessment, with its formats and its line in the checklist.
- **Not built yet:** the unticked lines in `PLAN.md`, which `/build-course <code>` continues.
- **Checks:** each check from step 6, with its real result, and each one you couldn't run, and
  why.
- **Leak check:** what you searched for, and where.
- **Questions:** the policies the user still has to write, the packages waiting for approval, and
  each `[citation needed: …]`.
- **Commit messages:** in `text` blocks, as `commits.md` describes, with no attribution lines.
