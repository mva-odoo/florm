//! What Claude Code has besides the conversation, reachable from the chat: the rules of the
//! permissions, the MCP servers, the subagents, the hooks, the memory (CLAUDE.md, `#` at the start
//! of a message), the folders given besides the project (`--add-dir`), the worktrees (a
//! conversation on a branch of its own, to run several at once) and `@file` in the field.

use super::*;
use super::claude_ui::{ClaudeUi, Item};
use crate::claude;
use serde::{Deserialize, Serialize};

/// The fields and lists of the menus of a conversation.
#[derive(Default)]
pub(super) struct ToolsUi {
    /// Rule being written in 🔒 Permissions, and whether it refuses.
    rule: String,
    rule_deny: bool,
    /// MCP servers (None: not read yet), being read.
    pub(super) mcp: Option<Result<Vec<claude::Mcp>, String>>,
    pub(super) mcp_busy: bool,
    mcp_name: String,
    mcp_target: String,
    mcp_scope: usize,
    /// Subagents (name, description, file) and settings files (label, file, hooks), read when the
    /// menu opens.
    pub(super) agents: Vec<(String, String, PathBuf)>,
    settings: Vec<(String, PathBuf, String)>,
    agent_name: String,
    add_dir: String,
    branch: String,
    /// `@` being typed: what was typed, and the matching files.
    pub(super) mention: (Option<String>, Vec<String>),
}

/// The worktree a conversation was started in: the repository it comes from and its branch.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Worktree {
    pub(super) main: PathBuf,
    pub(super) branch: String,
}

/// What was asked in the menus.
pub(super) enum ToolAct {
    Rule(String, bool),
    RemoveRule(usize, bool),
    McpList,
    McpAdd(String, String, &'static str),
    McpRemove(String),
    /// The tools of an MCP server allowed (rule `mcp__name`) or not.
    McpAllow(String, bool),
    Open(PathBuf, &'static str),
    NewAgent(String),
    AddDir(PathBuf),
    RemoveDir(usize),
    Worktree(String),
    RemoveWorktree,
    ReadLists,
}

/// Where MCP servers are kept: (value of --scope, label).
const SCOPES: [(&str, &str, &str); 3] = [
    ("local", "Ce projet, pour moi", "This project, for me"),
    ("project", "Ce projet, partagé (.mcp.json)", "This project, shared (.mcp.json)"),
    ("user", "Tous mes projets", "All my projects"),
];

fn scope_label(k: usize) -> &'static str {
    let (_, fr, en) = SCOPES[k.min(SCOPES.len() - 1)];
    if crate::i18n::en() {
        en
    } else {
        fr
    }
}

/// The state of an MCP server in words (the check marks of `claude mcp list` are not in the
/// fonts), and whether it is connected (None: neither, e.g. waiting for approval).
fn mcp_state(status: &str) -> (String, Option<bool>) {
    if status.contains('\u{2713}') {
        (t!("connecté", "connected").into(), Some(true))
    } else if status.contains('\u{2717}') {
        (t!("en échec", "failed").into(), Some(false))
    } else {
        (status.trim_start_matches(|c: char| !c.is_alphanumeric()).to_string(), None)
    }
}

/// Claude Code's own folder (`~/.claude`, or `CLAUDE_CONFIG_DIR`).
fn claude_home() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| config::home().join(".claude"))
}

/// The subagents of the project and the personal ones: (name, description, file).
fn read_agents(dir: Option<&Path>) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    let folders = dir.map(|d| d.join(".claude/agents")).into_iter().chain([claude_home().join("agents")]);
    for folder in folders {
        let Ok(rd) = std::fs::read_dir(&folder) else { continue };
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "md")).collect();
        files.sort();
        for p in files {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            // the fields of its front matter, between the two "---"
            let front: Vec<&str> = if text.starts_with("---") { text.lines().skip(1).take_while(|l| l.trim() != "---").collect() } else { Vec::new() };
            let field = |k: &str| front.iter().find_map(|l| l.strip_prefix(k)).map(|v| v.trim().to_string());
            let name = field("name:").unwrap_or_else(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            out.push((name, field("description:").unwrap_or_default(), p));
        }
    }
    out
}

/// The settings files of Claude Code (where the hooks are): (label, file, hooks it defines).
fn read_settings(dir: Option<&Path>) -> Vec<(String, PathBuf, String)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    if let Some(d) = dir {
        files.push((t!("Projet, partagé (.claude/settings.json)", "Project, shared (.claude/settings.json)").into(), d.join(".claude/settings.json")));
        files.push((t!("Projet, pour moi (.claude/settings.local.json)", "Project, for me (.claude/settings.local.json)").into(), d.join(".claude/settings.local.json")));
    }
    files.push((t!("Tous mes projets (~/.claude/settings.json)", "All my projects (~/.claude/settings.json)").into(), claude_home().join("settings.json")));
    files.into_iter().map(|(label, p)| {
        let hooks = std::fs::read_to_string(&p).ok().map(|t| hooks_summary(&t)).unwrap_or_else(|| t!("pas encore créé", "not created yet").into());
        (label, p, hooks)
    }).collect()
}

/// "PreToolUse (2), Stop (1)": the hooks of a settings file.
fn hooks_summary(json: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return t!("JSON invalide", "invalid JSON").into() };
    let Some(hooks) = v["hooks"].as_object().filter(|h| !h.is_empty()) else { return t!("aucun hook", "no hook").into() };
    hooks.iter().map(|(event, list)| format!("{event} ({})", list.as_array().map_or(0, |a| a.len()))).collect::<Vec<_>>().join(", ")
}

const SETTINGS_TEMPLATE: &str = "{\n  \"hooks\": {\n  }\n}\n";

fn agent_template(name: &str) -> String {
    tf!(
        "---\nname: {name}\ndescription: Quand l'utiliser (Claude choisit l'agent d'après cette phrase)\ntools: Read, Grep, Glob\n---\n\nSon rôle, comment il travaille et ce qu'il rend.\n",
        "---\nname: {name}\ndescription: When to use it (Claude picks the agent from this sentence)\ntools: Read, Grep, Glob\n---\n\nIts role, how it works and what it returns.\n"
    )
}

/// The `@word` being typed at char `cursor` of `input`: where it starts (the @) and the word.
pub(super) fn mention_at(input: &str, cursor: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());
    let start = chars[..cursor].iter().rposition(|c| c.is_whitespace()).map_or(0, |p| p + 1);
    (chars.get(start) == Some(&'@')).then(|| (start, chars[start + 1..cursor].iter().collect()))
}

/// The files of the project (`files`, relative to `root`) inside `dir` matching `query`, relative
/// to `dir`, best first.
pub(super) fn mention_matches(files: &crate::filelist::FileList, root: &Path, dir: &Path, query: &str) -> Vec<String> {
    let mut scored: Vec<(i32, String)> = files
        .iter()
        .filter_map(|f| {
            let rel = root.join(f).strip_prefix(dir).ok()?.to_string_lossy().replace('\\', "/");
            let score = if query.is_empty() { 0 } else { crate::fuzzy::score(query, &rel)? };
            Some((score, rel))
        })
        .take(if query.is_empty() { 8 } else { usize::MAX })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
    scored.into_iter().take(8).map(|(_, r)| r).collect()
}

/// The files and folders of `dir` cited with `@` in a message.
pub(super) fn mentioned_paths(text: &str, dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for w in text.split_whitespace().filter_map(|w| w.strip_prefix('@')) {
        let w = w.trim_end_matches([',', '.', ';', ':', '!', '?', ')']);
        let p = dir.join(w);
        if !w.is_empty() && p.exists() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// A worktree of `repo` on `branch` (created from HEAD when it does not exist), next to the
/// repository: `<repo>.worktrees/<branch>`.
fn create_worktree(repo: &Path, branch: &str) -> Result<PathBuf, String> {
    let branch = branch.trim();
    git::run(repo, &["check-ref-format", "--branch", branch]).map_err(|_| tf!("Nom de branche invalide : {branch}", "Invalid branch name: {branch}"))?;
    let top = git::repo_root(repo).ok_or_else(|| t!("Pas un dépôt git", "Not a git repository").to_string())?;
    let name = top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    let path = top.with_file_name(format!("{name}.worktrees")).join(branch.replace('/', "-"));
    if path.exists() {
        return Err(tf!("{} existe déjà", "{} already exists", path.display()));
    }
    let exists = git::run(&top, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok();
    let p = path.to_string_lossy().into_owned();
    if exists {
        git::run(&top, &["worktree", "add", &p, branch])?;
    } else {
        git::run(&top, &["worktree", "add", "-b", branch, &p, "HEAD"])?;
    }
    Ok(path)
}

/// The rules of the permissions, in 🔒 Permissions.
pub(super) fn rules_ui(ui: &mut egui::Ui, t: &mut ToolsUi, allow: &[String], deny: &[String]) -> Option<ToolAct> {
    let mut act = None;
    ui.label(RichText::new(t!("Règles", "Rules")).strong()).on_hover_text(t!(
        "Comme /permissions de Claude Code : autoriser ou interdire une partie d'un outil.\nBash(git *) : les commandes git seulement · Edit(src/**) : modifier sous src · Read(./.env) : ce fichier · WebFetch(domain:github.com) · mcp__github : les outils d'un serveur MCP.\nUne règle Bash(…) autorise cette partie même si « Lancer des commandes » n'est pas coché. Interdire l'emporte.",
        "Like /permissions of Claude Code: allow or refuse part of a tool.\nBash(git *): only git commands · Edit(src/**): edit under src · Read(./.env): that file · WebFetch(domain:github.com) · mcp__github: the tools of an MCP server.\nA Bash(…) rule allows that part even if “Run commands” is not ticked. Refusing wins."
    ));
    for (list, refused) in [(allow, false), (deny, true)] {
        for (k, r) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                let (sign, color) = if refused { ("×", err()) } else { ("✔", ok()) };
                ui.label(RichText::new(sign).color(color));
                ui.label(RichText::new(r).monospace());
                if ui.small_button("×").on_hover_text(t!("Retirer la règle", "Remove the rule")).clicked() {
                    act = Some(ToolAct::RemoveRule(k, refused));
                }
            });
        }
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("claude_rule_kind").width(90.0).selected_text(if t.rule_deny { t!("Interdire", "Refuse") } else { t!("Autoriser", "Allow") }).show_ui(ui, |ui| {
            ui.selectable_value(&mut t.rule_deny, false, t!("Autoriser", "Allow"));
            ui.selectable_value(&mut t.rule_deny, true, t!("Interdire", "Refuse"));
        });
        let r = crate::input::add(ui, egui::TextEdit::singleline(&mut t.rule).hint_text("Bash(git *)").desired_width(150.0));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if (ui.add_enabled(!t.rule.trim().is_empty(), egui::Button::new(t!("Ajouter", "Add"))).clicked() || enter) && !t.rule.trim().is_empty() {
            act = Some(ToolAct::Rule(std::mem::take(&mut t.rule).trim().to_string(), t.rule_deny));
        }
    });
    act
}

/// « ▾ » next to « Nouvelle conversation »: a conversation on a branch of its own.
pub(super) fn worktree_menu(ui: &mut egui::Ui, t: &mut ToolsUi, in_git: bool) -> Option<ToolAct> {
    let mut act = None;
    ui.menu_button("🌿", |ui| {
        ui.set_min_width(340.0);
        ui.label(RichText::new(t!("Nouvelle conversation sur une branche à part", "New conversation on a branch of its own")).strong());
        ui.label(RichText::new(t!(
            "Un worktree git : une copie de travail du dépôt dans un autre dossier, sur sa propre branche. Plusieurs Claude y travaillent en parallèle sans toucher aux mêmes fichiers ; tu fusionnes la branche ensuite.",
            "A git worktree: a working copy of the repository in another folder, on its own branch. Several Claudes work there in parallel without touching the same files; you merge the branch afterwards."
        )).small().weak());
        if !in_git {
            ui.label(RichText::new(t!("Le dossier de la conversation n'est pas un dépôt git", "The folder of the conversation is not a git repository")).color(warn()));
            return;
        }
        ui.horizontal(|ui| {
            ui.label(t!("Branche", "Branch"));
            let r = crate::input::add(ui, egui::TextEdit::singleline(&mut t.branch).hint_text("claude/ma-tache").desired_width(180.0));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.add_enabled(!t.branch.trim().is_empty(), egui::Button::new(t!("Créer", "Create"))).clicked() || enter) && !t.branch.trim().is_empty() {
                act = Some(ToolAct::Worktree(std::mem::take(&mut t.branch)));
                ui.close_menu();
            }
        });
    })
    .response
    .on_hover_text(t!("Nouvelle conversation dans un worktree : sur sa propre branche, pour lancer plusieurs Claude en parallèle", "New conversation in a worktree: on its own branch, to run several Claudes in parallel"));
    act
}

/// « ⋯ »: MCP servers, subagents, hooks, memory, extra folders, the worktree of the conversation.
pub(super) fn tools_menu(ui: &mut egui::Ui, c: &mut ClaudeUi, allow: &[String], agents_on: bool, repos: &[(PathBuf, String)]) -> Option<ToolAct> {
    let mut act = None;
    let dir = c.dir.clone();
    let t = &mut c.tools;
    let menu = ui.menu_button("⋯", |ui| {
        ui.set_min_width(260.0);
        ui.menu_button(t!("🔌 Serveurs MCP", "🔌 MCP servers"), |ui| {
            ui.set_min_width(460.0);
            if t.mcp.is_none() && !t.mcp_busy {
                act = Some(ToolAct::McpList);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("Outils externes branchés sur Claude (GitHub, base de données, navigateur…)", "External tools plugged into Claude (GitHub, database, browser…)")).small().weak());
                if t.mcp_busy {
                    spinner(ui);
                } else if ui.small_button("⟳").on_hover_text(t!("Relire (claude mcp list)", "Read again (claude mcp list)")).clicked() {
                    act = Some(ToolAct::McpList);
                }
            });
            match &t.mcp {
                Some(Err(e)) => {
                    ui.label(RichText::new(format!("⚠ {e}")).color(err()));
                }
                Some(Ok(list)) if list.is_empty() => {
                    ui.label(RichText::new(t!("Aucun serveur MCP", "No MCP server")).weak());
                }
                Some(Ok(list)) => {
                    for m in list {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&m.name).strong());
                            let (status, good) = mcp_state(&m.status);
                            let color = match good {
                                Some(true) => ok(),
                                Some(false) => err(),
                                None => ui.visuals().weak_text_color(),
                            };
                            ui.label(RichText::new(status).small().color(color)).on_hover_text(&m.status);
                            ui.label(RichText::new(&m.target).small().weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("×").on_hover_text(t!("Le retirer de Claude Code", "Remove it from Claude Code")).clicked() {
                                    act = Some(ToolAct::McpRemove(m.name.clone()));
                                }
                                let rule = format!("mcp__{}", m.name);
                                let mut on = allow.contains(&rule);
                                if ui.checkbox(&mut on, t!("autorisé", "allowed")).on_hover_text(tf!("Claude peut utiliser ses outils sans demander (règle {rule})", "Claude may use its tools without asking (rule {rule})")).changed() {
                                    act = Some(ToolAct::McpAllow(m.name.clone(), on));
                                }
                            });
                        });
                    }
                }
                None => {}
            }
            ui.separator();
            ui.label(RichText::new(t!("Ajouter un serveur", "Add a server")).strong());
            egui::Grid::new("claude_mcp_add").num_columns(2).show(ui, |ui| {
                ui.label(t!("Nom", "Name"));
                crate::input::add(ui, egui::TextEdit::singleline(&mut t.mcp_name).hint_text("github").desired_width(300.0));
                ui.end_row();
                ui.label(t!("Commande ou URL", "Command or URL"));
                crate::input::add(ui, egui::TextEdit::singleline(&mut t.mcp_target).hint_text("npx -y @modelcontextprotocol/server-github").desired_width(300.0));
                ui.end_row();
                ui.label(t!("Pour", "For"));
                egui::ComboBox::from_id_salt("claude_mcp_scope").selected_text(scope_label(t.mcp_scope)).show_ui(ui, |ui| {
                    for k in 0..SCOPES.len() {
                        ui.selectable_value(&mut t.mcp_scope, k, scope_label(k));
                    }
                });
                ui.end_row();
            });
            let ready = !t.mcp_name.trim().is_empty() && !t.mcp_target.trim().is_empty() && !t.mcp_busy;
            if ui.add_enabled(ready, egui::Button::new(t!("Ajouter", "Add"))).on_hover_text(t!("claude mcp add : une URL http(s) ou une commande avec ses arguments", "claude mcp add: an http(s) URL or a command with its arguments")).clicked() {
                act = Some(ToolAct::McpAdd(t.mcp_name.trim().to_string(), t.mcp_target.trim().to_string(), SCOPES[t.mcp_scope].0));
                t.mcp_name.clear();
                t.mcp_target.clear();
            }
            ui.label(RichText::new(t!("Claude les charge au démarrage : le prochain message les prend en compte.", "Claude loads them when it starts: the next message takes them into account.")).small().weak());
        });
        ui.menu_button(tf!("Agents ({})", "Agents ({})", t.agents.len()), |ui| {
            ui.set_min_width(420.0);
            ui.label(RichText::new(t!("Des Claude spécialisés, à qui Claude confie une partie du travail (comme /agents). Clic : ouvrir sa définition.", "Specialised Claudes, to which Claude hands part of the work (like /agents). Click: open its definition.")).small().weak());
            if !agents_on {
                ui.label(RichText::new(t!("⚠ « Sous-agents » n'est pas coché dans 🔒 Permissions", "⚠ “Subagents” is not ticked in 🔒 Permissions")).small().color(warn()));
            }
            for (name, about, path) in &t.agents {
                ui.horizontal(|ui| {
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        act = Some(ToolAct::Open(path.clone(), ""));
                        ui.close_menu();
                    }
                    ui.label(RichText::new(about).small().weak());
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                crate::input::add(ui, egui::TextEdit::singleline(&mut t.agent_name).hint_text("relecteur-sql").desired_width(180.0));
                let ok_name = !t.agent_name.is_empty() && t.agent_name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                if ui.add_enabled(ok_name, egui::Button::new(t!("+ Nouvel agent", "+ New agent"))).on_hover_text(t!("Crée .claude/agents/<nom>.md dans le projet et l'ouvre (minuscules, chiffres et -)", "Creates .claude/agents/<name>.md in the project and opens it (lowercase, digits and -)")).clicked() {
                    act = Some(ToolAct::NewAgent(std::mem::take(&mut t.agent_name)));
                    ui.close_menu();
                }
            });
        });
        ui.menu_button(t!("Hooks et réglages", "Hooks and settings"), |ui| {
            ui.set_min_width(420.0);
            ui.label(RichText::new(t!("Les hooks lancent tes commandes à des moments de Claude (avant un outil, à la fin…). Ils se règlent dans ces fichiers, clé « hooks ».", "Hooks run your commands at moments of Claude (before a tool, at the end…). They are set in these files, key “hooks”.")).small().weak());
            for (label, path, hooks) in &t.settings {
                ui.horizontal(|ui| {
                    if ui.button(label).on_hover_text(path.display().to_string()).clicked() {
                        act = Some(ToolAct::Open(path.clone(), SETTINGS_TEMPLATE));
                        ui.close_menu();
                    }
                    ui.label(RichText::new(hooks).small().weak());
                });
            }
        });
        ui.menu_button(t!("Mémoire (CLAUDE.md)", "Memory (CLAUDE.md)"), |ui| {
            ui.set_min_width(320.0);
            ui.label(RichText::new(t!("Ce que Claude lit au début de chaque conversation. Un message qui commence par # y ajoute une ligne.", "What Claude reads at the start of every conversation. A message starting with # adds a line to it.")).small().weak());
            if let Some(d) = &dir {
                if ui.button(t!("CLAUDE.md du projet", "CLAUDE.md of the project")).clicked() {
                    act = Some(ToolAct::Open(d.join("CLAUDE.md"), ""));
                    ui.close_menu();
                }
            }
            if ui.button(t!("Ma mémoire perso (~/.claude/CLAUDE.md)", "My personal memory (~/.claude/CLAUDE.md)")).clicked() {
                act = Some(ToolAct::Open(claude_home().join("CLAUDE.md"), ""));
                ui.close_menu();
            }
        });
        ui.menu_button(tf!("Dossiers en plus ({})", "Extra folders ({})", c.add_dirs.len()), |ui| {
            ui.set_min_width(380.0);
            ui.label(RichText::new(t!("Claude peut aussi lire et modifier ces dossiers (--add-dir).", "Claude may also read and change these folders (--add-dir).")).small().weak());
            for (k, d) in c.add_dirs.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(d.display().to_string());
                    if ui.small_button("×").clicked() {
                        act = Some(ToolAct::RemoveDir(k));
                    }
                });
            }
            for (repo, label) in repos.iter().filter(|(r, _)| Some(r) != dir.as_ref() && !c.add_dirs.contains(r)) {
                if ui.button(format!("+ {label}")).on_hover_text(repo.display().to_string()).clicked() {
                    act = Some(ToolAct::AddDir(repo.clone()));
                }
            }
            ui.horizontal(|ui| {
                crate::input::add(ui, egui::TextEdit::singleline(&mut t.add_dir).hint_text(t!("/chemin/du/dossier", "/path/of/the/folder")).desired_width(240.0));
                let p = PathBuf::from(t.add_dir.trim());
                if ui.add_enabled(p.is_dir(), egui::Button::new(t!("Ajouter", "Add"))).clicked() {
                    act = Some(ToolAct::AddDir(p));
                    t.add_dir.clear();
                }
            });
        });
        if let Some(w) = &c.worktree {
            ui.separator();
            ui.label(RichText::new(tf!("🌿 Worktree, branche {}", "🌿 Worktree, branch {}", w.branch)).strong());
            if ui.button(t!("Supprimer ce worktree (la branche reste)", "Remove this worktree (the branch stays)")).on_hover_text(t!("git worktree remove : refusé s'il reste des changements non commités", "git worktree remove: refused if uncommitted changes are left")).clicked() {
                act = Some(ToolAct::RemoveWorktree);
                ui.close_menu();
            }
        }
    });
    if menu.response.clicked() {
        act = Some(ToolAct::ReadLists);
    }
    menu.response.on_hover_text(t!("Serveurs MCP, agents, hooks, mémoire, dossiers en plus", "MCP servers, agents, hooks, memory, extra folders"));
    act
}

impl App {
    pub(super) fn claude_tool_act(&mut self, act: ToolAct) {
        let id = self.claude.id;
        let dir = self.claude.dir.clone();
        match act {
            ToolAct::Rule(rule, deny) => {
                let list = if deny { &mut self.settings.claude_deny } else { &mut self.settings.claude_allow };
                if !list.contains(&rule) {
                    list.push(rule);
                }
                self.claude_rules_changed();
            }
            ToolAct::RemoveRule(k, deny) => {
                let list = if deny { &mut self.settings.claude_deny } else { &mut self.settings.claude_allow };
                if k < list.len() {
                    list.remove(k);
                }
                self.claude_rules_changed();
            }
            ToolAct::McpAllow(name, on) => {
                let rule = format!("mcp__{name}");
                self.settings.claude_allow.retain(|r| *r != rule);
                if on {
                    self.settings.claude_allow.push(rule);
                }
                self.claude_rules_changed();
            }
            ToolAct::McpList => {
                let Some(dir) = dir else { return };
                self.claude.tools.mcp_busy = true;
                self.jobs.spawn(move || JobResult::ClaudeMcp(id, false, claude::mcp_list(&dir)));
            }
            ToolAct::McpAdd(name, target, scope) => {
                let Some(dir) = dir else { return };
                self.claude.tools.mcp_busy = true;
                self.jobs.spawn(move || JobResult::ClaudeMcp(id, false, claude::mcp_add(&dir, &name, &target, scope).and_then(|_| claude::mcp_list(&dir))));
                self.claude_restart_all();
            }
            ToolAct::McpRemove(name) => {
                let Some(dir) = dir else { return };
                self.claude.tools.mcp_busy = true;
                self.jobs.spawn(move || JobResult::ClaudeMcp(id, false, claude::mcp_remove(&dir, &name).and_then(|_| claude::mcp_list(&dir))));
                self.claude_restart_all();
            }
            ToolAct::Open(path, template) => self.claude_open_file(path, template),
            ToolAct::NewAgent(name) => {
                let Some(dir) = dir else { return };
                let path = dir.join(".claude/agents").join(format!("{name}.md"));
                let text = agent_template(&name);
                self.claude_open_file(path, &text);
            }
            ToolAct::AddDir(p) => {
                if !self.claude.add_dirs.contains(&p) {
                    self.claude.add_dirs.push(p);
                    self.claude_restart();
                    self.claude_save();
                }
            }
            ToolAct::RemoveDir(k) => {
                if k < self.claude.add_dirs.len() {
                    self.claude.add_dirs.remove(k);
                    self.claude_restart();
                    self.claude_save();
                }
            }
            ToolAct::Worktree(branch) => {
                let Some(repo) = dir else { return };
                self.set_status(tf!("Création du worktree {branch}…", "Creating the worktree {branch}…"));
                self.jobs.spawn(move || {
                    let main = git::repo_root(&repo).unwrap_or(repo.clone());
                    JobResult::ClaudeWorktree(create_worktree(&repo, &branch).map(|p| (p, branch, main)))
                });
            }
            ToolAct::RemoveWorktree => self.claude_remove_worktree(),
            ToolAct::ReadLists => {
                self.claude.tools.agents = read_agents(dir.as_deref());
                self.claude.tools.settings = read_settings(dir.as_deref());
            }
        }
    }

    /// The rules changed: written to the settings, and used from the next message on.
    fn claude_rules_changed(&mut self) {
        let (a, d) = (config::toml_str_array(&self.settings.claude_allow), config::toml_str_array(&self.settings.claude_deny));
        self.persist("claude_allow", a);
        self.persist("claude_deny", d);
        self.claude_restart_all();
    }

    /// Every running conversation starts Claude again at its next message (`--resume`: the
    /// conversation goes on), with the new permissions or MCP servers.
    pub(super) fn claude_restart_all(&mut self) {
        for c in std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()) {
            if c.session.is_some() {
                c.restart = true;
            }
        }
    }

    /// The current conversation starts Claude again at its next message.
    pub(super) fn claude_restart(&mut self) {
        if self.claude.session.is_some() {
            self.claude.restart = true;
        }
    }

    /// A file of Claude Code opened in the editor; created with `template` if missing (empty:
    /// created empty).
    pub(super) fn claude_open_file(&mut self, path: PathBuf, template: &str) {
        if !path.exists() {
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Err(e) = std::fs::write(&path, template) {
                self.set_error(tf!("{} : {e}", "{}: {e}", path.display()));
                return;
            }
        }
        if self.open_path(path) {
            self.pending_focus = true;
        }
    }

    /// The MCP servers of conversation `id` read (`side`: asked by /mcp, shown above the field).
    pub(super) fn claude_mcp_loaded(&mut self, id: u64, side: bool, r: Result<Vec<claude::Mcp>, String>) {
        let Some(c) = std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id) else { return };
        c.tools.mcp_busy = false;
        if side {
            let text = match &r {
                Ok(l) if l.is_empty() => Ok(t!("Aucun serveur MCP. Pour en ajouter : ⋯ → 🔌 Serveurs MCP.", "No MCP server. To add one: ⋯ → 🔌 MCP servers.").to_string()),
                Ok(l) => Ok(l.iter().map(|m| format!("- **{}** — {} — `{}`\n", m.name, mcp_state(&m.status).0, m.target)).collect()),
                Err(e) => Err(e.clone()),
            };
            if let Some((_, answer @ None)) = &mut c.side {
                *answer = Some(text);
            }
        }
        c.tools.mcp = Some(r);
    }

    /// `#` at the start of a message: the note added to the CLAUDE.md of the project, or to the
    /// personal one.
    pub(super) fn claude_remember(&mut self, note: String, personal: bool) {
        let path = if personal {
            claude_home().join("CLAUDE.md")
        } else {
            let Some(d) = self.claude.dir.clone() else { return };
            d.join("CLAUDE.md")
        };
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("- {}\n", note.trim()));
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.claude.items.push(Item::Info(tf!("— retenu dans {} : {note} —", "— remembered in {}: {note} —", path.display())));
                // Claude reads its memory when it starts: the next message starts it again
                self.claude_restart();
                self.claude_save();
            }
            Err(e) => self.set_error(tf!("{} : {e}", "{}: {e}", path.display())),
        }
    }

    /// The worktree is made: a new conversation in it, in a new tab.
    pub(super) fn claude_worktree_ready(&mut self, r: Result<(PathBuf, String, PathBuf), String>) {
        match r {
            Ok((path, branch, main)) => {
                self.claude_new_in(path.clone());
                self.claude.worktree = Some(Worktree { main, branch: branch.clone() });
                self.claude.items.push(Item::Info(tf!(
                    "— 🌿 worktree {} sur la branche {branch} : Claude travaille ici sans toucher au dépôt principal —",
                    "— 🌿 worktree {} on the branch {branch}: Claude works here without touching the main repository —",
                    path.display()
                )));
                self.set_status(tf!("Worktree créé : {}", "Worktree created: {}", path.display()));
            }
            Err(e) => self.set_error(tf!("Worktree impossible : {e}", "Cannot create the worktree: {e}")),
        }
    }

    /// The worktree of the conversation removed (its branch stays): the conversation goes on in
    /// the main repository, without the context of before (Claude keeps its sessions by folder).
    fn claude_remove_worktree(&mut self) {
        let Some(w) = self.claude.worktree.clone() else { return };
        let Some(path) = self.claude.dir.clone() else { return };
        if self.claude.busy {
            self.set_status(t!("Claude travaille : arrête-le d'abord (■ Arrêter)", "Claude is working: stop it first (■ Stop)"));
            return;
        }
        match git::run(&w.main, &["worktree", "remove", &path.to_string_lossy()]) {
            Ok(_) => {
                let c = &mut self.claude;
                c.session = None;
                c.session_id = None;
                c.last_uuid = None;
                c.checkpoints.clear();
                c.review_index = None;
                c.review = None;
                c.dir = Some(w.main.clone());
                c.worktree = None;
                c.items.push(Item::Info(tf!(
                    "— worktree supprimé ; la branche {} reste, à fusionner depuis la vue Git —",
                    "— worktree removed; the branch {} stays, to merge from the Git view —",
                    w.branch
                )));
                self.claude_save();
                self.git_refresh();
            }
            Err(e) => self.set_error(tf!("Worktree non supprimé : {e}", "Worktree not removed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_are_found_and_completed() {
        assert_eq!(mention_at("regarde @src/ma", 15), Some((8, "src/ma".into())));
        assert_eq!(mention_at("@", 1), Some((0, String::new())));
        assert_eq!(mention_at("mail a@b.fr ok", 11), None);
        assert_eq!(mention_at("regarde @src fini", 17), None);
        let files = crate::filelist::FileList::new(["src/main.rs", "src/app/claude_ui.rs", "README.md", "other/x.rs"].map(String::from).to_vec());
        let m = mention_matches(&files, Path::new("/p"), Path::new("/p"), "main");
        assert_eq!(m.first().map(String::as_str), Some("src/main.rs"));
        // only the files of the folder of the conversation, relative to it
        assert_eq!(mention_matches(&files, Path::new("/p"), Path::new("/p/src"), "claude"), ["app/claude_ui.rs"]);
        let dir = std::env::temp_dir().join(format!("florm_mention_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "").unwrap();
        assert_eq!(mentioned_paths("vois @src/a.rs, et @nope.rs puis @src", &dir), [dir.join("src/a.rs"), dir.join("src")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agents_hooks_and_worktrees() {
        let dir = std::env::temp_dir().join(format!("florm_tools_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join(".claude/agents")).unwrap();
        std::fs::write(repo.join(".claude/agents/sql.md"), "---\nname: relecteur-sql\ndescription: Relit le SQL\n---\n\nname: pas ça\n").unwrap();
        let agents = read_agents(Some(&repo));
        assert_eq!((agents[0].0.as_str(), agents[0].1.as_str()), ("relecteur-sql", "Relit le SQL"));
        assert_eq!(hooks_summary(r#"{"hooks":{"PreToolUse":[{},{}],"Stop":[{}]}}"#), "PreToolUse (2), Stop (1)");
        assert_eq!(hooks_summary("{}"), t!("aucun hook", "no hook"));
        git::run(&repo, &["init", "-q"]).unwrap();
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        git::run(&repo, &["add", "a.txt"]).unwrap();
        git::run(&repo, &["-c", "user.email=a@b", "-c", "user.name=a", "commit", "-qm", "init"]).unwrap();
        let wt = create_worktree(&repo, "claude/essai").unwrap();
        assert!(wt.ends_with("repo.worktrees/claude-essai") && wt.join("a.txt").exists(), "{}", wt.display());
        assert_eq!(git::stdout(&wt, &["branch", "--show-current"]).unwrap().trim(), "claude/essai");
        assert!(create_worktree(&repo, "claude/essai").is_err());
        assert!(create_worktree(&repo, "pas valide..").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
