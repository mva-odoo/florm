# Florm guide

This guide opens with **Help → Florm guide** (or the palette: “guide”).
Every shortcut can be changed in **Preferences → Shortcuts** (Ctrl+,).

---

## Find and open

| Shortcut | Action |
|---|---|
| **Ctrl+P** | Go to a file of the project. `name:42` opens it at line 42, `:42` goes to line 42 of the current file |
| **Ctrl+K** | The views apart from the commands: Search, Terminal, Claude, Git…, the terminals and the conversations, then the Ctrl+K… chords with their name. Type to filter; **Ctrl+K Ctrl+B**, **Ctrl+K V**… keep their role |
| **Ctrl+Shift+P** | Command palette. A leading **`#`** lists the views, the terminals and the conversations: `#git`, `#dock`, `#term`, `#claude` |
| **Alt** alone | Everything from the keyboard: one or two letters on each visible button, tab, box or field; typing them clicks it (Esc closes). In an open menu, ↓ ↑ move between its entries and Enter chooses. Can be turned off in Preferences → Editor |
| **Ctrl+R** | Go to a class, a function, an Odoo record of the file |
| **Ctrl+O** / **Ctrl+Shift+S** | Open / save as: **Tab completes the path** like in a terminal |
| **F12** | Go to definition (also on `'sale.order'` or `'module.xml_id'`) |
| **Ctrl+.** | Quick fixes for the problem under the cursor (also the light bulb at the end of the line): import a name of the project, ignore (`# noqa`), `ruff --fix`, fix with Claude, and the fixes of the language server in every language (import a name in Go, Rust, TypeScript…), at the top of the list; the cursor can be anywhere on the line |
| **Alt+Z** | Word wrap of the lines that are too long (display only; ↑ ↓ follow the rows) |

The breadcrumbs above the code are clickable: a folder shows its content, the file lists its
symbols, a class or a function takes you there. The views (Claude, Git, Pull requests (GitHub),
terminal…) have none: they keep the whole height.

Two open files of the same name: their folder shows in grey after the title (`input.rs …/app`). A file with errors shows how many in red (`9+` beyond nine).

On the right of the tab strip: **+** opens an empty text tab (Ctrl+N), **▾** a view in a tab, each with its icon: Search, Claude, Git, Pull requests (GitHub), Debug, then Terminal, Preferences and Welcome. When the tabs no longer fit in the strip, ‹ › scroll them and the same **▾** lists all the open tabs at the top.

**Groups of tabs**: right click a tab → **Add to a group** (an existing one or **+ New
group**). The tabs of a group stay side by side, with a band of its color and its name before
them: a click on the name folds the group (only its name stays, with the number of tabs), right
click to rename it, close its tabs or dissolve it. In a pull request, **📑 Open the files** opens
them in the group “PR #…”. The groups are kept with the session.

On restart (or back to a project), everything comes back as you left it: the tabs and the views,
the cursor position, the split editor with its tabs on each side, the open bottom panel and the
sidebar shown or not.

## Sidebar

- At the top: the name of the project (a click to switch project) and four small buttons, new
  file, new folder, **fold everything** and ⟳ refresh.
- **Ctrl+Shift+E** (or a click on a file): the keyboard goes to the tree. **↑ ↓** to move,
  **→** opens a folder, **←** closes it (or goes up to the parent folder), **Space** shows the
  file keeping the keyboard in the tree, **Enter** opens it and goes to the editor, **F2**
  renames, **Delete** deletes, **Escape** goes back to the editor.
- While scrolling, the folders holding the top of the list stay pinned there; a click goes back
  up to the folder.
- **Double-click the edge** between the sidebar and the editor: the width fits the longest name of
  the tree. Drag the edge to change the width; **Ctrl+B** (or Ctrl+K Ctrl+B) hides the sidebar, and a click on
  the thin strip on the left shows it again.
- On the folder of a git repository inside the open folder (the open folder's own is in the status bar): its branch, the tracked remote branch (↑ to push, ↓ to
  pull) and the number of changed files; hovering shows the remotes
  (can be turned off in Preferences → Git).
- The sidebar only shows the tree: the views (Search, Terminal, Claude, Git…) are in the bottom
  panel and the status bar (see below).
- **Copy / paste files**: right click → **Copy** puts the file or folder on the system clipboard; **Paste** (or Ctrl+C / Ctrl+V in the tree) drops the files copied from your PC's file manager, or from the tree. A name already taken becomes “name (copy).ext”: nothing is replaced.
- Right click on a folder outside any repository → **Initialize a git repository here…**, for a
  new project: a window asks for the **remote** to add (“origin”, empty: none), the branch
  (`main`), then **add everything** (`git add .`) and make the first commit, or an **empty
  commit** without any file, or nothing. With a remote, the commit is pushed (`git push -u`). If a
  step fails, the window stays open with the error (the steps before stay done).
- **Versions side by side** (git worktrees, as for Odoo: `odoo/master` holds the repository,
  `odoo/17.0`, `odoo/18.0`… are its worktrees, same for `enterprise/` and `design-themes/`): right
  click on `odoo/`, on one of its versions or on the folder above → **Add a version
  (worktrees)…** (also in the Git menu). Type the branch (`20.0`, `saas-18.3`…, the known branches
  are suggested; ⟳ runs `git fetch --all` to see the recent ones) and tick the repositories:
  each one gets its folder (`odoo/20.0`, `enterprise/20.0`…) on that branch, tracking the remote
  branch. A branch no remote knows yet is fetched first. If a repository fails, the others are still
  done and the window shows what went wrong.
- **The Versions window** (click **Version 20.0** in the status bar, right click on `odoo/` →
  **Versions side by side…**, or the Git menu) shows everything at once: a row per version
  (`master`, then the newest at the top), a column per repository. Each cell says whether the
  folder is **✔ up to date**, behind (**↓3** commits to pull, since the last fetch), ahead (**↑1**
  to push), with **● changed files**, or on another branch (its name in grey); **—**: the version
  is missing in this repository, **+** adds it there. For each version: **Work on it** (see
  below), **Update** (`git pull --ff-only` in each repository, never a merge commit), and in
  **⋯**: a terminal in one of its folders, add it where it is missing, or **remove** it (`git
  worktree remove`: the folders go, the branches stay; with changed files, Florm says so and asks
  to confirm). **⟳ Check the server** runs `git fetch --all` to count the ↓ again; **+ New
  version…** opens the window to add one.
- In the tree, each version folder that is a worktree carries a small **WT** (not `master`, which
  is the repository itself), and the version you work on is marked **● current**, in each
  repository. Right click on a version → **● Work on version 20.0** (or **Stop filtering the
  versions** on the current one).
- **Current version**: with versions side by side in the opened folder, the status bar shows
  **Version 20.0** (or **Versions: all**). In the Versions window, **Work on it** chooses the version you work on (**Stop filtering**
  puts them all back): only that one is
  in the index (go to file, F12, completion, search), in every repository at once (`odoo/20.0`,
  `enterprise/20.0`…). The others stay in the tree, dimmed, and still open. The choice is kept for
  the folder; **Exclude from the index** works as before, separately. To find a file in another
  version, tick **+ other versions** at the top of go to file (Ctrl+P) or next to the filters of the
  search (unticked whenever the version changes).

## Bottom panel and status bar

One simple rule: **files and the views that need room (Claude, Git, Pull requests (GitHub)) go in
the tabs; the bottom panel keeps what comes along with the work**: terminal, problems, output, and
also the search and the debugger if you want. To see two tabs side by side, split the editor.

- **The bottom panel shows one view at a time**, without a row of tabs: its name at the top left,
  **▾** to switch to another (with its shortcut, and what is waiting: number of problems, ⏸ when
  the debugger is stopped); under “Terminal” the open terminals. **Ctrl+Shift+J** shows or hides it.
- **⇱ / ⇲ in the header of the search and the debugger** puts them in a tab or in the bottom
  panel, and Florm remembers: their shortcut, the palette, the status bar and ▾ then open them in
  the same place. A view that already has its tab is shown there, never in two places.
- **The status bar only shows what is going on**: on the left the icon of the **Preferences**, then those of **Claude**, **Git**,
  **Pull requests** (a click opens its tab, another one goes back to the tab before; coloured when
  it is shown) and **Terminal** (the terminal of the bottom panel); on the right the branch (**↑** commits to
  push, **↓** to pull, **●** changed files) of the repository of the file shown (it follows the
  tabs; a repository chosen in the Git view stays until the next file shown), then, only when it is the case, Claude (**working**,
  or **✔** when it has finished: a click shows the answer), the
  debugger (**running** / **paused**), and the problems. At rest, it is almost empty.
- The Preferences: **Ctrl+,**, the menu or the palette.

## Find in folder (Ctrl+Shift+F)

Enter runs the search. The results are colored like in the editor; **F4 / Shift+F4** go to the
next / previous result. the **in a tab** button puts the search in an editor
tab (more room), **at the bottom** puts it back at the bottom
(Preferences → Claude to choose where it opens).

### Filter the files

Several rules are separated by commas; a `!` in front of a rule excludes it. The name of the
open project can go in front: in the `florm` project, `florm/test` is its `test` folder.

| “Files” filter | Searches in… |
|---|---|
| `*.py` | all the Python files |
| `*.py, *.xml` | the Python and XML files |
| `models/*.py` | the `.py` files of any `models` folder (wherever it is) and of its subfolders |
| `tests` | any folder (or file) named `tests` |
| `./src/*.rs` | from the root of the project only |
| `views/**/*.xml` | the `.xml` files under any `views` folder |
| `*.py, !tests` | the `.py` files, except those of the `tests` folders |
| `florm/*.py, !florm/test` | the `.py` files of the `florm` project, except its `test` folder |

| “Exclude” filter | Ignores… |
|---|---|
| `node_modules, *.min.js` | these folders and files |
| `tests, i18n` | the test and translation folders |
| `addons/sale/tests` | this folder only |
| `./test` (or `florm/test` in the `florm` project) | the `test` folder at the root |

Both fields combine: **Files** `*.py` + **Exclude** `test` searches in the `.py` files of the
project, except the `test` folder.

The `.gitignore` is respected.

### Text, regex, replacement

- **Aa** matches case, **« »** searches the whole word, **.*** turns on regular expressions:
  `def _compute_\w+` finds all the compute methods, `^\s*import ` the imports.
- With ⏵, the replacement shows; in regex `$1`, `$2` take back the groups:
  `fields\.(\w+)\(` → `Fields.$1(`.
- **New line** in the replacement: **Shift+Enter** in the field, or `\n` in regex
  (`\t` tab, `\\` backslash). To turn the `\n` written in the text into real new lines: regex,
  find `\\n`, replace with `\n`. Also in Ctrl+H.
- **Keep case**: cat → dog, Cat → Dog, CAT → DOG.
- **×** on a result (or a file) drops it: it will not be replaced.
- **TODO / FIXME** (on the right of the bar, or Tools → TODO / FIXME of the project): all the
  `TODO`, `FIXME`, `XXX` and `HACK` of the project, by file.

### Compare, local history

- **Compare two files or two folders**: right click the first one in the sidebar → **⇄ Select …
  to compare**, then right click the other → **⇄ Compare with “…”**.
- **Compare with the clipboard** and **Compare with…** (another file, or a folder): right click
  the tab, or the Tools menu. The text of the editor counts, even unsaved.
- The result opens in a tab: in red what is on the left, in green what is on the right.
- **Local history** (right click the tab, or Tools): Florm keeps a copy of the file at each save,
  outside git (the last 50). **Compare** a version with the current text, or **Restore** it in
  the editor (Ctrl+Z to come back).

## Git

Florm goes through the `git` command (your configuration, your credentials). Without `git`, the view
explains how to install it (the system is detected) and comes back by itself as soon as it is found.

- **The changes in the editor**: a colored mark in the gutter shows each block changed since the
  last commit. **A click on the mark** (or **Ctrl+K Ctrl+D** on the line) shows that change
  alone, in the text: the removed lines in red above, the new ones in green, and a small bar
  (previous, next, ↺ revert, All, ×). Another click, Escape or Ctrl+K Ctrl+D hides it.
  **Alt+F5 / Alt+Shift+F5** go to the next / previous change and show it; **Ctrl+K Ctrl+Z**
  reverts the one of the line. **Ctrl+Alt+D** (or the “Diff in the editor” preference) shows
  all of them all the time.
- **Ctrl+Shift+G** (or click the branch in the status bar): Git view. At the top the repository (large, a click lists them when the project has several), the branch (**🔀 name ▾**: switch or create one),
  pull / push / fetch and the commit message (Ctrl+Enter); below it the graph, whose first
  rows are the changes to stage / staged, with their files one per row (as in Atom): the state
  letter (**M** modified, **A** added, **D** deleted, **U** new), the name, the folder in grey and
  the +/− lines; on hover, **+** stages the file, **−** unstages it. Under the message, the length of
  its first line (orange past 50, red past 72).
  A click on a file opens its changes in the commits view.
- **Ctrl+Alt+G**: the commits view. At the top the list (with the unstaged and staged
  changes), below it **all the files of the commit and their changes, one under the other**,
  like a GitHub pull request.
  - **↑ ↓**: previous / next commit · **← →**: previous / next file ·
    **Page ↑ / Page ↓**: scroll the changes
  - click the header of a file: fold it; the chips above take you to a file
  - on the changes: **+ Stage this block**, **↺ Discard this block**, and per file
    **+ Stage**, **− Unstage**, **↺ Discard**
  - double-click a line: open the file at this line
- Changes made elsewhere (a commit in a terminal…) are picked up by themselves.
- **⋯ → Stash** in the bar: put the changes aside (with a message, new files included or not),
  then for each stash **Show**, **Apply** (it stays), **Pop** (applied then deleted) or
  **Delete**. Also in the palette: “Git: put the changes aside”, “Git: take back the last stash”.
- **🔀 branch ▾**: also **merge a branch** into the current one, **rebase** the current branch onto
  another, **rename** a branch (its remote branch of the same name follows), **delete** a local
  branch (and its remote branch if you choose so) or a remote one, and **amend the last commit**.
- **Amend the last commit** (box under the message, or branch menu): the message of the last
  commit comes back in the field; the button replaces it (with the staged files added). If it
  was already pushed, a push --force-with-lease will be needed.
- **↕ Sync**: Pull, **Pull --rebase** (your commits replayed on top, no merge commit), Push,
  **Push --force-with-lease** (after a rebase or an amend: refused if someone pushed meanwhile),
  **Push the tags**, Fetch. When a push is refused, the error message also offers
  **⬆ Force (--force-with-lease)**.
- Right click on a commit of the graph: besides checkout, cherry-pick, revert…
  - **Create a tag here** (“v1.2”, or “v1.2 message” for an annotated tag); on a tag:
    **Push the tag**, **Delete the tag** (locally, or on the remote too)
  - on each branch of the commit: **Merge** into the current branch, **Rename**, **Delete**;
    **Merge this commit**
  - **Move the branch here (reset)**: soft (changes kept, staged), mixed (kept, unstaged) or
    hard (everything thrown away, asks for confirmation)
  - **Bisect**: mark a bad commit and a good one; a banner then offers **✔ Good**, **× Bad**,
    **Skip** on each commit git checks out, down to the first bad one, then **Finish**.
- **⋯** in the bar:
  - **Search the history**: by message, author, text added or removed (`-S`), regex in the
    changed lines (`-G`) or file (through its renames). Tab in the question changes the
    criterion. The commits found are shown with their diff.
  - **Reflog**: where HEAD has been (commits, checkouts, resets, rebases). A lost commit is still
    there: **Bring back ▾** creates a branch on it, cherry-picks it or moves the branch there.
  - **Remotes**: add (“name URL”) or remove a remote. **Submodules**: init and update them,
    follow their remote branch, take the URLs again, add one.
  - **Delete the untracked files** (`git clean`): the list is shown first.
  - **✨ Commit message rules**: opens `.florm/commit.md`, what Claude must follow for ✨ and ✂ in
    this project (language, prefixes, length…). Commit it to share it. Without this file, Claude
    takes the “Commits” section of the project's CLAUDE.md if there is one.
  - **✨ My commit message rules**: yours, for every repository without its own (`commit.md` of
    the config folder): nothing to add to a shared repository. For instance: in English,
    `[FIX] module: …`, the why in the body, `task-…` at the end.
  - Both also open with a **right click on ✨ or ✂**, next to the commit message.
- **File history** (right click in the editor, Git menu): all its commits, through its renames.
  **Line history** (Ctrl+Alt+B) also takes several selected lines; each commit shows the whole
  block that changed around these lines, with some context. The history opens in a tab (↑ / ↓ to
  go from one commit to the next, Escape to come back).
- **Side-by-side diffs**: the **◫ Side by side** button, at the bottom right of every diff (Git,
  history, Claude's review, pull requests), puts the old version on the left and the new one on
  the right, each replaced line facing its new version, with the changed words darker.
  **≡ One column** goes back to the usual display. The choice applies to every diff and is kept.

### Resolve conflicts (rebase, merge, cherry-pick)

When a rebase, a merge, a cherry-pick or a revert stops on conflicts, the **⇄ Conflicts** tab
opens (or **⇄ Conflicts assistant** in the banner of the Git view).

- At the top: what is going on (“Rebase of feature onto master — commit 3 of 7”), the commit
  being applied, and **the real name of each side**. In a rebase, side 1 is the base your
  commits are replayed onto (what git calls “ours”), side 2 your commit.
- On the left, the conflicting files: blocks left, or “ready to mark”.
- On the right, each block with its two sides next to each other, and the original version
  (from before both) to unfold: **Keep side 1**, **Keep side 2**, **Keep both**, or **Edit by
  hand…**. **All from side 1 / 2** takes the whole file from one side. A file removed on one
  side is kept or removed.
- **✔ Mark resolved** (git add) once no marker is left.
- **✳ Resolve with Claude**: a conversation gets the files, both sides and what the commit
  changes in them; Claude combines them and removes the markers, without touching git. You
  check in its review, then mark resolved.
- **Continue the rebase ▶** goes to the next commit (the assistant comes back if there are
  other conflicts), **Skip this commit** or **Abort**.

The operations started from Florm put the original version between the two sides
(`merge.conflictStyle=diff3`).

## Terminal

- **Ctrl+`**: the terminal in the bottom panel.
- **Ctrl+Shift+`**: a terminal **in a
  tab** of the editor. Like any tab, it can be dragged to the right or bottom edge of the editor
  to be next to a file (or right click on the tab → Split right / down).
- In the bottom panel, **one terminal at a time**: its folder at the top, **+** opens another,
  **⇱** sends it to a tab (**at the bottom** in the tab brings it back). The other terminals are in
  **▾**, under “Terminal”: click to switch, **×** on hover to close it.
- All the views (Search, Git, Pull requests (GitHub), Claude, Debug, Terminal) have the same buttons at
  the right of their header, in the same order: **⟳** read again, **in a tab / at the
  bottom** (a small window: with a tab on top, or a band at the bottom; remembered for next time), **⋯** the other actions
  and options. The layout buttons all look alike: the four corners maximize a panel in its view
  (Escape to come back), a window with a column on the right shows or hides a side panel (the
  review of Claude), filled when it is shown. Git and the Preferences are
  tabs like the others: Escape goes back to the previous tab, Ctrl+W closes them.

## Errors in the code

Problems are underlined in the code and listed in **Problems** (bottom panel):

- **Python**: the first linter installed (`flake8`, else `ruff`, `pyflakes`, `pylint`; Preferences →
  Lint), while typing. **JavaScript**: `eslint`, on save.
- **Language servers** (Preferences → Editor → Language servers): their errors too, for every
  language they know (TypeScript, Rust, Go, C…). **Format the file** (palette) goes through `ruff`
  for Python, through the language server for the others. To format every Python file on save:
  Preferences → Editor.
- **XML and JSON**: a badly formed file (unclosed or badly nested tag, missing quote), live.

## Tests (pytest, jest, cargo, go)

The tests of any project run on this computer, with nothing to set:

- **pytest** for `test_*.py` and `*_test.py` (the `pytest` of the project's `.venv` if there is one),
  **jest** (or **vitest**, if `package.json` asks for it) for `*.test.js`, `*.spec.ts`…,
  **cargo test** for the `#[test]` of a Rust file, **go test** for `*_test.go`.
- The **▶** in the gutter of a test runs it; **Ctrl+Shift+F6** runs the test under the cursor,
  **Ctrl+F6** the last one again. The command is shown as it is (`pytest tests/test_a.py::TestX::test_y`,
  `cargo test app::calc::tests::adds`…): in “Tests: run tests…”, it can be changed before Enter.
  A project in a sub-folder starts with `cd sub-folder && `.
- The output shows in the bottom panel, the summary (✔ or ⚠, passed and failed) in the status bar.

## Odoo tests

An Odoo test is debugged with **Alt+F6** (cursor in the test): it runs under the debugger, on this
computer with the `odoo-bin` of the folder or in the container of the debugger. To simply run them,
a terminal is enough (`odoo --test-enable --test-tags …`); **Ctrl+click** on a `File "…", line N`
line there opens the file.

## Debugger

Like VS Code's, for Python (and Odoo, also in a Docker container), Node, Rust, C and Go. **Debug**
in the palette, or **▾ → Debug** in the bottom panel. During a session, the status bar shows
**running** or **paused**: a click shows the view.

- **Node, Rust, C, Go**: Florm recognizes the project (`package.json`, `Cargo.toml`, `go.mod`) and
  runs on this computer `npm start` (or the `main` of `package.json`), `cargo run`, `go run .`;
  **Edit…** changes the command (`node app.js --port 3000`, `cargo run -- --verbose`,
  `go run ./cmd/app`). The debugger of each:
  - **Node**: js-debug, VS Code's, downloaded once (1 MB) and checked;
  - **Rust and C**: `lldb-dap`, else `gdb` (version 14 and up). Florm builds first (`cargo build`);
  - **Go**: delve (`dlv`), also looked for in `~/go/bin`.
  When it is missing, the view says so, with an **Install** button.
- **Breakpoint**: click left of the line numbers (or **F9**). A red dot. In the
  **Breakpoints** list, a **condition** (`self.id == 7`) only stops when it is true. They follow
  the lines when the file changes and are kept with the project.
- **F5** starts. At the top of the view: **Where** (this computer or a container), **Launch** or
  **Attach**, and the command.
  - **Launch** here: the command (by default the `odoo-bin` of the folder, `--workers 0`) runs in
    the bottom panel, with its logs. `debugpy` is needed in this Python: otherwise, **Install
    debugpy**.
  - **Launch** in a container: the command runs in the container, next to the Odoo running there
    (by default `odoo --workers 0 --http-port 8070`). Florm talks to the debugger through
    `docker exec`: **no port to publish**. The mounted files open here; those of the image (the
    code of Odoo installed in the container) open read only.
  - **Attach**: to an Odoo started with debugpy in front of the usual command, with
    `--workers 0` (otherwise the requests go through other processes, which the debugger does not
    follow): `python3 -m debugpy --listen 5678 ./odoo-bin --addons-path=… -d my_db --workers 0`.
    In a container: `python3 -m debugpy --listen 127.0.0.1:5678 /usr/bin/odoo … --workers 0` in the
    `command:` of `docker-compose.yml`, debugpy installed in the image, and **Where** set to that
    container.
    **It is all automatic, whatever the mode**: as soon as a breakpoint is set, Florm waits for a
    program listening with debugpy and attaches to it, without F5 nor anything to choose; when the
    program ends, it waits for the next run. Start Odoo
    as you like (Florm's terminal, yours, `docker compose up`): **Copy the command** gives the line
    to run. **■ Stop waiting** (or Shift+F5) stops waiting, F5 waits again.
- **Paused**: the line is highlighted in yellow, the window comes to the front and the debugger opens in the bottom panel, under the file (unless it is in the other half of a split editor). On the left the
  **Variables** (click to unfold `self`, a recordset…; its fields first, the `__x__` and the
  groups of debugpy at the end), the **Watch** (expressions evaluated again
  at each stop), the **Call stack** (click on a call: its line and its variables, in green).
  Hovering a name shows its value (`self.partner_id.name`).
- **Console** (on the right): an expression evaluated in the paused frame (in Python:
  `self.env['res.partner'].search([])`, `x = 2`). ↑ ↓ bring back the previous ones.
- **F10** next line, **F11** step into the call, **Shift+F11** finish the function, **F5**
  continue, **F6** pause, **Shift+F5** stop, **Ctrl+Shift+F5** restart.
- **Tests**: right click on the **▶** of a test in the gutter (or **Alt+F6**) runs it under the
  debugger: an Odoo test in the chosen container (else the tests' one), a pytest, jest,
  `cargo test` or `go test` one on this computer.
- **Exceptions**: stop on those nothing catches, on all of them (Odoo raises many), or when one
  leaves the code of the project. **My code only** does not step into the code of Odoo nor of the
  libraries.

## Templates

**Ctrl+Alt+N** (or Tools → Insert a template): the templates of the type of the current file
come first. Florm ships none: add your own (see below).

The variables are filled from the file: in `estate/views/estate_property_views.xml`,
`${model}` = `estate.property`, `${model_}` = `estate_property`, `${Model}` = `EstateProperty`,
`${name}` = `Estate Property`, `${module}` = `estate`. `$0` marks the place of the cursor.

To add yours: **Tools → Templates folder**, one file per template
(its name = the name of the template, its extension = the type of file). A file with the name
of a built-in template replaces it, with “·” in place of “:” (refused in a file name on
Windows): `Python · pdb.py` replaces “Python : pdb”.

**Triggers**, like Sublime's snippets: typing `ipdb` in a Python file suggests it,
**Tab** (or Enter) replaces it with `import ipdb; ipdb.set_trace()`. Also `pdb`. A template of
your own whose name is a single word (`log.py`, `ifmain.py`) has that word as its trigger.

**Preferences → Templates**: the list of yours and of the built-in ones; a click opens one to
change its name (one word = its trigger), the files it is offered for and its text.
**+ New template** creates one; a changed built-in one becomes yours and replaces it.

**Tools → New template…** (or the palette): type the trigger (`log`), Florm creates `log.py`
(extension of the current file; `log.xml` for another type) in the templates folder and opens
it. If there was a selection, it becomes the text of the template.

## GitHub

Florm goes through the `gh` command: no token to create, you only need to have logged in once
with `gh auth login` (login through the browser). Without `gh`, the view explains how to install
it (your system is detected) and comes back by itself as soon as it is found.

### Pull requests

**Ctrl+Alt+R** (or Git → GitHub pull requests, **⋯ → GitHub pull requests** in the Git view, the
palette).

- Each row of the list: the author's initials (the dot on them says whether the pull request is
  open, a draft, merged or closed), the title on two lines at most, `#number · branch · age`, and on
  the right the state of the CI (**✔** passed, **✖** a failure, **●** running) then of the review
  (**👍** approved, **✖** to fix, **👁** review requested).
- On the left, the PRs of the repository (open, merged, closed, yours), with a filter. At the
  start, **yours** (preference “Author shown at the start” to see all of them).
- A PR you just created opens as soon as GitHub knows it (a few seconds).
- **Conversation**, as on GitHub: the description on top, then everything that happened from the
  oldest to the newest, on a line with an icon per event: the comments and reviews (in a box “bob
  commented 2 days ago”), the **comments on the code** (clickable file:line, the commented code,
  the replies) and the commits (a click shows them). At the bottom: whether the PR can be merged,
  then the field to write a comment.
- **Handle the comments with Claude**: tick the unresolved comments to handle (box on the
  left of each, in Conversation or Files; “All” at the top), then **✳ Send to Claude**. A
  Claude conversation opens in the repository with each comment (file:line, commented code,
  replies) and changes the code; you check it in its review. You first need **⤵ Checkout** of
  the branch of the PR.
- **Apply the suggestion**: a comment with a `suggestion` block replaces the lines in your file
  and saves it (Ctrl+Z in the file to undo). You first need **⤵ Checkout** to have the branch
  of the PR.
- Branch of the PR checked out: the unresolved comments are **in the editor**, a small bubble in
  the margin, the text on hover.
- **Commits**: a click shows the whole message of the commit and its changes alone;
  **double-click** (or **Show in the Git view ↗**, or right click) opens it selected in the
  graph of the Git view (the commits of the PR are first fetched into `remote/pr/N` when they
  are not in your repository). **Files**: the whole diff of the
  PR, the conversations **under the commented lines** like on GitHub (reply, resolve, apply a
  suggestion), and **double-click a line** to comment on it.
- A box before each file marks it **viewed**: it folds and GitHub remembers it for your review
  (“3/10 viewed” on top).
- **✳ AI review**: Claude reads the PR over and proposes remarks under the lines of the diff;
  nothing is sent. Each one is **kept**, edited (✏) or dropped (×), then **Publish** sends the
  kept ones as one GitHub review. **💬** on a remark talks it over with Claude to refine it
  (“shorter”, “in English”, “are you sure? look at `_compute_total`”): it answers under the remark
  and rewrites it when needed, **↶ Previous version** goes back. Also in the Git view, for the
  changes not committed yet.
- At the bottom of the conversation, the merge box, in the color of the state (red blocked,
  orange something missing, green ready): where the PR stands (**Review required**,
  **Conflicts**, **Behind**, **Ready to merge**…), the **checks** (the failing ones always shown,
  a click for all; **Logs** opens the log of a GitHub Actions job in a tab, **↻ Rerun the failed
  ones**), and the buttons: **Merge** (merge commit, squash or rebase, delete the branch), **⟳ Update the
  branch** (merge or rebase of the base), **Ready for review** / **Back to draft**, **Close** /
  **Reopen**. Merge, close and rebase ask first.
- Under the message: **✔ Approve** or **✖ Request changes** (the message says what).
- On the right: the reviewers with their verdict (✔ approved, ✖ changes requested, 💬 commented,
  ⏳ waiting), “Review required” when the repository asks for an approval, and the reviewers GitHub
  suggests (**Ask** in one click); the assignees; the labels and milestone. To add someone else:
  **Add on GitHub ↗**, which completes the logins.
- Your comments have **✏** (edit) and **🗑** (delete). The title, the description and the
  reactions are changed on GitHub (**Open on GitHub**).
- Right-click a PR of the list (or **⋯** in the header): open, open on GitHub, checkout, copy
  the link or the number.

## Claude

**Ctrl+Alt+C** (or Tools → ✳ Claude, the palette): **Claude Code itself**, in a terminal in the
middle of the Claude tab.
Florm runs the `claude` command with your login: no key to set up. Its commands (`/model`,
`/usage`, `/mcp`, `/memory`, `/agents`…), its modes (**Shift+Tab**), its permissions and what is
new in it work as they are. Claude's changes are reviewed in the **Git** view.

- **At the top**: the folder (chosen before the first message: a repository of the project, a
  recent project, or **📁 Another folder…**), **+ Context**, the 👤
  account, the **History** (the earlier conversations, kept between sessions) and **⟳** to restart
  Claude (the conversation goes on where it was). On the right, the button of the **Session**
  panel; hidden, the **5 h** quota gauge is at the top right (those of **7 d** and the **Context**
  past 70 %, all of them on hover).
- **Session** (panel on the right): the **context** in % and the **cost** of the conversation,
  the **5 h** and **7 d quotas** of your subscription (with when they reset), the project, its
  branch and the **model**. Florm reads them in the status line of Claude Code. Below, the **files
  changed** by Claude in this conversation, the last one at the top: a click opens it. Florm reloads
  them and refreshes Git as soon as Claude changes them.
- **What Claude waits for**: when it asks for a permission or a question, its message (“needs your
  permission to use Bash”…) shows on hover over the conversation in the list and over **❓ waiting**
  in the status bar. When Florm is not in front, a notification of the system tells you (macOS;
  Linux with `notify-send`).
- **Several conversations, one tab**: every conversation is listed on the left of the Claude
  tab, the newest at the top, with its title (its first message), what it does (⟳ working, ❓
  waiting for your answer, ✔ done) and how long it has been working; below, its project in its
  color, its branch and its cost. A click shows one, the others go on behind; **+** starts a new
  one. Right click: **⇉ Duplicate** (a copy of its session, to try something else), **⇱ Open in
  its own tab**, **× Close** (Claude stops, it stays in the history). **Ctrl+Alt+W**: the next one
  waiting for your answer. The status bar says how many are working or waiting.
- **Sandbox** (Preferences → Claude, on by default): Claude's commands run isolated, they write
  only in the project and the network is filtered. Linux: `bubblewrap` and `socat` packages;
  macOS: built in (Seatbelt); Windows: only in WSL. Off, there is no sandbox,
  even if your settings of Claude Code turn one on. Changing it needs no restart of Florm: the open
  conversations start again with the new setting (those at work, once they are done). On,
  `/sandbox` in Claude sets the details.
- **After a restart of Florm**, each conversation goes on where it was (`claude --resume`).
- **End of an answer**: when the conversation is not shown, its tab (or, in the bottom panel,
  ✳ ● next to its name and ✔ in the status bar) blinks a few seconds then stays green until you
  look at it, and a sound is played. **Preferences → Claude**: blink or not, and the sound
  (Chime, Bell, Drop, Beep, Chord, Microwave, or none; ▶ to listen).
- **Give lines or files to Claude**: they are written in its field, not sent, so that you go on
  with your request. When hovering a line number, click the **✳** on its left (the line, or the
  selected lines); right click in the editor → **Ask Claude about the selection…**
  (**Ctrl+Alt+K**) or **Add … to Claude's context**; right click on a file or a folder of the tree
  → **Add to Claude's context** (`@file`, which Claude reads). **+ Context**: the open file, the
  selection, all the tabs, the uncommitted changes, the problems of the file, an image or a PDF,
  **A commit** (also: right click on a commit in the Git view → **Give this commit to Claude**).
  **Resume the conversation about this file** (palette) reopens the last conversation this file
  was given to.
- **✳ Send to Claude** from a pull request (its comments) or the conflicts assistant: a
  conversation in the repository, which starts with the request.
- **Picture of Florm**: **Ctrl+Alt+I**, from any view (a pull request, a file, the
  preferences…), takes a picture of Florm's window as it is and writes its path in Claude's field:
  no need to switch screens. Also in **+ Context → Picture of Florm**. Florm takes the picture
  itself: no permission of the system.
- **Images**: a capture made with the system's tool (**Print**) goes to the clipboard; **+ Context
  → Image from the clipboard** gives it to Claude. An image or a PDF can also be dropped from the
  file manager on Claude, or chosen with **+ Context → Image or PDF…**. An image is made smaller
  (1568 pixels) to cost fewer tokens.
- **Several Claude accounts** (work, personal…): **👤 → + Add an account…** at the top of Claude, or **Preferences → Claude → Claude accounts**,
  “+ Add an account”, then **Log in…** (done once, in a terminal of Florm; each account has its
  folder, `~/.claude-perso` for instance). The **👤** menu at the top of a conversation chooses its
  account: changed on the way, Claude starts again with it and the conversation keeps its context.
  The “default” account is used for new conversations and for commit messages, PR descriptions and
  reviews. **+ Folder…** on an account: the new conversations of that folder (and its sub-folders)
  use it; **📁** changes its configuration folder. The quotas follow the account of the
  conversation shown.
- **Florm context**: when Claude starts, Florm tells it what it knows of the project: the
  **commit message rules** (the project's, else yours) and the branch and version you work on.
  Claude then writes its commits like ✨. To teach it how to run your tests (a container, tags…), a
  Claude Code **skill** does it better: it is only used when useful.
- **My skills** (Preferences → Claude): a folder of yours for all your skills (a GitHub
  repository of its own, say), given to every conversation of Claude whatever the account: no
  project has to hold them. **Choose…** sets it; arrange it as you like, every `SKILL.md` found is
  a skill, even in sub-folders by theme. Claude sees them as `florm:name`. **+ New skill** creates
  a `SKILL.md` to fill in (its `description` tells Claude when to use it). Without a folder chosen:
  `skills/` of the shared configuration.
- **AI completion** (Preferences → Editor, or the palette: “AI completion while typing”): when
  you stop typing at the end of a line, Claude (Haiku) suggests what follows in grey after the
  cursor. **Tab** takes it, **Escape** or another key ignores it; typing its first letters keeps
  the rest. **Alt+\\** asks for a suggestion at once, even when turned off. Few tokens: only the
  lines around the cursor, without tools or CLAUDE.md (about a tenth of a cent per suggestion);
  it takes one to two seconds to come. With several Claude accounts, the 👤 menu next to the
  option chooses the one whose subscription pays for the suggestions (by default: the default account).
  **With GitHub Copilot** (the menu next to the switch): the suggestions come faster (usually under
  a second), with a Copilot plan. **Install Copilot** downloads its official server (about 160 MB)
  into Florm's folder, without npm nor admin rights; **Sign in to GitHub** copies a code and opens
  the page to type it in, once and for all.
- **Closing Florm while Claude works**: Florm says so and offers **Close when it is done**
  (Florm closes by itself at the end; “Don't close” to stay) or **Close now**. An update also
  waits for Claude to be done before restarting Florm.

**✨ next to the commit message** (Git view): Claude suggests the message from the staged
changes (else all of them), in the style of the last commits of the repository and from your
first request in the Claude tab.

**✂ next to ✨**: instead of a single commit, Claude splits all the changes (new files included)
into several commits by theme, each with its message. The plan takes the place of the Git view's
list: read and change the messages, move a block from one commit to another (**→**), leave it out
of the commits, change the order (**↑**). Nothing is committed before **Commit the N commits**;
**Cancel** leaves everything as before.

## Markdown

Above the text of a `.md` file, a **formatting bar**: **B** bold, *I* italic, ~~S~~ strikethrough,
`c` code, Link, headings H1 to H3, lists (• and 1.), checkbox ☐, quote ❝, code block, horizontal
rule `――` and table `▦`. A button puts the syntax around the selection, or on the selected lines;
a second click removes the prefix. The text stays editable by hand. To hide the bar:
Preferences → Appearance → Markdown toolbar. The rendered preview stays Ctrl+Shift+V (tab) or
Ctrl+K V (side).

In the preview (and in Claude's answers), **Mermaid sequence diagrams** (` ```mermaid ` then
`sequenceDiagram`) are drawn by Florm: participants, actors, messages, notes, `loop` / `alt` /
`opt`, `autonumber`. The other kinds of diagrams stay shown as code. Common **HTML** shows too:
`<br>`, `<b>`, `<i>`, `<kbd>`, `<img>`, `<a href>`, `<details>` / `<summary>`, `<h1>`…, `<p align>`;
`<!-- -->` comments are hidden.

## Writing

- **Spelling**: the unknown words of the Markdown, the comments of the code and the commit
  messages are underlined with blue dots, in French and English at once (the proper nouns, the
  code, the addresses and the identifiers are left out). **Ctrl+.** on a word offers its
  corrections, or **adds it to your dictionary** (`dictionary.txt`, in the shared configuration
  if there is one). The dictionaries (Grammalecte for French, SCOWL for English) are downloaded
  the first time, about 550 KB. Turned off in Preferences → Editor → Spelling.
- **Alt+Shift+→** expands the selection step by step: the word, the inside of the string, the
  string with its quotes, the inside of the parentheses (brackets, braces), with them, the line,
  the block of the same indentation, the whole file. **Alt+Shift+←** goes back one step.
- **Edit → Case**: UPPERCASE, lowercase, **snake_case**, **camelCase**, **Title Case**, on the
  selection or the word at the cursor (`parseHTTPRequest` → `parse_http_request`).
  **Edit → Remove duplicate lines** keeps the first of each repeated line (the selection, or the
  whole file).
- **Macros**: **Ctrl+Alt+Q** starts recording what you type and the editing commands,
  **Ctrl+Alt+Q** again stops it; **Ctrl+Shift+Q** plays it, at the cursor.
- **Parameters of a function**: with a language server, typing `(` or `,` in a call shows above
  the cursor the signature of the function, the current parameter underlined, and its
  documentation. It goes away at `)`, with Escape or when leaving the line.
- **Auto-save** (Preferences → Editor): **On leaving** saves the changed files when Florm loses
  the focus or another tab is shown; **After a pause**, a second after the last key. New files
  without a name are left out.
- **Unsaved work**: when closing, it is kept and reopened the next time, even without an open
  folder. It is also copied every 30 seconds: if Florm crashes, it comes back at the next start,
  with a message telling where the error is written (`crash.log`, in the configuration folder).
- **Colored brackets**: each level of parentheses, brackets and braces has its color (gold,
  purple, blue), not counting those in strings and comments. The indentation guide of the block
  of the cursor is stronger than the others.
- **Visible whitespace** (View, or Preferences → Appearance): a dot per space, an arrow per tab.
- **Distraction-free mode** (**Ctrl+K Z**, or View): full screen, only the text, centered; Escape
  or Ctrl+K Z to leave it.
- Without word wrap, a line going past the right edge fades out: you see that it goes on.

## Appearance

**Style** (Preferences → Appearance): **Rounded** (rounded cards and buttons, shadows under the
menus) or **Square**: straight corners, no shadows, and the whole interface in the system's
monospaced font (the terminal's, on Linux the one `fc-match monospace` gives), like a terminal or
Omarchy. An interface font chosen by hand still wins.

**Preferences → Appearance**: language (Français / English), theme (clickable previews: Florm and
Florm Light, and Florm Monokai, One Dark, Dracula, GitHub Light after the usual themes of other editors),
accent color, **softened text** (less “bold” text, useful on light themes), fonts of the editor
and of the interface, minimap, breadcrumbs, **menu bar** (hidden, its commands are still in the palette,
**Ctrl+Shift+P**).

**Theme colors → Create a theme from “…”**: all the colors of the editor (background, text,
accent, cursor, selection, gutter…) and of the code (comments, keywords, strings, functions,
classes, tags…, italic or not), applied right away. **Save the theme** writes it in the
`themes` folder of the configuration; **Edit this theme** / **Delete this theme** when it is
one of yours.

**Git can be turned off too** (Preferences → Git), and it is by itself when the `git` command is
not installed: no more Git view, history, blame, change marks in the gutter nor pull requests, and
`git` is no longer called. Its section then tells how to install it.

**GitHub, Claude and the debugger can be turned off**: a switch on top of their section of the
preferences (Pull requests, Claude, Debugger). Off, their views, commands, menus, icons,
shortcuts and buttons go away (✨ and ✂ of Git, AI review, ✳ column and breakpoints of the
margin, linked PRs in the Git view…), and `gh` is no longer called. The same switch brings them
back, without restarting.
