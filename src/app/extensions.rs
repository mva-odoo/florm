//! Extensions in the app: their commands in the palette and on their shortcuts, the events sent
//! to them, what they ask Florm (edit the file, a list to choose from, a message…), and the page
//! « Extensions » of the preferences. The processes and the messages: `crate::ext`.

use super::*;
use crate::catalog;
use crate::ext::{self, Incoming};
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct ExtUi {
    pub host: ext::Host,
    pub loaded: bool,
    /// A list or a field shown for an extension: its answer goes to (extension, request id).
    pub wait: Option<(usize, Value)>,
    /// The commands asked and not answered yet: request id → (extension, title).
    pub asked: HashMap<i64, (usize, String)>,
    /// The file shown at the last frame (the `focus` event when it changes).
    pub focused: Option<u64>,
    /// The catalogue: its entries and whether each is installed (None: not read yet).
    pub catalog: Option<Vec<(catalog::Entry, catalog::State)>>,
    /// The catalogue repositories being fetched.
    pub fetching: bool,
    /// The field « owner/repo » of a new catalogue.
    pub catalog_input: String,
    /// An extension from elsewhere about to be installed: its folder (asks to confirm).
    pub confirm: Option<PathBuf>,
    /// Python being downloaded for the extensions, and the commands waiting for it.
    pub py_downloading: bool,
    pub waiting_python: Vec<(usize, String)>,
}

impl App {
    /// Reads the extensions (again).
    pub(super) fn ext_reload(&mut self) {
        self.ext.host.reload(&ext::dir());
        self.ext.loaded = true;
        self.ext.wait = None;
        self.ext.asked.clear();
        for e in self.ext.host.errors.clone() {
            self.log(tf!("Extension illisible : {e}", "Unreadable extension: {e}"));
        }
    }

    fn ext_on(&self, k: usize) -> bool {
        self.ext.host.list.get(k).is_some_and(|m| !self.settings.extensions_off.iter().any(|n| *n == m.name))
    }

    /// Where Florm is, for an extension.
    fn ext_context(&self, idx: usize) -> Value {
        let mut c = json!({"root": self.root.as_ref().map(|r| r.display().to_string())});
        if let Some(d) = self.docs.get(idx).filter(|d| d.view == document::View::Text) {
            let s = d.primary_sel();
            let line = d.rope.char_to_line(s.head);
            let sel: String = d.rope.slice(s.start()..s.end()).to_string();
            c["file"] = json!(d.path.as_ref().map(|p| p.display().to_string()));
            c["language"] = json!(d.syntax);
            c["selection"] = json!(sel);
            c["line"] = json!(line + 1);
            c["col"] = json!(s.head - d.rope.line_to_char(line) + 1);
        }
        c
    }

    /// The palette's entries of the extensions' commands.
    pub(super) fn ext_items(&mut self) -> Vec<Item> {
        if !self.ext.loaded {
            self.ext_reload();
        }
        let mut v = Vec::new();
        for (k, m) in self.ext.host.list.iter().enumerate() {
            if !self.ext_on(k) {
                continue;
            }
            for c in &m.commands {
                let key = crate::keymap::Combo::parse(&c.key).map(|k| k.label()).unwrap_or_default();
                v.push(Item { label: format!("✚ {} : {}", m.name, c.title), detail: key, action: Action::Ext(k, c.id.clone()) });
            }
        }
        v
    }

    /// Python is needed by extension `k` and not there: downloaded (once, on a thread). True
    /// while it is not there.
    fn ext_python_missing(&mut self, k: usize) -> bool {
        if !self.ext.host.list.get(k).is_some_and(ext::needs_python) || self.ext.host.running(k) || crate::runtimes::python_ready().is_some() {
            return false;
        }
        if !self.ext.py_downloading {
            self.ext.py_downloading = true;
            self.set_status(t!("Téléchargement de Python pour les extensions (une seule fois, ~30 Mo)…", "Downloading Python for the extensions (only once, ~30 MB)…"));
            self.jobs.spawn(|| JobResult::PythonReady(crate::runtimes::python().map(|_| ())));
        }
        true
    }

    /// Python downloaded (or not): the commands that waited for it run.
    pub(super) fn on_python_ready(&mut self, r: Result<(), String>) {
        self.ext.py_downloading = false;
        let waiting = std::mem::take(&mut self.ext.waiting_python);
        match r {
            Ok(()) => {
                self.set_status(t!("Python est prêt pour les extensions", "Python is ready for the extensions"));
                for (k, id) in waiting {
                    self.ext_command(k, id);
                }
            }
            Err(e) => {
                self.log(e.clone());
                self.set_error(tf!("Python pas téléchargé : {}", "Python not downloaded: {}", e.lines().next().unwrap_or("")));
            }
        }
    }

    /// Runs command `id` of extension `k`.
    pub(super) fn ext_command(&mut self, k: usize, id: String) {
        if self.ext_python_missing(k) {
            self.ext.waiting_python.push((k, id));
            return;
        }
        let ctx = self.egui_ctx.clone();
        if let Err(e) = self.ext.host.ensure(k, &ctx) {
            self.log(e.clone());
            self.set_error(e);
            return;
        }
        let context = self.ext_context(self.active);
        let title = self.ext.host.list[k].commands.iter().find(|c| c.id == id).map(|c| c.title.clone()).unwrap_or_else(|| id.clone());
        let rid = self.ext.host.request(k, "command", json!({"id": id, "context": context}));
        self.ext.asked.insert(rid, (k, title));
    }

    /// Tells the extensions that want it that `name` happened to document `idx` (open, save,
    /// close, focus).
    pub(super) fn ext_event(&mut self, name: &str, idx: usize) {
        if !self.ext.loaded {
            self.ext_reload();
        }
        let wants: Vec<usize> = (0..self.ext.host.list.len()).filter(|&k| self.ext_on(k) && self.ext.host.list[k].events.iter().any(|e| e == name)).collect();
        if wants.is_empty() {
            return;
        }
        let context = self.ext_context(idx);
        let ctx = self.egui_ctx.clone();
        for k in wants {
            // without Python yet: downloaded, this event is missed
            if self.ext_python_missing(k) {
                continue;
            }
            match self.ext.host.ensure(k, &ctx) {
                Ok(()) => self.ext.host.notify(k, "event", json!({"name": name, "context": context})),
                Err(e) => {
                    self.log(e);
                    // not started again at each event: off until ⟳
                    let n = self.ext.host.list[k].name.clone();
                    self.settings.extensions_off.push(n);
                }
            }
        }
    }

    /// At each frame: what the extensions sent, their shortcuts, the `focus` event.
    pub(super) fn ext_frame(&mut self, ctx: &egui::Context) {
        if !self.ext.loaded {
            return;
        }
        for m in self.ext.host.poll() {
            match m {
                Incoming::Log(k, line) => {
                    let name = self.ext.host.list.get(k).map(|m| m.name.clone()).unwrap_or_default();
                    self.log(format!("[{name}] {line}"));
                }
                Incoming::Exited(k) => {
                    let name = self.ext.host.list.get(k).map(|m| m.name.clone()).unwrap_or_default();
                    self.ext.asked.retain(|_, (e, _)| *e != k);
                    if self.ext.wait.as_ref().is_some_and(|(e, _)| *e == k) {
                        self.ext.wait = None;
                    }
                    self.log(tf!("Extension « {name} » arrêtée", "Extension “{name}” stopped"));
                }
                Incoming::Msg(k, v) => self.ext_message(k, v),
            }
        }
        // a list or a field closed with Esc: the extension gets None
        if let Some((k, id)) = self.ext.wait.clone() {
            let shown = matches!(self.overlay.as_ref().map(|o| &o.mode), Some(Mode::ExtPick(_) | Mode::Prompt(Prompt::ExtInput(_))));
            if !shown {
                self.ext.wait = None;
                self.ext.host.respond(k, id, Ok(Value::Null));
            }
        }
        // their shortcuts (not while a list or a dialog is open)
        if self.overlay.is_none() {
            let mut run = None;
            for (k, m) in self.ext.host.list.iter().enumerate() {
                if !self.ext_on(k) {
                    continue;
                }
                for c in &m.commands {
                    let Some(combo) = crate::keymap::Combo::parse(&c.key) else { continue };
                    let mut mods = egui::Modifiers::NONE;
                    if combo.ctrl {
                        mods = mods | egui::Modifiers::COMMAND;
                    }
                    if combo.alt {
                        mods = mods | egui::Modifiers::ALT;
                    }
                    if combo.shift {
                        mods = mods | egui::Modifiers::SHIFT;
                    }
                    if ctx.input_mut(|i| i.consume_key(mods, combo.key)) {
                        run = Some((k, c.id.clone()));
                    }
                }
            }
            if let Some((k, id)) = run {
                self.ext_command(k, id);
            }
        }
        let shown = self.docs.get(self.active).filter(|d| d.path.is_some()).map(|d| d.uid);
        if shown != self.ext.focused {
            self.ext.focused = shown;
            if shown.is_some() {
                self.ext_event("focus", self.active);
            }
        }
    }

    fn ext_message(&mut self, k: usize, v: Value) {
        let name = self.ext.host.list.get(k).map(|m| m.name.clone()).unwrap_or_default();
        let Some(method) = v["method"].as_str().map(String::from) else {
            // the answer to a command: an error said
            if let Some(rid) = v["id"].as_i64() {
                if let Some((_, title)) = self.ext.asked.remove(&rid) {
                    if v.get("error").is_some() {
                        let e = ext::error_text(&v);
                        self.log(format!("[{name}] {title} :\n{e}"));
                        self.set_error(tf!("{name} — {title} : {}", "{name} — {title}: {}", e.lines().next().unwrap_or("")));
                    }
                }
            }
            return;
        };
        let p = &v["params"];
        let id = v.get("id").cloned();
        let text = p["text"].as_str().unwrap_or("").to_string();
        let mut answer: Result<Value, String> = Ok(Value::Null);
        match method.as_str() {
            "status" => self.set_status(text),
            "message" => match p["level"].as_str() {
                Some("error") => self.set_error(format!("{name} : {text}")),
                Some("warning") => self.set_status(format!("⚠ {text}")),
                _ => self.set_status(text),
            },
            "output" => self.log(format!("[{name}] {text}")),
            "editor/text" => {
                answer = match self.docs.get(self.active).filter(|d| d.view == document::View::Text) {
                    Some(d) => Ok(json!({"text": d.rope.to_string(), "file": d.path.as_ref().map(|p| p.display().to_string())})),
                    None => Err(t!("Pas de fichier ouvert", "No file open").into()),
                };
            }
            "editor/replace_selection" | "editor/insert" | "editor/set_text" => {
                answer = match self.docs.get_mut(self.active).filter(|d| d.view == document::View::Text) {
                    Some(d) if d.read_only => Err(t!("Le fichier est en lecture seule", "The file is read-only").into()),
                    Some(d) => {
                        match method.as_str() {
                            "editor/set_text" => {
                                d.replace_all(&text);
                            }
                            "editor/insert" => {
                                for s in d.sels.iter_mut() {
                                    s.anchor = s.head;
                                }
                                d.insert_text(&text, false);
                            }
                            _ => d.insert_text(&text, false),
                        }
                        Ok(Value::Null)
                    }
                    None => Err(t!("Pas de fichier ouvert", "No file open").into()),
                };
            }
            "open" => {
                let path = PathBuf::from(p["path"].as_str().unwrap_or(""));
                let path = if path.is_relative() { self.root.clone().unwrap_or_default().join(path) } else { path };
                if !self.open_path(path) {
                    answer = Err(t!("Ouverture impossible", "Cannot open").into());
                } else if let (Some(l), Some(d)) = (p["line"].as_u64(), self.docs.get_mut(self.active)) {
                    d.goto_line((l as usize).saturating_sub(1), 0);
                }
            }
            "pick" | "input" if id.is_some() => {
                // answered when chosen (or None on Esc): one at a time
                if let Some((wk, wid)) = self.ext.wait.take() {
                    self.ext.host.respond(wk, wid, Ok(Value::Null));
                }
                let title = p["title"].as_str().unwrap_or("").to_string();
                let title = if title.is_empty() { name.clone() } else { format!("{name} — {title}") };
                if method == "pick" {
                    let items: Vec<Item> = p["items"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                        .map(|(i, it)| Item { label: it["label"].as_str().unwrap_or("").to_string(), detail: it["detail"].as_str().unwrap_or("").to_string(), action: Action::ExtPick(i) })
                        .collect();
                    self.open_overlay_with(Mode::ExtPick(title), items, String::new());
                } else {
                    let value = p["value"].as_str().unwrap_or("").to_string();
                    self.open_overlay_with(Mode::Prompt(Prompt::ExtInput(title)), Vec::<Item>::new(), value);
                }
                self.ext.wait = Some((k, id.clone().unwrap_or(Value::Null)));
                return;
            }
            "run" => {
                let want = p["command"].as_str().unwrap_or("");
                match commands::ALL.iter().find(|(cid, _, _)| *cid == want) {
                    Some((_, _, c)) => self.pending.push(*c),
                    None => answer = Err(tf!("Commande inconnue : « {want} »", "Unknown command: “{want}”")),
                }
            }
            "terminal" => {
                let cmd = p["cmd"].as_str().unwrap_or("").to_string();
                self.install_type(&cmd, false);
                if let Some(t) = self.install_term {
                    self.term.type_text(t, "\r");
                }
            }
            other => answer = Err(tf!("Méthode inconnue : « {other} »", "Unknown method: “{other}”")),
        }
        if let Some(id) = id {
            self.ext.host.respond(k, id, answer);
        }
    }

    /// The choice of a list shown for an extension.
    pub(super) fn ext_picked(&mut self, i: usize) {
        if let Some((k, id)) = self.ext.wait.take() {
            self.ext.host.respond(k, id, Ok(json!(i)));
        }
    }

    /// The text typed in a field shown for an extension.
    pub(super) fn ext_typed(&mut self, text: String) {
        if let Some((k, id)) = self.ext.wait.take() {
            self.ext.host.respond(k, id, Ok(json!(text)));
        }
    }

    /// The page « Extensions » of the preferences.
    pub(super) fn extensions_page(&mut self, ui: &mut egui::Ui) {
        if !self.ext.loaded {
            self.ext_reload();
        }
        ui.label(
            RichText::new(t!(
                "Une extension ajoute des commandes à la palette (et des raccourcis), et réagit quand un fichier s'ouvre ou s'enregistre. C'est un programme, en Python ou dans un autre langage, dans un dossier de « Dossier des extensions » avec son extension.toml. Elle tourne avec tes droits : n'en installe que de sources sûres.",
                "An extension adds commands to the palette (and shortcuts), and reacts when a file opens or is saved. It is a program, in Python or another language, in a folder of « Extensions folder » with its extension.toml. It runs with your rights: only install some from sources you trust."
            ))
            .weak(),
        );
        ui.add_space(6.0);
        let (mut reload, mut new, mut folder) = (false, false, false);
        ui.horizontal(|ui| {
            reload = ui.button(t!("⟳ Recharger", "⟳ Reload")).on_hover_text(t!("Relit les extensions et redémarre celles qui tournent", "Reads the extensions again and restarts the running ones")).clicked();
            new = ui.button(t!("+ Nouvelle extension…", "+ New extension…")).on_hover_text(t!("Un exemple en Python à modifier : une commande, un raccourci, une réaction à l'enregistrement", "A Python example to change: a command, a shortcut, a reaction to saving")).clicked();
            folder = ui.button(t!("📁 Dossier des extensions", "📁 Extensions folder")).on_hover_text(ext::dir().display().to_string()).clicked();
        });
        for e in &self.ext.host.errors {
            ui.label(RichText::new(tf!("⚠ {e}", "⚠ {e}")).color(warn()));
        }
        ui.add_space(8.0);
        if self.ext.host.list.is_empty() {
            ui.label(RichText::new(t!("Aucune extension. « + Nouvelle extension… » en crée une pour commencer.", "No extension. « + New extension… » makes one to start with.")).weak());
        }
        let code_bg = ui.visuals().extreme_bg_color;
        let mut toggle: Option<(String, bool)> = None;
        let mut restart: Option<usize> = None;
        let mut open: Option<PathBuf> = None;
        for (k, m) in self.ext.host.list.iter().enumerate() {
            let mut on = !self.settings.extensions_off.iter().any(|n| *n == m.name);
            let running = self.ext.host.running(k);
            egui::Frame::none().fill(code_bg).rounding(6.0).inner_margin(egui::Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, RichText::new(&m.name).strong().size(15.0)).changed() {
                        toggle = Some((m.name.clone(), on));
                    }
                    if running {
                        ui.label(RichText::new(t!("● en marche", "● running")).small().color(ok()));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("📄").on_hover_text(t!("Ouvrir son extension.toml", "Open its extension.toml")).clicked() {
                            open = Some(m.dir.join("extension.toml"));
                        }
                        if running && ui.small_button("⟳").on_hover_text(t!("Redémarrer (après un changement de son code)", "Restart (after a change of its code)")).clicked() {
                            restart = Some(k);
                        }
                    });
                });
                if !m.description.is_empty() {
                    ui.label(RichText::new(&m.description).weak());
                }
                for c in &m.commands {
                    let key = crate::keymap::Combo::parse(&c.key).map(|k| format!("   {}", k.label())).unwrap_or_default();
                    ui.label(RichText::new(format!("▸ {}{key}", c.title)).small());
                }
                if !m.events.is_empty() {
                    ui.label(RichText::new(tf!("Réagit à : {}", "Reacts to: {}", m.events.join(", "))).small().weak());
                }
                ui.label(RichText::new(format!("{}   ({})", m.cmd, m.dir.display())).small().weak().monospace());
            });
            ui.add_space(6.0);
        }
        if let Some((name, on)) = toggle {
            self.settings.extensions_off.retain(|n| *n != name);
            if !on {
                self.settings.extensions_off.push(name.clone());
                if let Some(k) = self.ext.host.list.iter().position(|m| m.name == name) {
                    self.ext.host.stop(k);
                }
            }
            let list = serde_json::to_string(&self.settings.extensions_off).unwrap_or_else(|_| "[]".into());
            self.persist("extensions_off", list);
        }
        if let Some(k) = restart {
            self.ext.host.stop(k);
            let ctx = self.egui_ctx.clone();
            if let Err(e) = self.ext.host.ensure(k, &ctx) {
                self.set_error(e);
            }
        }
        if let Some(p) = open {
            self.open_path(p);
        }
        if reload {
            self.ext_reload();
            self.set_status(tf!("{} extension(s)", "{} extension(s)", self.ext.host.list.len()));
        }
        if folder {
            let d = ext::dir();
            let _ = std::fs::create_dir_all(&d);
            self.reveal(&d);
        }
        if new {
            self.ext_new();
        }
        self.catalog_ui(ui);
    }

    /// Reads the catalogue again (and whether each entry is installed).
    pub(super) fn catalog_read(&mut self) {
        let entries = catalog::all(&self.settings.extension_catalogs);
        self.ext.catalog = Some(entries.into_iter().map(|e| {
            let s = catalog::state(&e);
            (e, s)
        }).collect());
    }

    /// Clones the catalogue repositories again (on a thread).
    fn catalog_fetch(&mut self) {
        if self.ext.fetching || self.settings.extension_catalogs.is_empty() {
            return;
        }
        self.ext.fetching = true;
        let list = self.settings.extension_catalogs.clone();
        self.jobs.spawn(move || JobResult::CatalogFetched(list.iter().filter_map(|c| catalog::fetch(c).err()).collect()));
    }

    pub(super) fn on_catalog_fetched(&mut self, errors: Vec<String>) {
        self.ext.fetching = false;
        for e in &errors {
            self.log(e.clone());
        }
        if let Some(e) = errors.first() {
            self.set_error(tf!("Catalogue pas mis à jour : {}", "Catalogue not updated: {}", e.lines().next().unwrap_or("")));
        } else {
            self.set_status(t!("Catalogues à jour", "Catalogues up to date"));
        }
        self.catalog_read();
    }

    fn catalog_persist(&mut self) {
        let list = serde_json::to_string(&self.settings.extension_catalogs).unwrap_or_else(|_| "[]".into());
        self.persist("extension_catalogs", list);
    }

    fn catalog_install(&mut self, e: catalog::Entry) {
        match catalog::install(&e) {
            Ok(_) => {
                self.set_status(tf!("Installé : {}", "Installed: {}", e.name));
                match e.kind {
                    catalog::Kind::Extension => self.ext_reload(),
                    catalog::Kind::Pack => self.packs_check(),
                }
            }
            Err(x) => self.set_error(tf!("{} pas installé : {x}", "{} not installed: {x}", e.name)),
        }
        self.catalog_read();
    }

    /// The catalogue part of the page « Extensions ».
    fn catalog_ui(&mut self, ui: &mut egui::Ui) {
        if self.ext.catalog.is_none() {
            self.catalog_read();
        }
        ui.add_space(18.0);
        ui.label(RichText::new(t!("Catalogue", "Catalogue")).size(17.0).strong());
        ui.label(
            RichText::new(t!(
                "Les extensions et packs de langage livrés avec Florm, ceux de ta configuration partagée (ses dossiers extensions/ et packs/), et ceux des dépôts de catalogue ajoutés ici (un dossier par extension, et packs/*.toml).",
                "The extensions and language packs shipped with Florm, those of your shared configuration (its extensions/ and packs/ folders), and those of the catalogue repositories added here (a folder per extension, and packs/*.toml)."
            ))
            .weak(),
        );
        ui.add_space(4.0);
        let (mut add, mut fetch) = (false, false);
        let mut remove: Option<String> = None;
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.ext.catalog_input).hint_text(t!("propriétaire/dépôt", "owner/repo")).desired_width(220.0));
            add = ui.button(t!("+ Ajouter un catalogue", "+ Add a catalogue")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if !self.settings.extension_catalogs.is_empty() {
                fetch = ui.add_enabled(!self.ext.fetching, egui::Button::new(t!("⟳ Mettre à jour les catalogues", "⟳ Update the catalogues"))).clicked();
            }
            if self.ext.fetching {
                ui.spinner();
            }
        });
        for c in &self.settings.extension_catalogs {
            ui.horizontal(|ui| {
                ui.label(RichText::new(c).monospace().small());
                if ui.small_button("×").on_hover_text(t!("Retirer ce catalogue (ce qui est installé reste)", "Remove this catalogue (what is installed stays)")).clicked() {
                    remove = Some(c.clone());
                }
            });
        }
        ui.add_space(6.0);
        let code_bg = ui.visuals().extreme_bg_color;
        let accent = self.hl.colors.accent;
        let mut act: Option<(catalog::Entry, bool)> = None;
        let mut confirm: Option<Option<PathBuf>> = None;
        let entries = self.ext.catalog.clone().unwrap_or_default();
        for (e, st) in &entries {
            egui::Frame::none().fill(code_bg).rounding(6.0).inner_margin(egui::Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&e.name).strong());
                    let kind = match e.kind {
                        catalog::Kind::Extension => t!("extension", "extension"),
                        catalog::Kind::Pack => t!("pack de langage", "language pack"),
                    };
                    ui.label(RichText::new(kind).small().color(accent));
                    let v = if e.version.is_empty() { String::new() } else { format!(" · {}", e.version) };
                    ui.label(RichText::new(format!("{}{v}", e.source)).small().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match st {
                        catalog::State::Missing => {
                            if ui.button(t!("Installer", "Install")).clicked() {
                                if e.builtin || e.kind == catalog::Kind::Pack {
                                    act = Some((e.clone(), true));
                                } else {
                                    confirm = Some(Some(e.path.clone()));
                                }
                            }
                        }
                        catalog::State::Differs => {
                            if ui.button(t!("Mettre à jour", "Update")).on_hover_text(t!("Le catalogue a une autre version que celle installée", "The catalogue has another version than the installed one")).clicked() {
                                if e.builtin || e.kind == catalog::Kind::Pack {
                                    act = Some((e.clone(), true));
                                } else {
                                    confirm = Some(Some(e.path.clone()));
                                }
                            }
                            if ui.small_button(t!("Désinstaller", "Uninstall")).clicked() {
                                act = Some((e.clone(), false));
                            }
                        }
                        catalog::State::Installed => {
                            if ui.small_button(t!("Désinstaller", "Uninstall")).clicked() {
                                act = Some((e.clone(), false));
                            }
                            ui.label(RichText::new(t!("✓ Installé", "✓ Installed")).color(ok()));
                        }
                    });
                });
                if !e.description.is_empty() {
                    ui.label(RichText::new(&e.description).weak());
                }
                if self.ext.confirm.as_ref() == Some(&e.path) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(tf!("⚠ Cette extension vient de « {} » et tournera avec tes droits.", "⚠ This extension comes from “{}” and will run with your rights.", e.source)).color(warn()));
                        if ui.button(t!("Installer quand même", "Install anyway")).clicked() {
                            act = Some((e.clone(), true));
                            confirm = Some(None);
                        }
                        if ui.button(t!("Annuler", "Cancel")).clicked() {
                            confirm = Some(None);
                        }
                    });
                }
            });
            ui.add_space(6.0);
        }
        if let Some(c) = confirm {
            self.ext.confirm = c;
        }
        if let Some((e, install)) = act {
            if install {
                self.catalog_install(e);
            } else {
                match catalog::uninstall(&e) {
                    Ok(()) => {
                        self.set_status(tf!("Désinstallé : {}", "Uninstalled: {}", e.name));
                        if e.kind == catalog::Kind::Extension {
                            self.ext_reload();
                        } else {
                            self.packs_check();
                        }
                    }
                    Err(x) => self.set_error(x),
                }
                self.catalog_read();
            }
        }
        if add {
            let spec = self.ext.catalog_input.trim().to_string();
            if catalog::resolve(&spec).is_none() {
                self.set_error(tf!("« {spec} » : propriétaire/dépôt ou une adresse git", "“{spec}”: owner/repo or a git address"));
            } else if !self.settings.extension_catalogs.contains(&spec) {
                self.settings.extension_catalogs.push(spec);
                self.ext.catalog_input.clear();
                self.catalog_persist();
                fetch = true;
            }
        }
        if let Some(c) = remove {
            self.settings.extension_catalogs.retain(|x| *x != c);
            let _ = std::fs::remove_dir_all(catalog::clone_dir(&c));
            self.catalog_persist();
            self.catalog_read();
        }
        if fetch {
            self.catalog_fetch();
        }
    }

    /// « + Nouvelle extension » : a folder with the example, its main.py opened.
    fn ext_new(&mut self) {
        let base = ext::dir();
        let mut d = base.join("mon-extension");
        let mut n = 2;
        while d.exists() {
            d = base.join(format!("mon-extension-{n}"));
            n += 1;
        }
        let r = std::fs::create_dir_all(&d)
            .and_then(|_| std::fs::write(d.join("extension.toml"), ext::EXAMPLE_TOML.replace("\"Mon extension\"", &format!("{:?}", d.file_name().and_then(|n| n.to_str()).unwrap_or("mon-extension")))))
            .and_then(|_| std::fs::write(d.join("main.py"), ext::EXAMPLE_PY));
        if let Err(e) = r {
            self.set_error(e.to_string());
            return;
        }
        self.ext_reload();
        self.open_path(d.join("extension.toml"));
        self.open_path(d.join("main.py"));
        self.set_status(t!("Nouvelle extension : ses commandes sont dans la palette (✚) ; après un changement, ⟳ dans Préférences → Extensions", "New extension: its commands are in the palette (✚); after a change, ⟳ in Preferences → Extensions"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real extension in Python, through the app: a command edits the selection, another shows
    /// a list and inserts what is chosen, a field closed with Esc gives None, an error is shown,
    /// an event comes on saving.
    #[test]
    fn an_extension_in_python_works_through_the_app() {
        if crate::os::command(crate::lint::python()).arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("florm_ext_app_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (proj, x) = (dir.join("proj"), dir.join("extensions").join("x"));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(&x).unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        std::fs::write(proj.join("a.txt"), "hello world\n").unwrap();
        std::fs::write(
            x.join("extension.toml"),
            "name = \"X\"\ncmd = \"python3 main.py\"\nevents = [\"save\"]\n[[command]]\nid = \"upper\"\ntitle = \"Upper\"\nkey = \"ctrl+alt+u\"\n[[command]]\nid = \"pick\"\ntitle = \"Pick\"\n[[command]]\nid = \"ask\"\ntitle = \"Ask\"\n[[command]]\nid = \"boom\"\ntitle = \"Boom\"\n",
        )
        .unwrap();
        std::fs::write(
            x.join("main.py"),
            r#"import florm

@florm.command("upper")
def upper(ctx):
    print("upper", ctx.line, ctx.col)
    florm.replace_selection(ctx.selection.upper())

@florm.command("pick")
def pick(ctx):
    k = florm.pick(["A", ("B", "the second")], "Which")
    florm.insert("<%s>" % ["A", "B"][k])

@florm.command("ask")
def ask(ctx):
    florm.status("typed: %r" % florm.input("Name"))

@florm.command("boom")
def boom(ctx):
    1 / 0

@florm.on("save")
def saved(ctx):
    florm.status("saved %s, %d chars" % (ctx.file.split("/")[-1], len(florm.text())))

florm.main()
"#,
        )
        .unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        assert!(app.open_path(proj.join("a.txt")));
        app.docs[app.active].sels = vec![document::Sel::range(0, 5)];
        let items = app.ext_items();
        assert_eq!(items.iter().map(|i| (i.label.as_str(), i.detail.as_str())).collect::<Vec<_>>(), [("✚ X : Upper", "Ctrl+Alt+U"), ("✚ X : Pick", ""), ("✚ X : Ask", ""), ("✚ X : Boom", "")]);
        let wait = |app: &mut App, done: &dyn Fn(&App) -> bool| {
            let t = Instant::now();
            while !done(app) {
                assert!(t.elapsed() < Duration::from_secs(10), "status: {:?}", app.status.as_ref().map(|s| s.text.clone()));
                app.ext_frame(&ctx);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let text = |app: &App| app.docs[app.active].rope.to_string();
        app.ext_command(0, "upper".into());
        wait(&mut app, &|a| text(a) == "HELLO world\n");
        // a list: the second entry chosen
        app.ext_command(0, "pick".into());
        wait(&mut app, &|a| matches!(a.overlay.as_ref().map(|o| &o.mode), Some(Mode::ExtPick(t)) if t == "X — Which"));
        app.overlay = None;
        app.ext_picked(1);
        wait(&mut app, &|a| text(a).contains("<B>"));
        // a field closed with Esc
        app.ext_command(0, "ask".into());
        wait(&mut app, &|a| a.overlay.is_some());
        app.overlay = None;
        wait(&mut app, &|a| a.status.as_ref().is_some_and(|s| s.text == "typed: None"));
        // an error: in the status bar, the trace in the log
        app.ext_command(0, "boom".into());
        wait(&mut app, &|a| a.status.as_ref().is_some_and(|s| s.error && s.text.contains("ZeroDivisionError")));
        // saving: the event, and the extension reads the text
        let i = app.active;
        assert!(app.save(i));
        wait(&mut app, &|a| a.status.as_ref().is_some_and(|s| s.text.starts_with("saved a.txt")));
        app.ext.host.stop_all();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Odoo extension of the catalogue, installed and run through the app on a small module:
    /// the tags of the test under the cursor offered in the field, a model found from
    /// 'sale.order', the records of a file.
    #[test]
    fn the_odoo_extension_of_the_catalogue() {
        if crate::os::command(crate::lint::python()).arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("florm_ext_odoo_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let proj = dir.join("proj");
        let m = proj.join("addons").join("sale");
        std::fs::create_dir_all(m.join("tests")).unwrap();
        std::fs::create_dir_all(m.join("models")).unwrap();
        std::fs::create_dir_all(m.join("views")).unwrap();
        std::fs::write(proj.join("odoo-bin"), "#!/usr/bin/env python3\n").unwrap();
        std::fs::write(m.join("__manifest__.py"), "{'name': 'Sale'}\n").unwrap();
        std::fs::write(m.join("models").join("sale_order.py"), "from odoo import models\n\nclass SaleOrder(models.Model):\n    _name = 'sale.order'\n").unwrap();
        std::fs::write(m.join("views").join("v.xml"), "<odoo>\n  <record id=\"view_order_form\" model=\"ir.ui.view\">\n  </record>\n</odoo>\n").unwrap();
        std::fs::write(
            m.join("tests").join("test_order.py"),
            "from odoo.tests import TransactionCase\n\nclass TestOrder(TransactionCase):\n    def test_confirm(self):\n        o = self.env['sale.order']\n",
        )
        .unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        let odoo = catalog::all(&[]).into_iter().find(|e| e.name == "Odoo").unwrap();
        app.catalog_install(odoo);
        let k = app.ext.host.list.iter().position(|m| m.name == "Odoo").expect("installed");
        let wait = |app: &mut App, done: &dyn Fn(&App) -> bool| {
            let t = Instant::now();
            while !done(app) {
                assert!(t.elapsed() < Duration::from_secs(15), "status: {:?}", app.status.as_ref().map(|s| s.text.clone()));
                app.ext_frame(&ctx);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        // the test under the cursor: its tags in the command offered the first time
        assert!(app.open_path(m.join("tests").join("test_order.py")));
        let d = &mut app.docs[app.active];
        let at = d.rope.line_to_char(4) + 22;
        d.sels = vec![document::Sel::range(at, at)];
        app.ext_command(k, "test".into());
        wait(&mut app, &|a| a.overlay.as_ref().is_some_and(|o| matches!(o.mode, Mode::Prompt(Prompt::ExtInput(_)))));
        assert_eq!(app.overlay.as_ref().unwrap().query, "./odoo-bin --stop-after-init --test-enable --test-tags /sale:TestOrder.test_confirm");
        app.overlay = None;
        wait(&mut app, &|a| a.ext.wait.is_none());
        // 'sale.order' under the cursor: its model opened
        app.ext_command(k, "goto".into());
        wait(&mut app, &|a| a.docs[a.active].path.as_ref().is_some_and(|p| p.ends_with("sale_order.py")));
        assert_eq!(app.docs[app.active].rope.char_to_line(app.docs[app.active].primary_sel().head), 3);
        // the records of an XML file
        assert!(app.open_path(m.join("views").join("v.xml")));
        app.ext_command(k, "records".into());
        wait(&mut app, &|a| a.overlay.as_ref().is_some_and(|o| matches!(o.mode, Mode::ExtPick(_))));
        let items = match &app.overlay.as_ref().unwrap().items {
            Items::List(l) => l.iter().map(|i| i.label.clone()).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        assert_eq!(items, ["view_order_form  (ir.ui.view)"]);
        app.ext.host.stop_all();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The page lists the catalogue: the Odoo extension with « Installer ».
    #[test]
    fn the_page_shows_the_catalogue() {
        let dir = std::env::temp_dir().join(format!("florm_ext_page_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 3000.0));
        let mut names = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.extensions_page(ui));
            });
            names.clear();
            fn walk(s: &egui::Shape, out: &mut Vec<String>) {
                match s {
                    egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                    egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut names));
        }
        for want in ["Catalogue", "Odoo", "extension", "Florm · 1.1", "Installer", "+ Ajouter un catalogue", "+ Nouvelle extension…"] {
            assert!(names.iter().any(|n| n == want), "{want}: {names:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
