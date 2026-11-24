# Florm
[Français](README.md) · **English**

Light and fast text editor in Rust, inspired by Sublime Text.

- **GPU rendering** (egui/glow): only the visible lines are drawn, whatever the size of the file.
- **Rope buffer** (`ropey`): insertions and deletions stay fast on files of several MB.
- **Incremental highlighting** with the Sublime Text grammars (`syntect`).
- A single binary (~17 MB on Linux, no system dependency apart from OpenGL).

## Installation

All the files are in the latest [release](https://github.com/mva-odoo/florm/releases). Replace `<version>` with the release number (e.g. `0.1.0`).

Florm needs nothing but OpenGL to display. The advanced features (Git, GitHub, Claude, linters…) use external programs: see [Tools and connections](#tools-and-connections).

### Linux

Florm works on X11 as well as on Wayland. The packages install `florm` in `/usr/bin`, a “Florm” entry in the applications menu and its icon.

#### Debian, Ubuntu, Linux Mint, Pop!_OS

```bash
sudo apt install ./florm_<version>_amd64.deb
```

`apt` also installs the graphics libraries needed (OpenGL, Wayland, X11). Uninstall: `sudo apt remove florm`.

#### Fedora, openSUSE, RHEL

```bash
sudo dnf install ./florm-<version>.x86_64.rpm        # Fedora, RHEL
sudo zypper install ./florm-<version>.x86_64.rpm     # openSUSE
```

#### Arch Linux, Manjaro, EndeavourOS, Omarchy

```bash
sudo pacman -U florm-<version>-1-x86_64.pkg.tar.zst
```

Uninstall: `sudo pacman -R florm`.

#### Other distributions: binary only

```bash
chmod +x florm-<version>-linux-x86_64
mv florm-<version>-linux-x86_64 ~/.local/bin/florm
```

You need the OpenGL libraries (`libGL`, `libEGL`) and `libxkbcommon-x11`, almost always already there on a desktop.

### Windows

**The simplest, with no security warning and no admin rights**: in PowerShell (Start menu → “PowerShell”), paste

```powershell
irm https://raw.githubusercontent.com/mva-odoo/florm/main/packaging/install.ps1 | iex
```

The script downloads the latest version, checks it (SHA-256) and installs it in `%LOCALAPPDATA%\Programs\Florm`, with **Florm** in the Start menu and the `florm` command in the terminal. Uninstall: Settings → Apps → Florm. Running the command again updates.

Without PowerShell, the release also contains:

- `florm-<version>-windows-x86_64.msi`: classic installer (double-click), same location, same uninstall;
- `florm-<version>-windows-x86_64.exe`: the program alone, **standalone**, to put wherever you want.

These two files are not signed: downloaded by the browser, **Windows SmartScreen** shows “Windows protected your PC” at the first launch. Click **More info** then **Run anyway**. The PowerShell script avoids this message: SmartScreen only checks the files marked by the browser.

The preferences are in `%APPDATA%\florm`. To get Git, GitHub, Python… see [Install everything on Windows](#install-everything-on-windows).

### macOS

**The simplest, with no warning and no `xattr`**: in the Terminal, paste

```bash
curl -fsSL https://raw.githubusercontent.com/mva-odoo/florm/main/packaging/install.sh | sh
```

The script downloads the latest version (Intel Mac and Apple Silicon), checks it (SHA-256) and installs **Florm** in Applications (`~/Applications` if the folder cannot be written to). Running the command again updates.

Or by hand: download `Florm-<version>-macos-universal.dmg`, open it and drag **Florm** into **Applications**. Florm is not signed by an Apple developer account: macOS quarantines what the browser downloads and may say that the application “is damaged”. You then have to remove the quarantine once:

```bash
xattr -dr com.apple.quarantine /Applications/Florm.app
```

The script does not need it: `curl` does not set the quarantine. Neither do the built-in updates (⬆ Update x.y.z).

### From the sources

With [Rust](https://rustup.rs) installed:

```bash
git clone https://github.com/mva-odoo/florm.git
cd florm
cargo build --release
./target/release/florm .
```

### Updating

When a new version is out, Florm says so at startup with a **⬆ Update x.y.z** badge at the bottom right. A click shows what changes (the new features, fixes and improvements since your version) and offers **Install and restart**: Florm downloads the version for your system, checks it (SHA-256) and replaces the installed one, then **⟳ Restart** starts it again. With a Linux package (`.deb`, `.rpm`, Arch), the system window asks for the password, as when installing a package. If that is not possible, the menu still offers to **download the file in the browser** (the `.deb` on Ubuntu, the Arch package on Arch, the `.exe` on Windows, the `.dmg` on macOS…) or to open the release notes. To check by hand: **Help → Check for an update**. To stop checking: Preferences → Advanced → “Check for updates”.

No need to uninstall the old version: the new one installs **on top of it**, and the preferences are kept.

| System | Update |
|---|---|
| Debian / Ubuntu | `sudo apt install ./florm_<version>_amd64.deb` |
| Fedora | `sudo dnf install ./florm-<version>.x86_64.rpm` |
| openSUSE | `sudo zypper install ./florm-<version>.x86_64.rpm` |
| Arch | `sudo pacman -U florm-<version>-1-x86_64.pkg.tar.zst` |
| Binary only | replace `~/.local/bin/florm` with the new file |
| Windows | run the `irm … \| iex` command again, or open the new `.msi`, or replace the old `.exe` |
| macOS | run the `curl … \| sh` command again, or drag the new Florm into Applications → **Replace** |

### First launch

```bash
florm                # reopens the last project
florm .              # opens the current folder
florm file.py        # opens a file
```

The preferences open with **Ctrl+,** and are saved in `~/.config/florm/settings.toml` (`%APPDATA%\florm\settings.toml` on Windows). The built-in guide is in the Help menu. Git, GitHub, Claude and the debugger can be turned off (Git is by itself when `git` is not installed) (switch at the top of their section of the preferences, or `github`, `claude`, `debugger` = `false` in `settings.toml`): their views, commands and buttons disappear. They are off at installation; as long as they are, their section tells whether what they need is installed (`gh`, `claude`, Python and debugpy) and, if not, how to install it.

## Tools and connections

Editing, search, highlighting, the symbol index (F12, completion) and the XML / JSON lint work **without installing anything**. The other features rely on external programs, looked for in the `PATH`. None is required: without it, only the feature concerned is unavailable.

| Feature | Program | Required for |
|---|---|---|
| Git view, commit, push, diff, rebase | `git` | Everything to do with Git |
| GitHub pull requests | `gh` (GitHub CLI), logged in | Pull requests tab, “Create a PR” |
| Claude in the editor | `claude` (Claude Code), logged in | Claude tab, ✨ commit message |
| Completion by GitHub Copilot (optional) | nothing: Florm downloads `copilot-language-server` (Preferences → Editor); a Copilot subscription | Continuation offered in grey while typing |
| Tests | `pytest`, `npx jest`, `cargo`, `go` | Tests of the project, run by the ▶ in the margin |
| Spelling | nothing (dictionaries downloaded by Florm, ~550 kB) | Unknown words in Markdown, comments and commits, in French and in English |
| Compare, local history | `git` | Compare two files / folders, with the clipboard, with a version from the local history |
| Debugger | `debugpy` (Python), where the program runs; `gdb` 14+ or `lldb-dap` (Rust, C); `dlv` (Go); `node` (js-debug, downloaded by Florm) | Breakpoints, variables, stepping (F5); Florm offers to install it |
| Python lint | `flake8` or `ruff` (or `pyflakes`, `pylint`) | Underlines in Python files |
| Python formatting | `ruff` | Ctrl+Alt+F, “Fix automatically” |
| JavaScript lint | `eslint` (otherwise `node`) | Underlines in JS files |
| Smart completion, F2 rename | a language server | See below |
| Terminal icons | a Nerd Font | The symbols of the prompts (starship, powerline) |

### Install everything on Windows

In PowerShell, all at once (each line is optional):

```powershell
winget install Git.Git                  # Git (also needed by Claude Code)
winget install GitHub.cli               # Pull requests; then: gh auth login
winget install Python.Python.3.13       # Python lint
winget install OpenJS.NodeJS.LTS        # JavaScript lint, npm language servers
irm https://claude.ai/install.ps1 | iex # Claude Code; then: claude (to log in)
```

Then, **in a new terminal** (so that it sees the new `PATH`): `pip install ruff flake8`. Restart Florm after these installations: it reads the `PATH` at startup. The details of each tool are below.

### Git

Needed for the Git view (Ctrl+Alt+G), the colors of the file tree and the diff in the margin.

| System | Installation |
|---|---|
| Debian / Ubuntu | `sudo apt install git` |
| Fedora | `sudo dnf install git` |
| Arch | `sudo pacman -S git` |
| Windows | [git-scm.com](https://git-scm.com/download/win) (leave “Git from the command line” checked) or `winget install Git.Git` |
| macOS | `xcode-select --install` or `brew install git` |

Set your identity once, otherwise commits fail:

```bash
git config --global user.name "Your Name"
git config --global user.email "you@example.com"
```

To **push** to GitHub, you need an SSH key added to your account ([GitHub guide](https://docs.github.com/en/authentication/connecting-to-github-with-ssh)), or else `gh auth login` (below), which also sets up Git over HTTPS.

### GitHub: pull requests

The **Pull requests** tab (Ctrl+Alt+R), the PR badges and “Create a PR” go through **GitHub CLI** (`gh`). Florm asks for no token: it uses the `gh` login.

| System | Installation |
|---|---|
| Debian / Ubuntu | `sudo apt install gh` (or the [official repository](https://github.com/cli/cli/blob/trunk/docs/install_linux.md) for a recent version) |
| Fedora | `sudo dnf install gh` |
| Arch | `sudo pacman -S github-cli` |
| Windows | `winget install GitHub.cli` |
| macOS | `brew install gh` |

Then log in, once:

```bash
gh auth login
```

Choose **GitHub.com**, **HTTPS** or **SSH**, then **Login with a web browser**: copy the code shown, paste it on the page that opens and click **Authorize**. Check with `gh auth status`.

For repositories of an organization that uses SSO, also authorize the organization on the authorization page.

### Claude in the editor

The **Claude** tab (Ctrl+Alt+C) and the ✨ button (suggest a commit message) run **Claude Code** (`claude`). You need a Claude account (Pro / Max subscription or API key).

```bash
# Linux, macOS
curl -fsSL https://claude.ai/install.sh | bash
```

```powershell
# Windows (PowerShell)
irm https://claude.ai/install.ps1 | iex
```

Then log in, once, in a terminal:

```bash
claude
```

and follow the login in the browser (or `/login` in Claude Code). Once `claude` is logged in in the terminal, Florm uses it as is.

### Python: lint and formatting

The Python lint runs while you type. Florm takes the first one available: `flake8`, otherwise `ruff`, `pyflakes`, `pylint` (to choose in Preferences → Lint, `python_linter`). It is run from the root of the repository and so reads `setup.cfg`, `.flake8`, `tox.ini` or `pyproject.toml`.

The simplest: the **“Install flake8”** button of the Problems panel. Florm installs it in its own folder (`~/.local/share/florm/pytools`) without touching the system Python. You only need `python3` and `pip`.

Or by hand:

```bash
pipx install ruff flake8        # everywhere
sudo apt install flake8         # Debian / Ubuntu
sudo pacman -S flake8 ruff      # Arch
brew install ruff flake8        # macOS
```

**Formatting** (Ctrl+Alt+F, `format_on_save`) and “Fix automatically” use `ruff`.

### JavaScript

`eslint` of the project (`node_modules/.bin/eslint`) or of the `PATH`. Without eslint, Florm checks the syntax with `node --check`.

### Language servers (LSP)

Without a language server, Florm completes and goes to the definition thanks to its own index. A server adds exact completion, documentation on hover, usages (Shift+F12) and renaming in the whole project (F2). Florm detects automatically:

| Language | Server | Installation |
|---|---|---|
| Python | `pyright-langserver` or `pylsp` | **“install pylsp”** button of the status bar, or `npm i -g pyright`, or `pipx install python-lsp-server` |
| JavaScript / TypeScript | `typescript-language-server` | `npm i -g typescript typescript-language-server` |
| Rust | `rust-analyzer` | `rustup component add rust-analyzer` |
| Go | `gopls` | `go install golang.org/x/tools/gopls@latest` |
| C / C++ | `clangd` | `clangd` package (Linux), `brew install llvm` (macOS), LLVM (Windows) |

To choose another command, in `settings.toml`:

```toml
[lsp_servers]
python = "pyright-langserver --stdio"
```

### Built-in terminal

It runs your shell (`$SHELL` on Linux and macOS, `cmd.exe` or `%COMSPEC%` on Windows; to change in Preferences → Terminal). For the prompt icons (starship, powerline), install a [Nerd Font](https://www.nerdfonts.com): Florm finds it by itself.

### Open a WSL folder (Windows)

Command palette → **Open a folder of a WSL distribution (Windows)…** (or File menu → **Open a WSL folder…**). The list offers the installed WSL distributions; Florm then asks for the folder to open, starting from your home folder over there (Tab completes the path).

The folder opens as a normal project through `\\wsl.localhost\<distribution>\…`: files, search, Git and linters work on it, but they run on Windows. **The terminal**, on the other hand, opens a shell in the distribution, in the project folder (`wsl.exe --cd`). The files go through the WSL share: on a big project, the first indexing (Go to file, F12) takes longer.

### Check that everything is in place

```bash
git --version
gh auth status
claude --version
flake8 --version || ruff --version
```

## Features

| | |
|---|---|
| Folders | File tree on the left (right click: new, rename, delete — to the trash, or permanently —, show in the file manager… ; right click on the empty space under the files: new file or folder at the root), **drag and drop to move** files and folders, colors according to the git status. **With the keyboard**: Ctrl+Shift+E then ↑ ↓ ← → Enter (F2 rename, Delete). **Double-click on the edge**: the width fits the longest name (Ctrl+B hides the sidebar, click on the strip on the left to show it again). On the folder of a repository: its branch, the remote branch it tracks (↑ / ↓) and its remotes on hover. The sidebar only holds the file tree: Claude, Git and GitHub open in a tab, the bottom panel keeps the terminal, the problems, the output, and the search or the debugger when they are there (⇱ / ⇲ in their header, remembered). The bottom panel shows one view at a time (▾ to change it, Ctrl+Shift+J to show it); the status bar has the icons of Claude, Git, GitHub and the terminal, and otherwise only shows what is going on (branch ↑ ↓ ●, Claude working / ✓, debugging running / paused, problems). Right click on a folder → “Exclude from the index” (marked with a crossed-out eye: no longer in Ctrl+P, F12 nor the search) |
| Global search | Ctrl+Shift+F: the whole folder (respects `.gitignore`), regex / case / whole word, filters `*.py`, `models/*.py` (“models” folders wherever they are, subfolders included), `./path` or absolute path, and exclusions, replace with **preview** (old struck through in red → new in green), replace all / one file / a single occurrence, × to dismiss a result, collapsible files, F4 / Shift+F4 to go through the results. History of searches and filters (⏷ button, or ↑ / ↓ in the field), filters remembered across restarts. Results **highlighted as in the editor**, search in the bottom panel or **in a tab** (“in a tab” / “at the bottom” buttons) |
| Projects | Each folder opened becomes a project (Ctrl+Alt+P to switch, “Project: rename…” in the palette). The session is restored: open files and views (and their groups), cursor positions, split editor (which tab in which pane), bottom panel open, sidebar, expanded folders, search filters. Without an argument, Florm reopens the last project |
| Split editor | Two panes side by side or one above the other, each with its tabs: right click on a tab → “Split right / down”, or drag the tab onto the right / bottom edge of the editor (or into the other pane). Ctrl+\\ splits right. Tabs are reordered by drag and drop. Right click: move to the other pane, merge, change the orientation, close the others |
| Multi-cursor | Ctrl+D, Alt+F3, Ctrl+click, Ctrl+Alt+↑/↓, Ctrl+Shift+L, Alt+drag (column selection), paste spread over the cursors |
| Git | Git view (Ctrl+Shift+G, or click on the branch in the status bar), bar at the top: branch with ↑ commits to push / ↓ to pull, pull/push/fetch, branches, commit (Ctrl+Enter); the staged and modified files are the first lines of the graph. Git errors stay shown until you close them. Marks in the gutter |
| Inline diff | Green / yellow / red marks in the margin, updated while typing. Click on a mark (or Alt+F5 / Alt+Shift+F5): the old and the new version are shown under the block, with **Revert this change** (can be undone with Ctrl+Z) |
| Diff in the editor | Ctrl+Alt+D: added/modified lines on a green background, deleted lines shown again in red in their place, “↺ Revert” button per block. Diff tabs also have the green/red backgrounds |
| Blame & line history | Ctrl+Shift+B: author, relative date and message of the last commit of the line, shown at the end of the line (takes unsaved changes into account). Ctrl+Alt+B: all the commits that touched the line with their diff, and “Show in the graph” |
| Right click in the editor | Cut / copy / paste, all occurrences of the word, find the word in the folder, comment, blame and history of the line, show or revert the git change, copy the GitHub link of the line, the `file:line` reference. The file path and “Show in the file manager”: right click on the tab |
| Commit graph | Ctrl+Alt+G or Ctrl+Shift+G: history with branches and merges drawn; at the top, as in tig, the unstaged and staged changes; **below, like a GitHub pull request, the commit details then all its files and their changes one under the other** (↑ ↓: previous / next commit, ← →: previous / next file, Page ↑ ↓: scroll, click on a header: collapse it). In a diff: “Open the file” (double-click on a line: at that line), and per block or for the whole file: stage / unstage / revert the change (like tig; asks for confirmation when the change would be lost), tags and branches as labels, filter, commit details and modified files, diff of a file or of the whole commit, “Create a branch here”. Right click on a commit: checkout (of its branches or as detached HEAD), create a branch, copy hash / message |
| Several repositories | The git repositories inside the opened folder (up to 3 levels) are all detected; the Git view has a repository selector and automatically follows the repository of the active file |
| Linters | XML and JSON live (built in). **Python while typing** with flake8 (or ruff, pyflakes, pylint: `python_linter` in the preferences), run from the root of the repository to read `setup.cfg` / `.flake8` / `tox.ini` / `pyproject.toml`. Hover on an underline = message, right click = “Ignore (# noqa)”, **Ctrl+.** (or the light bulb) = quick fixes: import the missing name (project index), `# noqa`, `ruff --fix`, Claude, actions of the language server. **Codes to ignore per linter** (flake8, ruff, pylint, eslint) in Preferences → Lint. With no linter installed: “Install flake8” button in Problems (installed in `~/.local/share/florm/pytools`, without touching the system Python). JS via `eslint` → `node --check`. **Formatting**: Ctrl+Alt+F (`ruff format`, only the selected lines if there is a selection), “Fix automatically” (`ruff --fix`), formatting on every save in Preferences → Editor |
| Terminal | Ctrl+` (or Ctrl+Alt+T): real terminal as in VS Code (interactive shell in a pseudo-terminal: colors, Tab completion, ↑ history, `vim`, `htop`, `less`…). Several terminals (＋ at the top of the panel, the others in ▾), mouse selection, Ctrl+C interrupts (or copies the selection), Ctrl+V pastes, mouse wheel for the history. **Terminal in an editor tab** (Ctrl+Shift+`, or ⇱): it sits beside a file like any tab. Shell: `$SHELL` (or `cmd` on Windows), can be changed in the preferences |
| Templates | Ctrl+Alt+N: the templates for the current file type inserted at the cursor, with `${model}`, `${Model}`, `${module}`, `$0` filled in from the file. Triggers like snippets: `ipdb` + Tab → `import ipdb; ipdb.set_trace()`. Your own with Tools → New template… (from the selection, a one-word name serves as the trigger). **Preferences → Templates** to create and edit them |
| GitHub pull requests | Ctrl+Alt+R. Through `gh` (no token: `gh auth login` once): list of PRs, description, commits, diff, comments under the lines of the diff as on GitHub (also in the editor margin), reply, resolve, comment on a line, **suggestions applied** in the files in one click, checkout |
| Claude | Ctrl+Alt+C: **Claude Code itself in a terminal**, in a tab (`claude` command, no key), with the list of conversations on the left (⟳ working, ❓ waiting, ✔ done) and the session on the right (context, cost, quotas). ✳ in the margin, + Context and Ctrl+Alt+I (picture of Florm) write into its field. ✨ in the Git view: commit message suggested from the diff and the request |
| Tests | pytest, jest (or vitest), `cargo test`, `go test`: ▶ in the margin of a test, Ctrl+Shift+F6 under the cursor, Ctrl+F6 runs again; nothing to set up, the command shown can be changed in “Tests: run tests…” |
| Debugger | Like VS Code, for Python and Odoo (**on this computer or in a Docker container**), Node (js-debug), Rust and C (gdb or lldb-dap, after `cargo build`) and Go (delve); the default command comes from the project: `npm start`, `cargo run`, `go run .` (Debug view, F5): breakpoint with a click left of the line numbers (or F9), with a condition; the program stops there, the line is highlighted in yellow, the **variables** (expandable: `self`, its fields…), the **watch**, the **call stack** (click: this frame) and a **console** that evaluates Python in the paused frame (`self.env['res.partner'].search([])`). The value of a variable on hover. F10 / F11 / Shift+F11 stepping, F5 continue, F6 pause, Shift+F5 stop. **Launch**: the command (by default the `odoo-bin` of the folder) runs in the bottom panel. **Docker**: the debugger (debugpy) runs in the container through `docker exec`, no port to publish; the mounted files open here, those of the image read-only. **Attach** to an Odoo started with `python3 -m debugpy --listen 5678 …` (also in a container): **automatic, whatever the mode**, as soon as a breakpoint is set Florm waits for the program and attaches to it, at each run (“Copy the command” gives the line to run). **Right click on the ▶ of a test** (pytest, jest, cargo, go) or **Alt+F6** (an Odoo test too): debug this test. Exceptions: uncaught, all raised, leaving my code. Breakpoints are kept with the project |
| Help | Help → Florm guide: examples of search filters, Git, terminal, templates |
| Completion, definition, hover, usages | Like Sublime, without installing anything: the project is indexed in the background (classes, functions, methods, Odoo models `_name`, fields, XML records); F12 goes to the definition (several results: list of files; also works on `'sale.order'` or `'module.xml_id'`) and the suggestions offer the words of the open files and the symbols of the project. With a language server (pyright or pylsp — “install pylsp” button in the status bar —, typescript-language-server, rust-analyzer, gopls, clangd): smart completion, exact definition, F2 rename in the whole project. Command per language in `[lsp_servers]`. **Hover** on a symbol: its documentation (language server) or its definitions in the project. **Shift+F12**: its usages (language server, otherwise the whole word in the folder) |
| Symbols and breadcrumb | Ctrl+R: classes, functions, methods, Odoo records / templates, Markdown headings. Above the editor: `folder › file › Class › method`, clickable |
| Code folding | Arrows in the margin, Ctrl+Shift+[ / Ctrl+Shift+], Ctrl+K Ctrl+0 fold all, Ctrl+K Ctrl+J unfold all; clickable “⋯ N lines” on a folded block |
| Sticky header, rulers | While scrolling, the `class` / `def` lines (headings, records…) that contain the top of the screen stay there; click to go there (`sticky_scroll`). Vertical ruler at 80 characters by default, adjustable in Preferences → Appearance (`rulers`, e.g. `[79, 120]`) |
| Merge conflicts | As in VS Code, in the file: a green side, a blue side, and “Keep the green · Keep the blue · Keep both · Compare” on each block; a bar with the real name of the sides, then “Mark resolved, next file” or “… and continue the rebase”; banner for a merge / cherry-pick / rebase in progress (Continue / Skip / Abort) in the Git view |
| Find in selection | “Sel.” button of Ctrl+F / Ctrl+H; “Keep case” when replacing (cat → dog, Cat → Dog, CAT → DOG), also in the global search |
| Markdown preview, images | Ctrl+Shift+V (tab) or Ctrl+K V (beside, follows typing); png, jpg, gif, webp, bmp, ico, svg open in an image tab |
| Unsaved work kept | When closing or switching projects, the unsaved changes (and the untitled tabs) are kept and reopened, without a question, even without an open folder (can be turned off). They are also copied every 30 s: after a crash, your work comes back, and the error is written to `crash.log` (configuration folder) |
| Advanced Git | Cherry-pick, revert, interactive rebase (reorder, change a message, squash, drop), compare two commits or a commit with a branch, **stash** (set aside, view, apply, pop, drop) |
| Appearance | Interface in French or in English (Preferences → Appearance → Language), themes with previews, accent color, **softened text** (less bold text, especially on light themes), menu bar that can be hidden (commands in the palette), **your own theme** (Preferences → Appearance: all the colors of the editor and of the code, applied live) |
| Miscellaneous | Command palette (accents optional, the last used ones first), Ctrl+P fuzzy, word wrap (Alt+Z), Tab to complete paths (open, save as), minimap, indentation guides, matching brackets, auto-closing, find/replace (regex), fonts to choose among the installed ones (Preferences → Appearance; by default Consolas / Segoe UI on Windows), `.tmTheme` themes (built in: “Florm” and “Florm Light”, plus “Florm Monokai”, “Florm One Dark”, “Florm Dracula” and “Florm GitHub Light” after the usual themes of other editors). An unsaved file modified or deleted by another program: a banner asks which version to keep, Ctrl+S does not overwrite the other one without asking. Click on the message of the status bar: the last messages (errors in red) |

## Shortcuts (like Sublime)

| Shortcut | Action |
|---|---|
| Ctrl+Shift+P / F1 | Command palette |
| Ctrl+P | Go to file, recent ones first (`name:42` to go to the line, `:42` in the current file) |
| Ctrl+G | Go to line |
| Alt+G | Go to line and select it: `42`, `10-20` for several lines, empty + Enter = the current line |
| Ctrl+R | Go to symbol in the file |
| Alt+L | Show the current file in the file tree (also: right click on the tab) |
| Alt+- / Alt++ | Previous / next cursor position (also across files; back / forward mouse buttons) |
| Ctrl+K Ctrl+Z | Revert the git change under the cursor (Ctrl+Z to get it back) |
| Ctrl+K Ctrl+Shift+Z | Put the whole file back as at the last commit |
| Ctrl+Space / F12 / F2 | Suggestions / go to definition / rename the symbol |
| Shift+F12 | Find usages of the symbol |
| Ctrl+F2 / F8 / Shift+F8 | Bookmark on the line / next / previous (Ctrl+Shift+F2: remove all; “Bookmark: list” in the palette) |
| Ctrl+Shift+[ / Ctrl+Shift+] | Fold / unfold the block |
| Ctrl+\\ | Split the editor to the right |
| Ctrl+Shift+V / Ctrl+K V | Markdown preview / beside |
| Ctrl+F / Ctrl+H | Find / Replace (Enter, Shift+Enter, Alt+Enter = select all) |
| F3 / Shift+F3 | Next / previous result |
| Ctrl+Shift+F | Find in folder |
| F4 / Shift+F4 | Next / previous global result |
| Ctrl+, | Preferences |
| Ctrl+. | Quick fixes |
| Alt+Z | Word wrap |
| Ctrl+Alt+P | Switch project |
| Ctrl+Shift+E | The keyboard in the file tree (↑ ↓ ← → Enter) |
| Ctrl+Shift+` | New terminal in a tab |
| Ctrl+Alt+N | Insert a template |
| Ctrl+Alt+G | Commit graph |
| Ctrl+Alt+R | GitHub pull requests |
| Ctrl+F6 / Ctrl+Shift+F6 | Tests (Odoo, pytest, jest, cargo, go): run the last ones again / run the one under the cursor |
| F5 / Shift+F5 / Ctrl+Shift+F5 | Debugging: start or continue / stop / restart |
| F9 | Breakpoint on the line |
| F10 / F11 / Shift+F11 / F6 | Step over / into / out, pause |
| Alt+F6 | Debug the Odoo test under the cursor |
| Ctrl+Alt+C | Claude |
| Ctrl+Alt+D | Git diff in the editor (green / red) |
| Ctrl+Alt+F | Format the Python file, or the selected lines (ruff) |
| Alt+F5 / Alt+Shift+F5 | Next / previous git change (inline diff) |
| Ctrl+D | Select the word, then add the next occurrence |
| Alt+F3 | Select all occurrences |
| Ctrl+Alt+↑/↓, Alt+Shift+↑/↓ | Add a cursor above / below |
| Ctrl+Shift+L | Split the selection into lines |
| Ctrl+L | Select the line |
| Ctrl+Shift+D / Ctrl+Shift+K | Duplicate / delete the line |
| Ctrl+Shift+↑/↓ | Move the line |
| Ctrl+/ | Comment |
| Ctrl+] / Ctrl+[ | Indent / unindent |
| Ctrl+J | Join the lines |
| Ctrl+Enter / Ctrl+Shift+Enter | Insert a line after / before |
| Ctrl+M | Go to the matching bracket |
| Ctrl+F9 | Sort the lines |
| Ctrl+B (or Ctrl+K Ctrl+B) | Show/hide the sidebar |
| Alt+Shift+→ / Alt+Shift+← | Expand / shrink the selection (word, string, parentheses, block…) |
| Ctrl+Alt+Q, Ctrl+Shift+Q | Record a macro (and stop it), play it back |
| Ctrl+K Z | Distraction-free mode |
| Ctrl+K Ctrl+U / Ctrl+K Ctrl+L | Uppercase / lowercase |
| Ctrl+S, Ctrl+Shift+S, Ctrl+Alt+S | Save, save as, save all |
| Ctrl+W, Ctrl+Shift+T | Close the tab, reopen the last closed one |
| Ctrl+Tab, Ctrl+PageUp/PageDown | Switch tab |
| Ctrl+Shift+M | Problems (lint) |
| Ctrl+Shift+B | Blame of the line (follows the cursor; Ctrl+Shift+B again to hide) |
| Ctrl+Alt+B | Full history of the line (all the commits that modified it, with their diff) |

On macOS, Ctrl = Cmd. **All shortcuts can be changed** in Preferences → Shortcuts: click on a shortcut, then the new combination (Esc cancels); conflicts are reported, “↺” restores the default shortcut and “×” removes it. They are saved in the `[keys]` section of `settings.toml` (`"none"` disables a combination).

## Configuration

**Ctrl+,** (or File → Preferences) opens the preferences page: Appearance, Editor, Lint, Git, Terminal, Shortcuts, Advanced sections, with a search. Each change applies immediately and is saved in `~/.config/florm/settings.toml` (`%APPDATA%\florm` on Windows), which can also be edited directly (File → Open settings.toml).
The interface **language** (French or English) is chosen in Preferences → Appearance → Language, or with `language = "fr"` / `"en"` (empty: the system one); it changes right away, menus, panels, messages and guide included. In English, Claude also gets its instructions in English.
Changes apply on save. The theme chosen through “View → Theme…”, the minimap, the diff in the editor and the zoom are saved there automatically.

```toml
language = "fr"
font_size = 14.0
tab_size = 4
translate_tabs_to_spaces = true
theme = "Florm One Dark"
show_minimap = true
lint_on_save = true

[keys]
"ctrl+shift+r" = "git_push"
"ctrl+k ctrl+t" = "toggle_terminal"
```

The command identifiers are listed in `src/commands.rs`. The `.tmTheme` themes (e.g. Monokai)
put in `~/.config/florm/themes/` are loaded at startup.

### Configuration shared between several computers

The preferences, templates, themes and Claude skills can come from a GitHub repository, the same on each PC. The repository has a fixed layout, each part is optional:

```text
florm-config/
├── settings.toml   # preferences applied on top of those of the computer
├── templates/      # templates (one file per template, like the local folder)
├── themes/         # .tmTheme themes
└── skills/         # Claude skills (each SKILL.md, even in subfolders; or a separate repository: Preferences → Claude → My skills)
```

In Preferences → Advanced → **Shared configuration** (or `shared_config` in settings.toml), give `login` (the `login/florm-config` repository), `owner/repo` or a git address. Florm clones it into `~/.config/florm/shared/` (with `gh` if it is installed, so private repositories too) and updates it at each startup.

- **Create on GitHub** makes the repository (private, with `gh`) from the configuration of this computer.
- The shared `settings.toml` only changes the keys it contains (`[keys]`, `[lsp_servers]`… are merged). A preference changed in Florm and present in this file is written there too; new templates and themes are created in the repository.
- `shared_config`, `shell`, `odoo_test_container` and `odoo_test_history` stay specific to each computer.
- **Publish** (or Tools → Publish the shared configuration) makes the commit and the push; **Synchronize** fetches the changes from the other PCs.

## Publishing a release

```bash
gh workflow run release.yml -f version=0.2.0    # or Actions → Release → Run workflow
```

Builds on GitHub Actions and publishes in the `v0.2.0` release (created if needed): `.deb`, `.rpm`, `.pkg.tar.zst` (Arch), Linux binary, Windows `.exe` and `.msi`, macOS `.dmg` (universal Intel + Apple Silicon). Without `version`: the one of `Cargo.toml`. A push on `main` does not create a release.

### Signing the versions

The `packaging/install.sh` and `packaging/install.ps1` scripts avoid the warnings without signing. For the files downloaded **in the browser** to open without a warning too, they have to be signed, and that is not free everywhere:

- **macOS**: an [Apple Developer](https://developer.apple.com/programs/) account ($99 a year). Create a “Developer ID Application” certificate, export it as `.p12`, and an [app-specific password](https://account.apple.com) for your Apple ID. Add these secrets to the repository (Settings → Secrets and variables → Actions): `APPLE_CERTIFICATE_P12` (the `.p12` in base64: `base64 -i cert.p12 | pbcopy`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`. The release then signs and notarizes `Florm.app` and the `.dmg` by itself; without these secrets, it keeps the ad hoc signature.
- **Windows**: a code signing certificate. Florm is open source (MIT): the [SignPath Foundation](https://signpath.org) provides one for free to open source projects that ask for it, signed from GitHub Actions. The other options are paid (certificate from an authority, about €200 to €400 a year; Azure Artifact Signing is limited to individuals in the United States and Canada). Even signed, SmartScreen may still warn for the first versions, while the certificate builds a reputation.

## License

MIT — see [LICENSE](LICENSE).
