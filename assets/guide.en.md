# Florm guide

This guide opens with **Help → Florm guide** (or the palette: “guide”).
Every shortcut can be changed in **Preferences → Shortcuts** (Ctrl+,).

---

## Find and open

| Shortcut | Action |
|---|---|
| **Ctrl+P** | Go to a file of the project. `name:42` opens it at line 42, `:42` goes to line 42 of the current file |
| **Ctrl+Shift+P** | Command palette |
| **Ctrl+R** | Go to a class, a function, an Odoo record of the file |
| **Ctrl+O** / **Ctrl+Shift+S** | Open / save as: **Tab completes the path** like in a terminal |
| **F12** | Go to definition (also on `'sale.order'` or `'module.xml_id'`) |
| **Ctrl+.** | Quick fixes for the problem under the cursor (also the light bulb at the end of the line): import a name of the project, ignore (`# noqa`), `ruff --fix`, fix with Claude, and the actions of the language server |
| **Alt+Z** | Word wrap of the lines that are too long (display only; ↑ ↓ follow the rows) |

The breadcrumbs above the code are clickable: a folder shows its content, the file lists its
symbols, a class or a function takes you there. The views (Claude, Git, GitHub, Docker,
terminal…) have none: they keep the whole height.

**Groups of tabs**: right click a tab → **Add to a group** (an existing one or **+ New
group**). The tabs of a group stay side by side, with a band of its color and its name before
them: a click on the name folds the group (only its name stays, with the number of tabs), right
click to rename it, close its tabs or dissolve it. In a pull request, **📑 Open the files** opens
them in the group “PR #…”. The groups are kept with the session.

On restart (or back to a project), everything comes back as you left it: the tabs and the views,
the cursor position, the split editor with its tabs on each side, the open bottom panel and the
sidebar shown or not.

## Sidebar

- **Ctrl+Shift+E** (or a click on a file): the keyboard goes to the tree. **↑ ↓** to move,
  **→** opens a folder, **←** closes it (or goes up to the parent folder), **Space** shows the
  file keeping the keyboard in the tree, **Enter** opens it and goes to the editor, **F2**
  renames, **Delete** deletes, **Escape** goes back to the editor.
- While scrolling, the folders holding the top of the list stay pinned there; a click goes back
  up to the folder.
- **Double-click the edge** between the sidebar and the editor: the width fits the longest name of
  the tree. Drag the edge to change the width; **Ctrl+K Ctrl+B** hides the sidebar, and a click on
  the thin strip on the left shows it again.
- On the folder of a git repository: its branch, the tracked remote branch (↑ to push, ↓ to
  pull) and the number of changed files; hovering shows the remotes
  (can be turned off in Preferences → Git).
- The icons of the views (Preferences, Search, Terminal, Claude, Git…) are **in the status bar, bottom left**, as in Zed. **▾** at the end of the row chooses those shown, and can move them back to the sidebar (top or bottom). A hidden view stays in the palette (Ctrl+Shift+P) and the menus.
- **Copy / paste files**: right click → **Copy** puts the file or folder on the system clipboard; **Paste** (or Ctrl+C / Ctrl+V in the tree) drops the files copied from your PC's file manager, or from the tree. A name already taken becomes “name (copy).ext”: nothing is replaced.
- Right click on a folder → **Initialize a git repository here** (`git init`), for a new project.

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

- **Ctrl+Shift+G** (or click the branch in the status bar): Git view. At the top the repository (large, a click lists them when the project has several), the branch (**🔀 name ▾**: switch or create one),
  pull / push / fetch and the commit message (Ctrl+Enter); below it the graph, whose first
  rows are the changes to stage / staged.
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
- **File history** (right click in the editor, Git menu): all its commits, through its renames.
  **Line history** (Ctrl+Alt+B) also takes several selected lines.

### Split a commit

Right click on a commit → **Split this commit into several…**, or in the **interactive rebase**
(right click → “Interactive rebase from here…”), the action **Split into several commits** on
one of the commits.

1. Tick the blocks that go in the first part — or **click + / − lines** to take only some of
   them (like “e” in `git add -p`): a + line not taken is not added, a − line not taken stays
   in the file.
2. Write the message, then **Commit part 1**.
3. Start again with the rest; **Finish** replays the next commits.
   **Cancel the splitting** puts everything back exactly as before.

## Terminal

- **Ctrl+`** (or the **>_ Terminal** button of the status bar): the terminal at the bottom.
- **Ctrl+Shift+`** (or click the Terminal icon at the bottom of the sidebar): a terminal **in a
  tab** of the editor. Like any tab, it can be dragged to the right or bottom edge of the editor
  to be next to a file (or right click on the tab → Split right / down).
- **In a tab** in the bottom panel sends the current terminal there, **at the bottom** brings it back.
- All the views (Search, Git, GitHub, Docker, Claude, Debug, Terminal) have the same buttons at
  the right of their header, in the same order: **⟳** read again, **in a tab / at the
  bottom** (a small window: with a tab on top, or a band at the bottom), **⋯** the other actions
  and options. The layout buttons all look alike: the four corners maximize a panel in its view
  (Escape to come back), a window with a column on the right shows or hides a side panel (the
  review of Claude), filled when it is shown. Git, the Preferences and the split of a commit are
  tabs like the others: Escape goes back to the previous tab, Ctrl+W closes them.

## SQL (PostgreSQL)

**Tools → SQL** (or “sql” in the palette): a tab to query a PostgreSQL database with the `psql`
command (the client alone is enough).

- On top, **Database**: the list of the databases of the server; type a few letters to find the
  right one, ↑ ↓ and Enter to choose it. ⟳ reads the list again. **Server** stays empty for the
  PostgreSQL of this machine; else `host=… port=… user=…` or `postgresql://user@host:port`. The
  password goes in `~/.pgpass` (or the `PGPASSWORD` variable) rather than in the field.
- The query, colored; **▶ Run** or **Ctrl+Enter**. Its area is small: drag its lower edge to make
  it taller. The result shows as a table (resizable columns, `NULL` in grey, click a value to copy
  it, **Copy (spreadsheet)** to paste it all in a spreadsheet), with the number of rows and the
  time. **■ Stop** interrupts a query too long.
- **⏱ EXPLAIN ANALYZE**: the plan of the query as a tree of steps, as on explain.dalibo.com. For
  each step: its share of the execution time (a bar, red from half of it, orange from a fifth),
  its own time, the rows given and those expected (⚠ when the estimate is off by a factor of 10:
  statistics to check), the table and index read, and its conditions (filter, join, sort,
  buffers). The query runs in a transaction rolled back afterwards: an `UPDATE` or a `DELETE`
  changes nothing. **Copy the plan (JSON)** gives it as is, to paste in explain.dalibo.com for
  even more detail.
- The database, the server and the query are kept with the project.

## Docker

**Docker** in the palette (“docker”) or **Tools → Docker (containers and logs)**: the list of
containers on the left, their logs on the right. **In a tab** shows it large in the editor, **at
the bottom** brings it back to the bottom. The list is read again every few seconds.

- **List**: the containers are grouped by `docker compose` project. The dot next to the name is
  green when it runs, yellow when it restarts, is paused or is not *healthy*, red when it is
  dead, grey when it is stopped.
- **■ / ▶** next to a container: stop or start it. For a project, **■ all** stops all its
  containers, from the bottom of the list up; **▶ all** starts the stopped ones in the order of
  the list, each once the previous one is ready (*healthy*). The dotted handle on the left can be
  dragged to change that order.
- **⬆ compose up** (or **⬆** on a project): `docker compose up -d` in the bottom panel. Also creates and
  starts the services that have no container yet, and recreates those whose configuration changed.
- **Logs**: click a container, its logs show on the right. If it restarts, the logs start again by
  themselves. The button at the top right of the logs makes them larger (click it again to give the
  list its room back).
  - In its bar: **⟳ Restart**, **■ Stop** (or **▶ Start**), **>_ Terminal** (a shell in the
    container, `docker exec`, in a tab).
  - **Attached**: a container waiting for input (ipdb…) receives what you type, and **Ctrl+C**
    interrupts it. Otherwise the logs are read-only.
- **🧪 Tests**: runs Odoo tests in the container. The tags are typed in the field
  (`:MyClass`, `/my_module`, `:MyClass.test_x`); **▾** brings back those already run.

## Debugger

Like VS Code's, for Python and Odoo, on this computer or in a Docker container. **Debug** in the
palette, or the **Debug** tab of the bottom panel.

- **Breakpoint**: click left of the line numbers of a Python file (or **F9**). A red dot. In the
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
    In a container: `python3 -m debugpy --listen 0.0.0.0:5678 /usr/bin/odoo … --workers 0` in the
    `command:` of `docker-compose.yml`, debugpy installed in the image, and **Where** set to that
    container.
    **It is all automatic, whatever the mode**: as soon as a breakpoint is set, Florm waits for a
    program listening with debugpy and attaches to it, without F5 nor anything to choose; when the
    program ends, it waits for the next run. Start Odoo
    as you like (Florm's terminal, yours, `docker compose up`): **Copy the command** gives the line
    to run. **■ Stop waiting** (or Shift+F5) stops waiting, F5 waits again.
- **Paused**: the line is highlighted in yellow, the window comes to the front and the debugger opens in the bottom panel, under the file (unless it is in the other half of a split editor). On the left the
  **Variables** (click to unfold `self`, a recordset…), the **Watch** (expressions evaluated again
  at each stop), the **Call stack** (click on a call: its line and its variables, in green).
  Hovering a name shows its value (`self.partner_id.name`).
- **Console** (on the right): Python evaluated in the paused frame,
  `self.env['res.partner'].search([])`, `x = 2`. ↑ ↓ bring back the previous ones.
- **F10** next line, **F11** step into the call, **Shift+F11** finish the function, **F5**
  continue, **F6** pause, **Shift+F5** stop, **Ctrl+Shift+F5** restart.
- **Odoo tests**: right click on the **▶** of a test in the gutter (or **Alt+F6**) runs it under
  the debugger, in the chosen container (else the tests' one).
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
of a built-in template replaces it.

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

- On the left, the PRs of the repository (open, merged, closed, yours), with a filter.
- **Conversation**: the description, the **comments on the code** (clickable file:line, the
  commented code, the replies) then the discussion and the reviews.
- **Apply the suggestion**: a comment with a `suggestion` block replaces the lines in your file
  and saves it (Ctrl+Z in the file to undo). You first need **⤵ Checkout** to have the branch
  of the PR.
- Branch of the PR checked out: the unresolved comments are **in the editor**, a small bubble in
  the margin, the text on hover.
- **Commits**: a click shows the changes of that commit alone; **Files**: the whole diff of the
  PR, the conversations **under the commented lines** like on GitHub (reply, resolve, apply a
  suggestion), and **double-click a line** to comment on it.
- A box before each file marks it **viewed**: it folds and GitHub remembers it for your review
  (“3/10 viewed” on top).
- On top of the conversation: the **checks** (the failing ones always shown, a click for all;
  **Logs** opens the log of a GitHub Actions job in a tab, **↻ Rerun the failed ones**), then
  whether the PR can be merged (conflicts, behind, missing approvals or checks) and the
  buttons: **Merge** (merge commit, squash or rebase, delete the branch), **⟳ Update the
  branch** (merge or rebase of the base), **Ready for review** / **Back to draft**, **Close** /
  **Reopen**. Merge, close and rebase ask first.
- Under the message: **✔ Approve** or **✖ Request changes** (the message says what).
- On the right: reviewers, assignees (`me` for you), labels and milestone, to add or remove.
- **✏** next to the title: change the title, the description or the base branch.
- Under each message, its **reactions** (a click gives or takes back yours, 🙂 to add one);
  your comments have **✏** (edit) and **🗑** (delete).
- Right-click a PR of the list (or **⋯** in the header): open, open on GitHub, checkout, copy
  the link or the number.

## Claude

**Ctrl+Alt+C** (or Tools → ✳ Claude, the palette): a conversation with Claude Code, in the
bottom panel (**in a tab** to give it a whole editor tab, **at the bottom** to put it back). Florm uses the `claude`
command and your login: no key to set up.

- At the top: the repository, the model, and what Claude is allowed to do (**Edit files**,
  **Plan only**, **Allow everything** so that it also runs commands).
  The model and the mode can change in the middle of a conversation: at once if Claude is
  working, else from the next message, the conversation kept. **Shift+Tab** in the field: next
  mode, as in Claude Code.
- **End of an answer**: if the conversation is not shown, its tab (or, in the bottom panel, the
  Claude tab and its icon in the sidebar) blinks a few seconds (the whole tab lights up) then stays green until you look at
  it, and a sound plays. **Preferences → Claude**: blink or not, and the sound (Chime, Bell,
  Drop, Beep, Chord, Microwave, or none; ▶ to listen).
- **/** at the start of the field: the commands of Claude Code (`/usage`, `/context`, `/compact`,
  `/clear`, `/model`, your skills…), **Tab** or a click completes. They go alone, without the
  context. **/btw question** asks a question on the side, even while Claude is working: the
  answer shows above the field and does not enter the conversation. **/status**: version, model,
  session, folder. Those of the terminal interface only (`/doctor`…) open in a terminal below.
- **🔒 Permissions → Rules**: besides the boxes, precise rules allowed or refused, as in Claude
  Code's `/permissions`: `Bash(git *)`, `Edit(src/**)`, `Read(./.env)`,
  `WebFetch(domain:github.com)`, `mcp__github` (the tools of an MCP server). A rule allowing a
  part of a tool not checked (`Bash(git *)` without “Run commands”) allows that part. A change
  applies from the next message, the conversation kept.
- **⋯** in the bar: the **MCP servers** (state, add a URL or a command, for this project, shared
  in `.mcp.json` or for all your projects, remove), the **subagents** (open, create one), the
  **hooks** of the settings files, the **memory** (CLAUDE.md) and the **extra folders** Claude may
  use (`--add-dir`). Also as commands: **/mcp**, **/agents**, **/permissions**, **/memory**,
  **/add-dir path**.
- **# at the start of a message**: the line is not sent, it is remembered in the CLAUDE.md of the
  project or in your personal memory (`~/.claude/CLAUDE.md`), as you choose.
- **@ in the field**: the files of the project matching what is typed; ↑ ↓ to choose, **Tab** or
  **Enter** completes, and the file goes with the message.
- **🌿** (worktree): a new conversation on a branch of its own, in its own folder (a git worktree),
  to have several Claudes work at once without getting in each other's way. Each conversation has
  its tab and keeps working even when it is not shown.
- **Options ▾**, next to “+ Context”, groups what goes with each message:
- **Open file and selection**: Claude knows which file you are editing and what you selected.
- **Pasting**: a long pasted text (more than 6 lines or 500 characters) becomes a “Pasted text (N
  lines)” chip above the field, sent whole with the message; the field stays short. A click on the
  chip shows the whole text below it, editable; another click folds it. The pasted text can go
  alone: **Send** works even when the field is empty. A chip is removed with its ×.
- **Images**: an image goes with the message, scaled down to 1568 pixels to cost fewer tokens.
  Three ways to give it: paste it with Ctrl+V (a copied image), drop it from the file manager on
  the conversation, or **+ Context → Image…**. For a screenshot: take it with the system's tool (it
  goes to the clipboard), then Ctrl+V. The image shows as a thumbnail above the field, then in the
  message sent; a click opens it full size.
- **Questions first**: if the request leaves choices open, Claude codes nothing and asks 1 to 3
  questions with options (the recommended one ticked already, **Other** to write yours);
  **Answer** sends them and it goes on. In the task queue, the task waits for your answers
  before the next one.
- **Model first**: before a message goes, Haiku checks whether the request needs another model
  (Opus for a hard job, Haiku for a small task) and suggests it. You choose **Switch to…** or
  **Keep…**; **Cancel** puts the message back in the field. The tasks of the queue are checked too
  (unless they have their own model); **Cancel** there puts the task back to waiting and pauses the
  queue. A message written during the check goes in the queue, after the answer.
- **Warnings**: the chat says once when the conversation fills over 60% of the context (`/compact`
  summarizes it), and when Claude opens an image (it costs many tokens with every next message).
  The size of the context joined to a message shows in the status bar.
  Over 60%, a bar offers **/compact** or **New conversation**. The options of the menu stay with the
  conversation when you resume it.
- **Mentioned definitions**: the classes and functions of the project named in the message
  (`SaleOrder`, `_compute_total`, `foo()`…) go with it, their first lines included, found in
  Florm's index: Claude does not have to look for them (fewer tokens). A name defined in more
  than two places is not given.
- **Give lines or files to Claude**: when hovering a line number, click the **✳** on its left
  (the line, or the selected lines); right click in the editor → **Ask Claude about the
  selection…** (**Ctrl+Alt+K**) or **Add … to Claude's context**; right click on a file or a
  folder of the tree → **Add to Claude's context**. They show as chips above the message
  (click: remove) and go with it. **+ Context → A commit** (or right click on a commit in the
  Git view → **Give this commit to Claude**): its message and its diff, to start from what it
  does (“do the same for…”).
- On the right, **what Claude changed** since your first message, file by file and block by
  block: **✔ Keep** (the block leaves the review and stays in the file) or **↺ Reject** (it is
  removed from the file). Your own earlier changes are not in it.
- **Question by question**: the **▾** in front of one of your messages folds the answer to that
  question (one line stays: “N steps · N files changed”); right click on the ▾: fold all but the
  last one, or unfold all. **±** on the message shows on the right the file changes of that
  question alone (**× Whole review** to go back to the review).
- **↶ Go back**: before each message, Florm keeps the state of the files (in git, no commit and
  nothing staged). **↶** on a message (or **/rewind** for the last one) offers to put **the files
  and the conversation** back as before that message, **the conversation only** or **the files
  only**. What changed since is lost; the message goes back to the field to be written again.
- **While it works**, you choose where your message goes:
  - **Send** (Ctrl+Enter): at once. Claude reads it between two steps and takes it into account in the
    work going on (“use that file instead”, “add a test too”…).
  - **⏭ After** (Ctrl+Shift+Enter): the message waits in the queue and is sent by itself when Claude is
    done. A list stays one message (no splitting); the queue makes no commit for it.
- **Task queue**: write several requests as a list (`- …`, `1. …`) and click **+ Queue**
  instead of Send: each item becomes a task (the sentence before the list goes with each).
  **Run** sends them one by one in the same conversation, each when Claude is done with the
  previous one; **Commit after each task** makes a commit (message written by Claude) in
  between. An error or **■ Stop** pauses the queue; a waiting task can be edited or moved up
  (↑); any task except the running one can be removed (×), and the **×** of the queue closes
  it.
  For fewer tokens: **New conversation per task** (Claude does not read the whole history
  again, the conclusion of the previous task is given to it) and a **model for the queue**
  (Haiku or Sonnet for small tasks, the conversation's model otherwise), which a waiting task
  can replace with its own (menu next to the task).

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
`markdown_toolbar = false` in `settings.toml`. The rendered preview stays Ctrl+Shift+V (tab) or
Ctrl+K V (side).

## Appearance

**Preferences → Appearance**: language (Français / English), theme (clickable previews: Florm and
Florm Light, and Florm Monokai, One Dark, Dracula, GitHub Light after the usual themes of other editors),
accent color, **softened text** (less “bold” text, useful on light themes), fonts of the editor
and of the interface, minimap, breadcrumbs, **menu bar** (hidden, the menus are still under
**☰** at the bottom left).

**Theme colors → Create a theme from “…”**: all the colors of the editor (background, text,
accent, cursor, selection, gutter…) and of the code (comments, keywords, strings, functions,
classes, tags…, italic or not), applied right away. **Save the theme** writes it in the
`themes` folder of the configuration; **Edit this theme** / **Delete this theme** when it is
one of yours.

**GitHub, Claude and the debugger can be turned off**: a switch on top of their section of the
preferences (Pull requests, Claude, Debugger). Off, their views, commands, menus, icons,
shortcuts and buttons go away (✨ and ✂ of Git, AI review, ✳ column and breakpoints of the
margin, linked PRs in the Git view…), and `gh` is no longer called. The same switch brings them
back, without restarting.
