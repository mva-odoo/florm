# Florm guide

This guide opens with **Help → Florm guide** (or the palette: “guide”).
Every shortcut can be changed in **Preferences → Shortcuts** (Ctrl+,).

---

## Find and open

| Shortcut | Action |
|---|---|
| **Ctrl+P** | Go to a file of the project. `name:42` opens it at line 42, `:42` goes to line 42 of the current file |
| **Ctrl+Shift+P** | Command palette: the views first (Claude, Git, Pull requests, Search, Terminal…, the terminals and the conversations), then the commands. Type to filter (accents are optional); a view whose name has the words typed stays first (“pull request” → the pull requests view, then the commands). A leading **`#`** lists only the views: `#git`, `#term`, `#claude`. |
| **Alt** alone | Everything from the keyboard: one or two letters on each visible button, tab, box or field; typing them clicks it (Esc closes). In an open menu, ↓ ↑ move between its entries and Enter chooses. Can be turned off in Preferences → Editor |
| **Ctrl+R** | Go to a class, a function of the file |
| **Ctrl+O** / **Ctrl+Shift+S** | Open / save as: **Tab completes the path** like in a terminal |
| **F12** | Go to definition |
| **Ctrl + mouse over a name** | The name underlined like a link and the list of its definitions in the project (a click goes there); without Ctrl, hovering only shows the documentation and the errors |
| **Ctrl+.** | Quick fixes for the problem under the cursor (also the light bulb at the end of the line): import a name of the project, ignore (`# noqa`), `ruff --fix`, fix with Claude, and the fixes of the language server in every language (import a name in Go, Rust, TypeScript…), at the top of the list; the cursor can be anywhere on the line |
| **Alt+Z** | Word wrap of the lines that are too long (display only; ↑ ↓ follow the rows) |
| **Shift+F12** | Usages of the symbol (language server, else the whole word in the folder) |
| **Alt+-** / **Alt++** | Go back to the previous cursor position / forward to the next one (across files too) |
| **Ctrl+Alt+P** | Switch project (each open folder is one, found again as you left it) |
| **Ctrl+\** | Split the editor: the current file on the right; **Ctrl+1** goes from one side to the other |

The breadcrumbs above the code are clickable: a folder shows its content, the file lists its
symbols, a class or a function takes you there. The icon on its right copies the path of the
file (right click: the file:line reference or the GitHub link of the line). The views (Claude, Git, Pull requests (GitHub),
terminal…) have none: they keep the whole height.

Two open files of the same name: their folder shows in grey after the title (`input.rs …/app`). A file with errors shows how many in red (`9+` beyond nine).

On the right of the tab strip: **+** opens an empty text tab (Ctrl+N), **▾** a view in a tab, each with its icon: Search, Claude, Git, Pull requests (GitHub), then Terminal, Preferences and Welcome. A title too long (a conversation, a pull request) is cut with “…”, whole on hover. When the tabs no longer fit in the strip, ‹ › scroll them and the same **▾** lists all the open tabs at the top.

On restart (or back to a project), everything comes back as you left it: the tabs and the views,
the cursor position, the split editor with its tabs on each side, the open bottom panel and the
sidebar shown or not.

## Editing

- **Several cursors**: **Ctrl+D** selects the word then adds the next occurrence, **Alt+F3**
  takes them all, **Ctrl+Shift+L** puts a cursor on each selected line, **Ctrl+Alt+↑ / ↓** adds
  one above / below, **Ctrl+click** where you want. **Alt+drag** (or drag with the middle button)
  selects a column. What is pasted is spread over the cursors; Escape goes back to one.
- **Bookmarks**: **Ctrl+F2** sets or removes a bookmark on the line, **F8 / Shift+F8** go to the
  next / previous one, “Bookmark: list” (palette) shows them in every open file.
- **Folding**: **Ctrl+Shift+[** folds the block of the cursor, **Ctrl+Shift+]** unfolds it;
  **Ctrl+K Ctrl+0** folds everything (classes, functions…), **Ctrl+K Ctrl+J** unfolds everything.
  Also the arrows in the gutter.
- **XML and HTML tags**: typing `>` at the end of an opening tag (`<field name="x">`) adds its
  closing tag (`</field>`) after the cursor, and `</` completes the one of the element still open.
  Nothing for `<br/>`, `<?xml ?>`, `<!-- -->` nor the HTML tags without a closing one (`<br>`,
  `<img>`…). Follows the “Auto-close brackets” preference (Preferences → Editor).
- **Emmet** (HTML, JSX, XML): an abbreviation then **Tab** writes its tags. `ul>li.item*3`,
  `.card>h2+p`, `nav>ul>li^footer`, `a[href=/]`, `{text}`, `!` (an HTML page). In XML (Odoo
  views), `field[name=partner_id]` gives `<field name="partner_id"/>`, `group>field*2` two fields
  to fill, `button`, `record`, `xpath` their usual attributes. **Tab** then goes from one empty
  place to the next. Also “Emmet: expand the abbreviation” (palette); turned off in
  Preferences → Editor.
- **Inlay hints**: with a language server that gives them (rust-analyzer, basedpyright,
  typescript-language-server, gopls, clangd), the inferred types and the names of the parameters
  are shown in grey in the code (`let x: i32`, `f(a: 1)`). They are not part of the file; turned
  off in Preferences → Editor.
- **Vim mode** (Preferences → Editor, or “Vim mode” in the palette): normal, insert (**i**,
  **a**, **o**…) and visual (**v**, **V**) modes, motions (`w b e 0 ^ $ gg G f t % { }`, with a
  count: `3w`), the operators `d c y > <` with a motion or a text object (`ciw`, `di"`, `ca(`,
  `yap`), `dd cc yy x p P u` **Ctrl+R** `.` `J ~ r`, the search `/ n N * #` and the command line
  `:w :q :wq :42 :%s/a/b/g`. Also the registers (`"ayy`, `"Ayy` appends, `"ap`, `"0p` the last
  yank, `"+p` the clipboard, `"_d` keeping nothing), macros (`qa` … `q`, `@a`, `@@`), marks (`ma`,
  `'a`, `` `a ``, also with an operator: `d'a`) and the visual block (**Ctrl+V**, then `I` / `A` to
  type on all the lines, `c`, `d`, `y`, `r`, `~`, `>`). The mode is shown in the status bar. The
  shortcuts with Ctrl stay Florm's (Ctrl+S, Ctrl+P…).
- **Semantic highlighting**: the language server colors what the syntax alone cannot tell apart
  (parameters, properties, types, constants, `self`), with the colors of the theme.
- **Code lenses**: what the server offers on a line (“▶ Run”, “3 implementations”…), in grey at the
  end of the line; a click runs it.
- **Call hierarchy**: **Shift+Alt+H** (or “Who calls this function…”) lists the functions calling
  the one under the cursor, Enter goes to the call; “What this function calls…” the other way.
  Without a server that gives it, the usages.
- **Renaming or moving a file** (tree): the language server first updates the imports that name
  it (TypeScript, Rust, Python with basedpyright…); the files that were not open are saved, the
  open ones are still to save.

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

## Bottom panel and status bar

One simple rule: **files and the views that need room (Claude, Git, Pull requests (GitHub)) go in
the tabs, the search too; the bottom panel keeps what comes along with the work**: terminals,
debugger, problems, output. To see two tabs side by side, split the editor.

- **The bottom panel**: its views are chosen with the **icon tabs at the bottom right** of the status
  bar, **Terminal**, **Debugger** (green during a session, orange when paused), **Problems** (their
  number) and **Output**; a click shows the view, another one hides it. At the top of the panel, the
  name of the view; for the terminal, one tab per terminal (**×** on hover closes it) and **+** for
  another one. **Ctrl+Shift+J** shows or hides the panel.
- **The bar at the top**, as in Zed: the tab strip goes on over the sidebar. On the left, the **F**
  (the menus File, Edit, Git…, as Zed's ☰), the **version number** (a click looks for an update) and,
  when there is one, the round **⬆** button of the update (**⟳** once installed: restart). The name
  of the project is right above the tree (a click: switch project; right click: new file, new
  folder, refresh, fold everything…). Then **the views as icon tabs**: **Preferences** (right click:
  Languages, Extensions), **Claude**, **Git**, **Pull requests** and **Search** in files (a click
  shows the view in place, without one more tab; another one goes back to the tab before); then the
  tabs of the files, and **+** for a new file. The files have the icon of their type (Material Icon
  Theme). At its end, the window's buttons (minimize, maximize, close) on Windows and
  Linux, round ones on the left on macOS, none under a tiling window manager (Hyprland, Sway…).
  Dragging the bar moves the window.
- **The status bar only shows what is going on**, on the left: the branch (**↑** commits to
  push, **↓** to pull, **●** changed files) of the repository of the file shown (it follows the
  tabs; a repository chosen in the Git view stays until the next file shown), then, only when it is the case, Claude (**working**,
  or **✔** when it has finished: a click shows the answer), and the problems. At rest, it is almost empty.
- The Preferences: **Ctrl+,**, the menu or the palette.

## Find in folder (Ctrl+Shift+F)

Enter runs the search. The results are colored like in the editor; **F4 / Shift+F4** go to the
next / previous result. The search is a page of its own, in an editor tab: all the room for the
results.

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
- With **Replace** (on the right of **Find**), the replacement shows; in regex `$1`, `$2` take back the groups:
  `fields\.(\w+)\(` → `Fields.$1(`.
- **New line** in the replacement: **Shift+Enter** in the field, or `\n` in regex
  (`\t` tab, `\\` backslash). To turn the `\n` written in the text into real new lines: regex,
  find `\\n`, replace with `\n`. Also in Ctrl+H.
- **Keep case**: cat → dog, Cat → Dog, CAT → DOG.
- **×** on a result (or a file) drops it: it will not be replaced.
- **✏ Change the results** (like Zed's multibuffer): the lines found, with one line around, in a
  tab where they are changed as code (several cursors, Vim, Claude…), under a header
  `▌ file:line` not to touch. **Ctrl+S** writes each excerpt into its file; a file changed since the
  search is left as it is (the message says which).

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
A folder received from elsewhere (a zip) may have commands in its `.git/config`: Florm runs git
without `core.fsmonitor` nor the diff programs of the repository (`textconv`, external diff), so
opening the folder or showing a diff does not run them. The hooks of commits and pushes run as usual.

- **The changes in the editor**: a colored mark in the gutter shows each block changed since the
  last commit. **A click on the mark** (or **Ctrl+K Ctrl+D** on the line) shows that change
  alone, in the text, as in Zed: the removed lines on red above, the new ones on green (gutter
  included), the words that changed on a stronger background, and a small bar (previous, next,
  ↺ revert, All, ×). Another click, Escape or Ctrl+K Ctrl+D hides it.
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
- **Interactive rebase**: **🔀 branch ▾ → Interactive rebase…** (or “Git: interactive rebase” in
  the palette) asks for the starting branch (`master`, `origin/17.0`…) and lists the commits of the
  current branch that are not in it; or right click on a commit of the graph → **Interactive
  rebase from here…**. For each commit: keep it, change its message, merge it with the previous
  one (with or without its message) or drop it, and ↑ ↓ to change the order.
- **Blame** (**Ctrl+Shift+B**): the author, date and message of the last commit of the line, in
  grey at its end; again to hide it. Hovering that text opens a small window with the commit's
  whole message, **See in Florm (Git)**, **Open on GitHub ↗** and **Copy the hash**.
- **Blame of every line** (**Ctrl+K B**, or right click → Blame of every line), as in Zed: a column
  of the gutter gives the author and age of each block of lines of the same commit; on hover, a
  small window gives the commit, its author, its date and **its whole message**, with **See in Florm
  (Git)**, **Open on GitHub ↗** (if the repository is there) and **Copy the hash**; it stays while
  the pointer goes to it. A click on the column also opens the commit in the Git view. The lines changed since the last commit
  say “not committed yet”. One `git blame` per file and per commit (a few seconds on a very long
  history, in the background): typing does not run it again.
- **Amend the last commit** (box under the message, or branch menu): the message of the last
  commit comes back in the field; the button replaces it (with the staged files added). If it
  was already pushed, a push --force-with-lease will be needed.
- **↕ Sync**: Pull, **Pull --rebase** (your commits replayed on top, no merge commit), Push,
  **Push --force-with-lease** (after a rebase or an amend: refused if someone pushed meanwhile),
  **Push the tags**, ⟳ Fetch, and **⤵ Rebase onto… (after fetch)**: fetches the commits of the server, then
  offers the branch to replay yours onto (the server's first, master or main on top). When a push is refused, the error message also offers
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
  - **✨ Commit message rules**: opens `.florm/commit.md`, what Claude must follow for ✨ in
    this project (language, prefixes, length…). Commit it to share it. Without this file, Claude
    takes the “Commits” section of the project's CLAUDE.md if there is one.
  - **✨ My commit message rules**: yours, for every repository without its own (`commit.md` of
    the config folder): nothing to add to a shared repository. For instance: in English,
    `[FIX] module: …`, the why in the body, `task-…` at the end.
  - Both also open with a **right click on ✨**, next to the commit message.
- **File history** (right click in the editor, Git menu): all its commits, through its renames.
  **Line history** (Ctrl+Alt+B) also takes several selected lines; each commit shows the whole
  block that changed around these lines, with some context, and the last one is the creation
  of the file (✚). The history opens in a tab (↑ / ↓ to
  go from one commit to the next, Escape to come back).
- **Side-by-side diffs**: the **◫ Side by side** button, at the bottom right of every diff (Git,
  history, Claude's review, pull requests), puts the old version on the left and the new one on
  the right, each replaced line facing its new version, with the changed words darker.
  **≡ One column** goes back to the usual display. The choice applies to every diff and is kept.

### Resolve conflicts (rebase, merge, cherry-pick)

As in VS Code, conflicts are resolved **in the file**. When a rebase, a merge, a cherry-pick or a
revert stops on conflicts, the first conflicting file opens on its first block (also with **⇄ Go
to the first conflict** in the banner of the Git view).

- Each block has a **green** side and a **blue** side. After its `<<<<<<<` line, links: **Keep the
  green · Keep the blue · Keep both · Compare** (the two sides as a diff, in a tab). Ctrl+Z goes
  back.
- **The bar above the file** says what is going on (“Rebase of feature onto master — commit 3 of
  7”) and **the real name of each color**: in a rebase, green is the base your commits are
  replayed onto, blue your commit. ◀ ▶ go from block to block; the same choices are there for the
  block under the cursor, with **✳ Resolve with Claude** (Claude gets the file, both sides and
  what the commit changes in it, and removes the markers without touching git) and **Abort**.
- Once the file has no marker left: **✔ Mark resolved, next file ›** (saves, git add, opens the
  next conflicting file), or for the last one **✔ Mark resolved and continue the rebase ▶**. If
  the next commit has conflicts, its first file opens in turn.
- A file removed on one side and changed on the other is **kept** or **removed** in the banner of
  the Git view, which also has **Continue**, **Skip this commit** and **Abort**.

The operations started from Florm put the original version between the two sides
(`merge.conflictStyle=diff3`).

## Terminal

- **Ctrl+`**: the terminal in the bottom panel.
- **Shortcuts in a terminal** (and in Claude): Ctrl+letter (Ctrl+C, Ctrl+R, Ctrl+D…), Alt+letter
  and Ctrl+Shift+C / V / F go to the shell; the F keys (F2, F12…), Ctrl+Alt+…, the other
  Ctrl+Shift+… (Ctrl+Shift+G, Ctrl+Shift+J…) and Ctrl+Tab, Ctrl+PgUp / PgDn stay with Florm.
- **Shift+Enter** (or Alt / Option+Enter) goes to a new line without sending, in Claude as in zsh
  and fish.
- **Links**: **Ctrl+click** (⌘+click on a Mac) on a web address opens it in the browser; on a file
  path printed by a program (`src/app.rs:12`, `~/notes.txt`, a full path, the `b/…` of a diff), it
  opens it in Florm, at the line. With Ctrl held, the link under the mouse is underlined. In
  Claude as in the shell.
- **Scrolling**: the wheel, the scrollbar on the right, **Shift+↑ / Shift+↓** (a line) and
  **Shift+PgUp / Shift+PgDn** (a page) go through the history; ↑ and ↓ stay with the program (the
  history of commands, the choices of Claude's menus). Claude writes its whole conversation there,
  to scroll back through. A program that handles the mouse itself (htop…) gets the wheel.
- **Double click** selects a whole word (a path, an address), **triple click** the line.
- **Dragging a file** onto a terminal writes its path at the prompt (quoted when needed): Claude
  reads the file, an image included.
- The links a program puts on its own text (“PR #12” leading to the page of the PR…) open the
  same way. Only `http` and `https` addresses go to the browser; a `file://` address opens the
  file in Florm, the others (`javascript:`, `mailto:`…) are not opened: the status bar says so.
- **Ctrl+Shift+F** (⌘F on a Mac, or right click → Find…) searches the whole history of the
  terminal: **Enter** goes up to the older match, **Shift+Enter** down, **Esc** closes. Capitals
  only count if you type one.
- The bold, italic, underlined and grayed text of the programs is shown, and the cursor shape they
  ask for (the bar of Claude's field, a blinking cursor). A program that copies text (Claude's
  `/copy`, vim, tmux, even over ssh) puts it in the clipboard.
- **Theme colors**: the black, grays and white of the programs are those of Florm's theme, and the
  other colors match it; a program that asks for the terminal's colors (Claude Code, vim) gets
  those of the theme and picks its light or dark colors accordingly.
- A program that rings (the terminal bell) shows 🔔 before the name of its tab until you come back
  to it; if Florm is behind another window, its icon says so.
- **Ctrl+Shift+`**: a terminal **in a
  tab** of the editor. Like any tab, it can be dragged to the right or bottom edge of the editor
  to be next to a file (or right click on the tab → Split right / down).
- In the bottom panel, **each terminal has its tab**: its folder on the right, **+** opens another,
  **⇱** sends it to an editor tab (**at the bottom** in the tab brings it back), **×** on its hovered
  tab closes it.
- All the views (Search, Git, Pull requests (GitHub), Claude, Terminal) have the same buttons at
  the right of their header, in the same order: **⟳** read again, **in a tab / at the
  bottom** (a small window: with a tab on top, or a band at the bottom; remembered for next time), **⋯** the other actions
  and options. The layout buttons all look alike: the four corners maximize a panel in its view
  (Escape to come back), a window with a column on the right shows or hides a side panel (the
  review of Claude), filled when it is shown. Git and the Preferences are
  tabs like the others: Escape goes back to the previous tab, Ctrl+W closes them.

## Languages: installing the tools

**Preferences → Languages** (right click on the gear of the status bar, Tools menu, or
“Languages: install the tools of a language” in the palette) lists
each language with its tools: language server (LSP), linter, formatter, debugger. ✓: installed;
✖: missing.

**Each language has its page**, as in Zed: click its row, or its name in the column on the left
(under “Languages”). It gathers everything of its own:

- its **tools** (and “Syntax · tree-sitter”); its **language server** and its **lint**, which can be
  turned off for it alone; its linters (for Python: the chosen linter, its arguments, the codes
  ignored by flake8, ruff and pylint; for JavaScript / TypeScript: ESLint's; shellcheck, yamllint,
  markdownlint… for the others);
- **its settings**, which replace for its files those of Editor and Appearance: tab width, indent
  with spaces, word wrap, vertical rulers, visible whitespace, indent guides, colored brackets,
  spelling, auto-close brackets, format on save, trim trailing whitespace, final newline. Each row
  says the value of all the languages; changed, it is marked **its own**, and **↺** puts it back
  like all. For instance: 2 spaces for JavaScript, word wrap for Markdown, format on save for Rust
  only. Alt+Z changes the one of the file's language when it has its own.

In settings.toml: `languages = { "JavaScript / TypeScript" = { tab_size = 2 }, Markdown = { word_wrap = true } }`
(the pack's name; the same keys as the general settings, plus `lsp` and `lint`).

The page of all the languages keeps what applies to all: the automatic install and the kinds of
tools to install, the language servers (general switch), when to check (while typing, on save), the
linters declared in settings.toml and the trusted folders.

- **Install** puts what is missing in Florm's tools folder, without touching the system: `pip` in a
  virtual environment of its own (pylsp, ruff, debugpy, yamllint, clangd…), `npm`
  (typescript-language-server, eslint, the YAML, Bash, HTML, CSS, Dockerfile servers…), `go install`
  (gopls, dlv), or the ready-made program of their GitHub release (rust-analyzer, marksman, hadolint,
  lua-language-server). **Nothing else to install**: without Python, Node.js or Go on the computer,
  Florm downloads its own the first time (in its folder, checked by its SHA-256 sum). A click on a
  ✖ tool installs that one alone.
- What only the system installs (gdb, Ruby, luacheck) is given as commands for your system: a click
  types it in a terminal (Enter runs it), a right click copies it.
- **By themselves**: when a file of a language that misses tools opens, Florm installs them in the
  background, without asking (once per session; the status bar says how it goes). The file opens at
  once, colored; the language server's completion and the lint come when the install is done. Only
  that language's tools are installed, and only those checked at the top of the page: language
  server, linter, formatter (by default), debugger (else installed at the first **F5**, then the
  session starts by itself). An unchecked tool is still installed by a click on its chip. If it fails (or if the « Install by themselves » box is
  unchecked), a bar above the file proposes to install them; **Don't ask again** stops it for that
  language.
- **Syntax · tree-sitter** (Python, JavaScript, TypeScript, Rust, Go, C, C++, JSON, Shell, CSS,
  HTML, YAML, XML, TOML): the highlighting comes from the file's **syntax tree**,
  as in Zed, instead of regular expressions: it understands the code (calls, builtin functions,
  strings and their escapes…), and Alt+Shift+→ grows the selection one logical block at a time. The
  tree follows each key without reading everything again (under a millisecond on a 7,000-line
  file). The grammar is built into Florm, on every system; a click on the pack's line turns it on
  or off.
- **+ New pack…**: a language Florm does not know, or other tools for a language, in a TOML file of
  `~/.config/florm/packs/` (a pack with the same name replaces Florm's). A pack can declare a
  language server (`cmd` of an `lsp` tool) and a linter (`cmd` of a `lint` tool, writing
  `file:line:col: message`): Florm runs them for its files, in a trusted folder.
  ```toml
  name = "Nim"
  ext = ["nim", "nims"]
  [[tool]]
  kind = "lsp"
  bin = "nimlangserver"
  cmd = "nimlangserver"
  hint = "nimble install nimlangserver"
  ```

## Errors in the code

Problems are underlined in the code and listed in **Problems** (bottom panel):

- **Python**: the first linter installed (`flake8`, else `ruff`, `pyflakes`, `pylint`; Preferences →
  Lint), while typing. **JavaScript**: `eslint`, on save.
- **Language servers** (Preferences → Languages → Language servers): their errors too, for every
  language they know (TypeScript, Rust, Go, C…). **Format the file** (Ctrl+Alt+F, palette) goes
  through `ruff` for Python; in a web project that uses **Biome** or **Prettier** (their
  configuration file, or the tool in `node_modules`), through them, with the project's
  configuration; else through the language server. Nothing is formatted by a tool the project did
  not choose. To format on every save: Preferences → Editor (or per language).
- **XML and JSON**: a badly formed file (unclosed or badly nested tag, missing quote), live.
- **The other languages**, while typing, with their linter when it is installed: `shellcheck`
  (shell), `yamllint`, `markdownlint`, `hadolint` (Dockerfile), `luacheck`, `rubocop`, `stylelint`
  (CSS). Any other one is declared in `settings.toml`: a command writing `file:line:col: message`
  lines (the text of the file comes on its standard input, `{file}` is replaced by its path). An
  empty command turns off a linter of Florm, the same key replaces it:
  ```toml
  [linters.nimcheck]
  cmd = "nim check --stdin"
  ext = ["nim"]
  [linters.markdownlint]
  cmd = ""
  ```
  For another output, `pattern`: a regex with the groups `line`, `col`, `msg`, `severity`.
- **Language servers of every language**: Florm finds by itself those of YAML, Bash, CSS, HTML,
  Lua, Ruby, PHP, Markdown (`marksman`), TOML (`taplo`), Dockerfile, Terraform, Java, Kotlin, Swift,
  Zig, C#, Haskell, OCaml, Elixir when they are installed (the status bar says which one to
  install). Another language is declared with the extensions of its files:
  ```toml
  [lsp_servers]
  nim = { cmd = "nimlangserver", ext = ["nim", "nims"] }
  ```

**🔒 Restricted mode.** A language server, eslint, the flake8 plugins declared by a project or the
configuration of a git repository (`.git/config`, filters of `.gitattributes`) may run its code. In
a folder not trusted yet, a bar above the file says so: Git, the language servers, eslint and the
linters of the other languages do not run, and flake8 ignores the configuration of the project (`node --check` and the Python lint stay);
the Git view says so too, with the same button. **Trust “folder”** starts them, for that folder and those inside it
(`trusted_folders` key of `settings.toml`); **Later** hides the bar until Florm is closed.
**Preferences → Languages → Trusted folders** lists them; × takes the trust of a folder back, and it
goes back to restricted mode. The repositories Florm clones itself (the shared
configuration, the extension catalogues) do not need to be in it.

## Tests (pytest, jest, cargo, go)

The tests of any project run in the terminal of the bottom panel:

- **pytest** for `test_*.py` and `*_test.py` (the `pytest` of the project's `.venv` if there is one),
  **jest** (or **vitest**, if `package.json` asks for it) for `*.test.js`, `*.spec.ts`…,
  **cargo test** for the `#[test]` of a Rust file, **go test** for `*_test.go`.
- The **▶** in the gutter of a test runs it; **Ctrl+Shift+F6** runs the test under the cursor,
  **Ctrl+F6** the last one again. The command is shown as it is (`pytest tests/test_a.py::TestX::test_y`,
  `cargo test app::calc::tests::adds`…): in “Tests: run tests…”, it can be changed before Enter.
  A project in a sub-folder starts with `cd sub-folder && `.
- The output shows in the bottom panel, the summary (✔ or ⚠, passed and failed) in the status bar.
- An `ipdb` in a test stops in that terminal, where it is typed as usual.

## Debugging (F5)

Florm's debugger speaks the protocol of debuggers (DAP), like Zed and VS Code: **debugpy** for
Python, **dlv** for Go, **lldb-dap** or **gdb** (14 or later) for Rust, C and C++.

- **Breakpoint**: a click in the column on the left of the line numbers (a faint red dot shows it
  on hover), or **F9** on the line. They follow the lines when the text changes.
- **Conditional breakpoint**: **right click** on the dot (or **Shift+F9**): a condition
  (`partner.id == 42`, `len(lines) > 3`) stops only when it is true; `log: total = {total}` makes
  it a *logpoint*, which writes the text in the console without stopping. The dot turns orange, its
  condition shows on hover; empty, it is a plain breakpoint again.
- **Watched expressions**: the **WATCH** section, above the variables, evaluates its expressions
  again at each stop (field “+ Expression to watch”, × to take one away). Also “Debugger: watch the
  selection” in the palette.
- **A command, as in a terminal**: the **Debugger** view (🐞 in the status bar) opens on **Command**
  (the list at the top left); type the line that starts your program (`python app.py --port 8000`, `python -m flask run`,
  `pytest tests/test_a.py`, `go run .`, `./my_program --verbose`, variables in front like
  `DEBUG=1 ./my_program`, the Python of a virtual environment like
  `~/venvs/app/bin/python app.py`) and its **folder**: empty, the project; a relative path starts
  from the project; **📁** opens the system's folder chooser. A folder that does not exist shows in
  red (hovering says which) and F5 refuses it. The program runs with the **same Python as in a
  terminal**: at startup Florm reads the environment of your shell (pyenv, nvm, `~/bin`… of
  `config.fish`, `.zshrc`, `.bashrc`), even started from the desktop; for a script like `./odoo-bin`,
  it is the folder's `.venv` if there is one, else the Python of its first line
  (`#!/usr/bin/env python3`) found in that PATH. Then **Enter** or **F5**. Florm picks the debugger from the command; empty, F5 debugs the open file. Each command run stays in **🕘**, per project; the
  last one comes back when the project opens again. A wrapped command (`make`, `docker`…) cannot be
  debugged directly: start the program with `debugpy --listen` and attach to it (see below).
- **F5** starts what is chosen in the **Debugger** view (bottom panel): the **command**, the **open file**, the
  **test under the cursor** (pytest, go test, cargo test; also “Debugger: debug the test under the
  cursor” in the palette) or a configuration of the project. The changed files are saved first.
  Rust: `cargo build` first, then the binary of the package.
- Stopped, the line is on a yellow background with an arrow in the gutter. **F5** goes on, **F10**
  goes to the next line, **F11** steps into the function, **Shift+F11** out of it, **Shift+F5**
  stops, **Ctrl+Shift+F5** restarts (also the buttons ▶ ⏸ ↷ ↓ ↑ ⟳ ■ of the panel).
- The **Debugger** view: the **call stack** (a click shows the line of a call and its variables),
  the **variables** (▸ opens a list, an object), the **console**: the output of the program, and an
  expression typed at the bottom (Enter) is evaluated where it is stopped.
- **Configurations of the project**: “Debugger: configurations of the project” creates and opens
  `.florm/debug.toml`, with examples. One per `[[config]]`: `adapter` (python, go, lldb, gdb),
  `program` or `module` (Python), `args`, `cwd`, `env`, `build` (a command before, like
  `cargo build`); `{file}`, `{dir}` and `{root}` are replaced. **Attach** to a Python program
  already running, in a container for instance (`python -m debugpy --listen 0.0.0.0:5678 …`):
  `request = "attach"`, `port = 5678`, `path_mappings = { "/app" = "{root}" }`.
- debugpy missing: “Debugger: install debugpy” (palette) installs it for Florm only. Like the
  language servers, the debugger only runs in a trusted folder.

**Ctrl+click** on a `File "…", line N` line of a traceback opens the file.

## Templates

**Ctrl+Alt+N** (or Tools → Insert a template): the templates of the type of the current file
come first. Florm ships a few: `ipdb`, `pdb`, `ifmain` (Python), and for Odoo `omodel` (a model),
`om2o` (a Many2one field), `ocompute` (a computed field and its method), `oform` (a form view, in
XML): type the word then Tab; add your own (see below).

The text of a template follows the snippet syntax of VS Code: `$1`, `$2`… are the places to fill,
in order: **Tab** goes to the next one, **Shift+Tab** back, Escape stops. `${1:text}` puts a text
already selected there, `${1|a,b|}` the first choice; the same number at several places is typed
everywhere at once. `$0` marks the place of the cursor at the end. `$TM_FILENAME_BASE` gives the
name of the file without its extension; `\$` writes a `$` (the other `$NAME` stay as written). The
suggestions of the language servers that are snippets (the arguments of a function) work the same
way.

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

## Extensions

An extension adds commands to the palette (with a shortcut if it wants) and reacts when a file
opens, is saved, closes or is shown. It is a program, in Python or any language, in a folder of
`~/.config/florm/extensions/` with its `extension.toml`. It runs with your rights: only install
some from sources you trust. Without Python on the computer, Florm downloads its own at the first
command of a Python extension (only once, ~30 MB).

- **Preferences → Extensions** (right click on the gear of the status bar, or Tools menu): the list (a checkbox turns one off), **+ New extension…** makes a
  Python example and opens it, **⟳ Reload** after a change, **📁** the folder.
- Their commands are in the palette, after ✚. What they write (`print`) and their errors go to
  **Output**.
- `extension.toml`:
  ```toml
  name = "My extension"
  cmd = "python3 main.py"        # started in its folder, when first needed
  events = ["save"]              # open, save, close, focus
  [[command]]
  id = "upper"
  title = "Upper-case the selection"
  key = "ctrl+alt+u"
  ```
- `main.py`, with the library Florm gives it (`florm.py`):
  ```python
  import florm

  @florm.command("upper")
  def upper(ctx):                # ctx.file, ctx.root, ctx.selection, ctx.line, ctx.col
      florm.replace_selection(ctx.selection.upper())

  @florm.on("save")
  def saved(ctx):
      florm.status(f"{ctx.file}: {florm.text().count(chr(10))} lines")

  florm.main()
  ```
- What an extension can do: `status`, `message` (info, warning, error), `output`, `text()`,
  `replace_selection`, `insert`, `set_text` (undone by Ctrl+Z), `open(path, line)`,
  `pick(list, title)` (the index chosen, None on Esc), `input(title)`, `run("save")` (a command of
  Florm, the identifiers of `[keys]`), `terminal("make test")`.
- In another language: Florm talks to it in JSON-RPC on its input and output, like to a language
  server (`Content-Length` header); the methods are those of `florm.py`.

**Catalogue** (Preferences → Extensions, at the bottom; or “Extensions: installed and catalogue” in
the palette): the extensions and language packs shipped with Florm, those of your shared
configuration (its `extensions/` and `packs/` folders), and those of the catalogue repositories you
add (`owner/repo` or a git address: a folder per extension, and `packs/*.toml`). **Install** copies
into your folders, **Update** when the catalogue has another version (the state the extension keeps
stays), **Uninstall** removes. An extension that does not come from Florm asks to confirm: it will
run with your rights. **⟳ Update the catalogues** fetches them again.

**The Odoo extension** (in Florm's catalogue):
- **Ctrl+Alt+O**: runs the test under the cursor (`--test-tags /sale:TestOrder.test_confirm`, the
  class, or the module outside a test) in a terminal. The first time, the command shows to be
  completed (the database `-d`, a `docker compose exec …` in front); then only the tags change.
  “Odoo: change the tests command” to change it.
- **Alt+F12**: on `'sale.order'`, goes to the model (`_name`, then its `_inherit` to choose from); on
  `'module.xml_id'` or `ref="…"`, goes to the record (XML, CSV).
- “Odoo: update the module (-u)”, “models and records of the file”, “open the module's
  `__manifest__.py`”.

## GitHub

Florm goes through the `gh` command: no token to create, you only need to have logged in once
with `gh auth login` (login through the browser). Without `gh`, the view explains how to install
it (your system is detected) and comes back by itself as soon as it is found.

### Pull requests

**Ctrl+Alt+R** (or Git → GitHub pull requests, **⋯ → GitHub pull requests** in the Git view, the
palette).

- Each row of the list: the author's initials (the dot on them says whether the pull request is
  open, a draft, merged or closed), the title on two lines at most, `#number · branch · age`, and on
  the right the state of the review (**👍** approved, **✖** to fix, **👁** review requested); the
  checks are in the open PR (the list loads much faster that way).
- On the left, the PRs of the repository (open, merged, closed, yours), with a filter. At the
  start, **yours** (preference “Author shown at the start” to see all of them).
- A PR you just created opens as soon as GitHub knows it (a few seconds).
- **The view is for reading**: to write (comment, reply, approve, resolve a conversation),
  **Open on GitHub** in the header. Claude's help stays (✳ AI review, 🗺 Explain, ✳ Send to
  Claude), and so does applying suggestions.
- **Conversation**: the description on top, then everything that happened from the oldest to the
  newest: the comments and reviews (in a box “bob commented 2 days ago”), the **comments on the
  code** (clickable file:line, the commented code, the replies) and the commits (a click shows
  them). At the bottom: whether the PR can be merged.
- **Handle the comments with Claude**: tick the unresolved comments to handle (box on the
  left of each, in Conversation or Files; “All” at the top), then **✳ Send to Claude**. A
  Claude conversation opens in the repository with each comment (file:line, commented code,
  replies) and changes the code; you check it in its review. You first need **⤵ Checkout** of
  the branch of the PR (button in the header).
- **Apply the suggestion**: a comment with a `suggestion` block replaces the lines in your file
  and saves it (Ctrl+Z in the file to undo). You first need **⤵ Checkout** to have the branch
  of the PR (button in the header).
- Branch of the PR checked out: the unresolved comments are **in the editor**, a small bubble in
  the margin, the text on hover.
- **Commits**: a click shows the whole message of the commit and its changes alone;
  **double-click** (or **Show in the Git view ↗**, or right click) opens it selected in the
  graph of the Git view (the commits of the PR are first fetched into `remote/pr/N` when they
  are not in your repository). **Files**: the whole diff of the
  PR, the conversations **under the commented lines** like on GitHub (apply a suggestion), and
  **double-click a line** to open the file there.
- A box before each file marks it **viewed**: it folds and GitHub remembers it for your review
  (“3/10 viewed” on top).
- **✳ AI review**: Claude reads the PR over and proposes remarks under the lines of the diff;
  nothing is sent. Each one is **kept**, edited (✏) or dropped (×), then **Publish** sends the
  kept ones as one GitHub review. **💬** on a remark talks it over with Claude to refine it
  (“shorter”, “in English”, “are you sure? look at `_compute_total`”): it answers under the remark
  and rewrites it when needed, **↶ Previous version** goes back. Also in the Git view, for the
  changes not committed yet.
- **🗺 Explain** (header of the PR): Claude explains the PR idea by idea, in the Claude tab (see below).
  **🗺 Explain this comment**, under each conversation: Claude rephrases what the reviewer asks,
  says whether they are right and why, and what it would change.
- At the bottom of the conversation, the merge box, in the color of the state (red blocked,
  orange something missing, green ready): where the PR stands (**Review required**,
  **Conflicts**, **Behind**, **Ready to merge**…), the **checks** (the failing ones always shown,
  a click for all; **Logs** opens the log of a GitHub Actions job in a tab, **↻ Rerun the failed
  ones**), and the buttons: **Merge** (merge commit, squash or rebase, delete the branch), **⟳ Update the
  branch** (merge or rebase of the base), **Ready for review** / **Back to draft**, **Close** /
  **Reopen**. Merge, close and rebase ask first.
- On the right, for information: the reviewers with their verdict (✔ approved, ✖ changes
  requested, 💬 commented, ⏳ waiting), “Review required” when the repository asks for an approval,
  and the assignees. They are changed on GitHub, like the labels and the milestone.
- Right-click a PR of the list: open, open on GitHub, checkout, copy the link or the number. In
  the open PR, **Open on GitHub** and **⤵ Checkout** are buttons of the header, and **⋯** copies
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
  Claude (the conversation goes on where it was).
- **The conversation shown** grows in the list on the left: its **context** in % and its
  **cost**, its **model** (and its account when there are several), read in the status line of
  Claude Code. Florm reloads the files and refreshes Git as soon as Claude changes them.
- **Usage**, at the bottom of the list: the **5 h** and **7 d quotas** of your subscription (on
  hover, when they reset).
- **What Claude waits for**: when it asks for a permission or a question, its message (“needs your
  permission to use Bash”…) shows on hover over the conversation in the list and over **❓ waiting**
  in the status bar. When Florm is not in front, a notification of the system tells you at
  once, and also when Claude is done, even while Florm's window is hidden (macOS; Windows; Linux
  with `notify-send`). **A click on the notification** brings Florm to the front, on that
  conversation: on Windows and Linux (Hyprland, Sway; elsewhere Florm asks for it itself, which
  depends on the desktop), and on macOS when `terminal-notifier` is installed
  (`brew install terminal-notifier`).
- **Several conversations, one tab**: every conversation is listed on the left of the Claude
  tab, the newest at the top, with its title (its first message), what it does (⟳ working, ❓
  waiting for your answer, ✔ done) and how long it has been working; below, its project in its
  color and its branch. **×** on a hovered row closes it. A click shows one, the others go on behind; **+** starts a new
  one. Right click: **⇉ Duplicate** (a copy of its session, to try something else), **⇱ Open in
  its own tab**, **× Close** (Claude stops, it stays in the history). **Ctrl+Alt+W**: the next one
  waiting for your answer. The status bar says how many are working or waiting.
- **Sandbox** (Preferences → Claude, on by default; a **green shield** in the bar of the
  conversation and on Claude's icon says so, « ⚠ No sandbox » in orange otherwise; a click: the
  setting): Claude's commands run isolated, they write
  only in the project and the network is filtered. Claude cannot run **any command out of the
  sandbox**, even in auto mode, and it has **neither your keys nor your tokens**: `~/.ssh`,
  `~/.gnupg`, the token of `gh`, `~/.aws`, `~/.kube`, `~/.netrc`, `~/.git-credentials`, Claude's
  login… are not readable, and your SSH agent is not given to it. What needs them (a `git push`,
  `ssh`, `gh pr create`…), Claude hands over to you: **Florm types
  the command in the terminal of the bottom panel**, without running it; you check it and press
  **Enter** (or erase it). It is typed in Claude's folder, and when it ends, Florm tells Claude whether
  it succeeded, with the last lines of the terminal: it goes on without you having to write it. Linux: `bubblewrap` and `socat` packages;
  macOS: built in (Seatbelt); Windows: not available. Off, there is no sandbox,
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
- **✳ Send to Claude** from a pull request (its comments) or the bar of a conflicting file: a
  conversation in the repository, which starts with the request.
- **Images**: a capture made with the system's tool (**Print**; **Cmd+Ctrl+Shift+4** on Mac,
  **Win+Shift+S** on Windows, recalled in + Context) goes to the clipboard; **Ctrl+V** in
  Claude (Cmd+V on Mac), or **+ Context → Image from the clipboard**, gives it to Claude. An
  image or a PDF can also be dropped from the
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
  **commit message rules** (the project's, else yours) and the branch you work on.
  Claude then writes its commits like ✨. To teach it how to run your tests (a container, tags…), a
  Claude Code **skill** does it better: it is only used when useful.
- **My skills** (Preferences → Claude): a folder of yours for all your skills (a GitHub
  repository of its own, say), given to every conversation of Claude whatever the account: no
  project has to hold them. **Choose…** sets it; arrange it as you like, every `SKILL.md` found is
  a skill, even in sub-folders by theme. Claude sees them as `florm:name`. **+ New skill** creates
  a `SKILL.md` to fill in (its `description` tells Claude when to use it). Without a folder chosen:
  `skills/` of the shared configuration.
- **AI completion** (Preferences → Editor, or the palette: “AI completion while typing”): when
  you stop typing at the end of a line, **GitHub Copilot** suggests what follows in grey after the
  cursor, usually in under a second (Copilot plan). **Tab** takes it, **Escape** or another key
  ignores it; typing its first letters keeps the rest. **Alt+\\** asks for a suggestion at once,
  even when turned off. **Install Copilot** downloads its official server (about 160 MB) into
  Florm's folder, without npm nor admin rights; **Sign in to GitHub** copies a code and opens the
  page to type it in, once and for all.
- **Closing Florm while Claude works**: Florm says so and offers **Close when it is done**
  (Florm closes by itself at the end; “Don't close” to stay) or **Close now**. An update also
  waits for Claude to be done before restarting Florm.

**✨ next to the commit message** (Git view): Claude suggests the message from the staged
changes (else all of them), in the style of the last commits of the repository and from your
first request in the Claude tab.

### ✳ Change it here (Ctrl+I)

Select lines (or put the cursor on a line), **Ctrl+I**, write what to change (“add an
@api.constrains that refuses a negative share”) and **Enter**. Claude rewrites only those lines,
from the rest of the file; its text takes their place, on a green background: **✓ Keep**
(Ctrl+Enter), **× Undo** (Escape: the old text comes back), **↻ Ask again** (the old text comes
back and the request opens again). Hovering **Old** shows the text before. Going on typing keeps the
change; Ctrl+Z undoes it like the others. For a real task, on several files, the Claude tab stays
the right place.

### 🗺 Explaining a change

To understand **what Claude proposes and why**, idea by idea rather than file by file.
**🗺 Explain** in the Git view (next to ✳ AI review), or the palette (“Git: explain the uncommitted changes”):
the request goes to the Claude tab, to the conversation working in that folder (it knows why it
made those changes), else to a new one. Claude gives the problem and the idea, then each idea with
the places in the code (path:line, clickable) and what could break; it changes nothing. The talk
goes on with it as usual.

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

The images of the preview are `http(s)` addresses or files of the project (a relative path, 20 MB
at most); a symbolic link, a file outside the project or an absolute path is not read, its text
shows instead. In a pull request or a comment, only `http(s)` images are kept. An SVG image never
reads another file of the disk (its `<image>` embedded as `data:` show). Links (Markdown, pull
requests, Claude, terminal) only open in the browser when they are `http` or `https`.

## Writing

- **Spelling**: the unknown words of the Markdown, the comments of the code and the commit
  messages are underlined with blue dots, in French and English at once (the proper nouns, the
  code, the addresses and the identifiers are left out). **Ctrl+.** on a word offers its
  corrections, or **adds it to your dictionary** (`dictionary.txt`, in the shared configuration
  if there is one). The dictionaries (Grammalecte for French, SCOWL for English) are downloaded
  the first time, about 550 KB. Turned off in Preferences → Editor → Spelling.
- **Alt+Shift+→** expands the selection step by step: the word, the inside of the string, the
  string with its quotes, the inside of the parentheses (brackets, braces), with them, the line,
  the block of the same indentation, the whole file. **Alt+Shift+←** goes back one step. In a
  language that has its **syntax tree** (Python, JavaScript, TypeScript, Rust, Go, C, C++, JSON…), each step follows the code: the name,
  the argument, the argument list, the call, the statement, the block, the function, the class.
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

## Shared configuration

The preferences, shortcuts, templates, themes and Claude skills can come from a GitHub
repository, the same on every computer: **Preferences → Advanced → Shared configuration**, with
your login (the `login/florm-config` repository), `owner/repo` or a git address. **Create on
GitHub** makes the repository (private) from this computer; **Publish** sends your changes,
**Synchronize** fetches those of the others (also at each startup). What is specific to a
computer (the commands of the tests, the language servers and the linter, the trusted
folders, the Claude accounts…) is never shared.

## Appearance

The interface follows Zed's design: the panels, the tab strip and the menus a step lighter (darker on a
light theme) than the editor, the title bar and the status bar a step beyond, thin lines between the
parts, the active tab on the editor's background. These shades come from the chosen theme: they follow
every theme.

The text is in IBM Plex, built into Florm: the same font on Linux, Windows and macOS. On a screen of
low density (a 1080p laptop at 100 %), Florm takes its « Text » weight, a little fuller, so that the
text does not look thinner and greyer than on a Mac's Retina screen.

**Preferences → Appearance**: language (Français / English), theme (clickable previews: Florm and
Florm Light, Florm Minimal and Florm Minimal Light, more sober (a deep or cream background, a
soft palette), and after the most used themes of VS Code and Zed: Florm Monokai, One Dark, Dracula,
GitHub Light, Catppuccin Mocha and Latte, Tokyo Night, Gruvbox Dark and Light, Nord, Solarized Dark and Light, GitHub Dark, One Light, Rosé Pine, Ayu Dark, Night Owl, Everforest Dark, Kanagawa),
accent color, **softened text** (less “bold” text, useful on light themes), fonts of the editor
and of the interface, minimap, breadcrumbs. The menus are under the **F** at the top left (as in Zed,
without a menu bar), and their commands in the palette, **Ctrl+Shift+P**. The folder icon before the
project's name opens the list of projects.

Taken from Zed, in **Appearance → Cursor and gutter / Scrolling**: cursor shape (bar, block,
underline), blinking cursor, current line highlight, **selection occurrences** (the other places of
the selected text framed), line numbers and **relative numbers** (counted from the cursor's line),
indent guides, scroll past the end of the file, margin of lines around the cursor. In **Editor**:
completion while typing, documentation on hover, **smart case** (a search with a capital letter is
case sensitive), final newline on save, delay of the auto-save, ask before quitting. In **Git**: the
change marks in the gutter, and the line blame from the start.

**Theme colors → Create a theme from “…”**: all the colors of the editor (background, text,
accent, cursor, selection, gutter…) and of the code (comments, keywords, strings, functions,
classes, tags…, italic or not), applied right away. **Save the theme** writes it in the
`themes` folder of the configuration; **Edit this theme** / **Delete this theme** when it is
one of yours.

**Git can be turned off too** (Preferences → Git), and it is by itself when the `git` command is
not installed: no more Git view, history, blame, change marks in the gutter nor pull requests, and
`git` is no longer called. Its section then tells how to install it.

**GitHub and Claude can be turned off**: a switch on top of their section of the
preferences (Pull requests, Claude). Off, their views, commands, menus, icons,
shortcuts and buttons go away (✨ of Git, AI review, ✳ column of the
margin, linked PRs in the Git view…), and `gh` is no longer called. The same switch brings them
back, without restarting.
