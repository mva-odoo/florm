//! Starting, attaching, stopping and stepping: the command to run (here or in a container), the
//! connection to debugpy, the end of a session.

use super::*;

impl App {
    pub(super) fn debug_odoo_bin(&mut self) -> Option<String> {
        let root = self.root.clone()?;
        if self.debug.odoo_bin.as_ref().is_none_or(|(r, _)| *r != root) {
            self.debug.odoo_bin = Some((root.clone(), dap::find_odoo_bin(&root)));
        }
        self.debug.odoo_bin.as_ref().and_then(|(_, c)| c.clone())
    }

    /// The program launched here when none is set: the odoo-bin of the folder.
    pub(super) fn debug_default_local(&mut self) -> String {
        self.debug_odoo_bin().map(|b| format!("{b} --workers 0")).unwrap_or_default()
    }

    pub(super) fn debug_local_command(&mut self) -> String {
        let c = self.settings.debug_command.trim().to_string();
        if c.is_empty() { self.debug_default_local() } else { c }
    }

    pub(super) fn debug_config(&mut self) -> dap::Config {
        let target = if self.settings.debug_target.is_empty() { Target::Local } else { Target::Docker(self.settings.debug_target.clone()) };
        let command = match &target {
            Target::Local => self.debug_local_command(),
            Target::Docker(_) => Some(self.settings.debug_docker_command.trim()).filter(|c| !c.is_empty()).unwrap_or(DOCKER_COMMAND).to_string(),
        };
        let s = &self.settings;
        dap::Config {
            target,
            mode: if s.debug_mode == "attach" { Mode::Attach } else { Mode::Launch },
            command,
            cwd: self.root.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            python: Some(s.debug_python.trim()).filter(|p| !p.is_empty()).unwrap_or(if cfg!(windows) { "python" } else { "python3" }).to_string(),
            host: "127.0.0.1".into(),
            port: s.debug_port,
            just_my_code: s.debug_just_my_code,
            // attach: Florm waits for the program to listen (it may be started after)
            wait: s.debug_mode == "attach",
        }
    }

    pub(super) fn debug_title(cfg: &dap::Config) -> String {
        let place = match &cfg.target {
            Target::Local => String::new(),
            Target::Docker(c) => format!(" ({c})"),
        };
        match cfg.mode {
            Mode::Attach => tf!("attaché au port {}{place}", "attached to port {}{place}", cfg.port),
            Mode::Launch => {
                let words = crate::odoo_test::split(&cfg.command);
                let prog = words.iter().find(|w| !w.starts_with('-') && !w.starts_with("python")).map_or("?", |w| w.rsplit('/').next().unwrap_or(w));
                format!("{prog}{place}")
            }
        }
    }

    /// F5: continues when paused, else starts the program set in the view.
    pub(in crate::app) fn debug_start_or_continue(&mut self) {
        match &self.debug.session {
            Some(s) if s.paused.is_some() => self.debug_step("continue"),
            Some(s) if s.connected => self.set_status(t!("Le programme tourne (F6 : pause, Maj+F5 : arrêter)", "The program is running (F6: pause, Shift+F5: stop)")),
            None if self.debug.preparing.is_some() && !self.debug.preparing_auto => {}
            // nothing yet, or only waiting for a program: F5 starts the one of the view
            _ => {
                self.debug.auto_off = false;
                self.debug.auto_retry = None;
                let cfg = self.debug_config();
                let title = Self::debug_title(&cfg);
                self.debug_launch(cfg, title);
            }
        }
    }

    /// The Odoo test `tags` under the debugger: in the container of the debugger, else in the one
    /// of the tests when there is no odoo-bin here.
    pub(in crate::app) fn debug_test(&mut self, tags: String) {
        let mut cfg = self.debug_config();
        cfg.mode = Mode::Launch;
        if cfg.target == Target::Local && cfg.command.trim().is_empty() && !self.settings.odoo_test_container.is_empty() {
            cfg.target = Target::Docker(self.settings.odoo_test_container.clone());
        }
        let quoted = if tags.contains(char::is_whitespace) { format!("'{tags}'") } else { tags.clone() };
        cfg.command = match &cfg.target {
            Target::Docker(_) => {
                let t = Some(self.settings.odoo_test_command.trim()).filter(|t| !t.is_empty()).unwrap_or(crate::odoo_test::DEFAULT_COMMAND);
                t.replace("{tags}", &quoted)
            }
            Target::Local if cfg.command.trim().is_empty() => String::new(),
            Target::Local => format!("{} --stop-after-init --test-enable --test-tags {quoted}", cfg.command),
        };
        let title = tf!("Test {tags}", "Test {tags}");
        if self.debug.session.is_some() {
            self.debug.next = Some((cfg, title));
            self.debug_stop();
            return;
        }
        self.debug_launch(cfg, title);
    }

    pub(super) fn debug_launch(&mut self, cfg: dap::Config, title: String) {
        self.debug_launch_as(cfg, title, false);
    }

    /// `auto`: Florm waits for the program by itself: the view is not opened, and a failure is
    /// tried again later without an error each time.
    pub(super) fn debug_launch_as(&mut self, cfg: dap::Config, title: String, auto: bool) {
        // asked by hand (F5, a test): the waiting for a program gives way
        if !auto {
            if self.debug.preparing_auto {
                self.debug.preparing = None;
                self.debug.preparing_auto = false;
            }
            if self.debug.session.as_ref().is_some_and(|s| s.auto && !s.connected) {
                self.debug.session = None;
            }
        }
        if self.debug.session.is_some() || self.debug.preparing.is_some() {
            return;
        }
        self.debug.prep_id += 1;
        let id = self.debug.prep_id;
        self.debug.preparing = Some(title.clone());
        self.debug.preparing_auto = auto;
        self.debug.last = Some((cfg.clone(), title.clone()));
        if !auto {
            self.debug.failure = None;
            self.debug_note(Cat::Info, tf!("── {title} ──", "── {title} ──"));
            self.set_status(tf!("Débogage de {title}…", "Debugging {title}…"));
            self.debug_show();
        }
        self.jobs.spawn(move || JobResult::DebugReady { id, result: dap::prepare(&cfg), cfg, title });
    }

    /// A breakpoint set: Florm waits by itself (no F5, whatever the mode) for a program started
    /// with debugpy (a terminal, a container), attaches to it, and waits again after each run of
    /// it, until it is stopped by hand.
    pub(super) fn debug_auto(&mut self) {
        let d = &self.debug;
        let want = !d.auto_off && d.bps.values().any(|v| !v.is_empty());
        if !want {
            // nothing to wait for any more (breakpoints removed): the waiting stops
            if d.session.as_ref().is_some_and(|s| s.auto && !s.connected) {
                self.debug.session = None;
            }
            if self.debug.preparing_auto {
                self.debug.preparing = None;
                self.debug.preparing_auto = false;
            }
            return;
        }
        if d.session.is_some() || d.preparing.is_some() {
            return;
        }
        if let Some(t) = d.auto_retry {
            if t > Instant::now() {
                self.egui_ctx.request_repaint_after(t - Instant::now());
                return;
            }
        }
        self.debug.auto_retry = None;
        let mut cfg = self.debug_config();
        cfg.mode = Mode::Attach;
        cfg.wait = true;
        let title = Self::debug_title(&cfg);
        self.debug_launch_as(cfg, title, true);
    }

    /// A message of the waiting, written once.
    pub(super) fn debug_auto_note(&mut self, cat: Cat, text: String) {
        if self.debug.auto_note.as_deref() != Some(text.as_str()) {
            self.debug.auto_note = Some(text.clone());
            self.debug_note(cat, text);
        }
    }

    /// The program answered: the debugger is attached and stops at the breakpoints.
    pub(super) fn debug_connected(&mut self) {
        let Some(s) = self.debug.session.as_mut() else { return };
        s.connected = true;
        let title = s.title.clone();
        self.debug.failure = None;
        self.debug.auto_note = None;
        self.debug_note(Cat::Info, tf!("Connecté : {title}", "Connected: {title}"));
        self.set_status(tf!("Débogueur connecté ({title}) : il s'arrête aux points d'arrêt", "Debugger connected ({title}): it stops at the breakpoints"));
    }

    /// The command that starts the program so that Florm can attach to it (to copy into a
    /// terminal, or into the command of the container).
    pub(super) fn debug_listen_command(&mut self) -> String {
        let port = self.settings.debug_port;
        if self.settings.debug_target.is_empty() {
            let cmd = self.debug_local_command();
            let cmd = if cmd.is_empty() { "./odoo-bin --workers 0".to_string() } else { cmd };
            let python = Some(self.settings.debug_python.trim()).filter(|p| !p.is_empty()).unwrap_or(if cfg!(windows) { "python" } else { "python3" }).to_string();
            format!("{python} -m debugpy --listen {port} {cmd}")
        } else {
            format!("python3 -m debugpy --listen 0.0.0.0:{port} /usr/bin/odoo --workers 0")
        }
    }

    pub(in crate::app) fn debug_ready(&mut self, id: u64, cfg: dap::Config, title: String, result: Result<dap::Prepared, dap::Failure>) {
        // stopped meanwhile, or replaced by another one
        if id != self.debug.prep_id || self.debug.preparing.take().is_none() {
            return;
        }
        let auto = std::mem::take(&mut self.debug.preparing_auto);
        let p = match result {
            Ok(p) => p,
            Err(f) => {
                let msg = match &f {
                    dap::Failure::NoDebugpy { place, .. } => tf!("debugpy n'est pas installé {place}", "debugpy is not installed {place}"),
                    dap::Failure::Other(e) => e.clone(),
                };
                if auto {
                    // the container is maybe not started yet: tried again in a moment
                    self.debug_auto_note(Cat::Err, msg);
                    self.debug.failure = Some(f);
                    self.debug.auto_retry = Some(Instant::now() + AUTO_RETRY);
                    self.egui_ctx.request_repaint_after(AUTO_RETRY);
                    return;
                }
                self.debug_note(Cat::Err, msg.clone());
                self.set_error(tf!("Débogage impossible : {msg}", "Cannot debug: {msg}"));
                self.debug.failure = Some(f);
                self.debug.next = None;
                return;
            }
        };
        let ctx = self.egui_ctx.clone();
        let repaint = move || ctx.request_repaint();
        let client = match &p.transport {
            dap::Transport::Spawn { argv, cwd } => dap::Client::spawn(argv, cwd.as_deref(), repaint),
            dap::Transport::Tcp(host, port) => dap::Client::connect(host, *port, repaint),
        };
        let mut client = match client {
            Ok(c) => c,
            Err(e) => {
                let e = if cfg.mode == Mode::Attach { tf!("{e} (le programme écoute-t-il avec debugpy --listen ?)", "{e} (is the program listening with debugpy --listen?)") } else { e };
                if auto {
                    self.debug_auto_note(Cat::Err, e);
                    self.debug.auto_retry = Some(Instant::now() + AUTO_RETRY);
                    self.egui_ctx.request_repaint_after(AUTO_RETRY);
                    return;
                }
                self.debug_note(Cat::Err, e.clone());
                self.set_error(tf!("Débogage impossible : {e}", "Cannot debug: {e}"));
                return;
            }
        };
        let seq = client.request(
            "initialize",
            json!({
                "clientID": "florm", "clientName": "Florm", "adapterID": "debugpy", "locale": if crate::i18n::en() { "en" } else { "fr" },
                "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path",
                "supportsVariableType": true, "supportsRunInTerminalRequest": true, "supportsProgressReporting": false,
            }),
        );
        let mut pending = HashMap::new();
        pending.insert(seq, Req::Initialize);
        if cfg.wait {
            let place = match &cfg.target {
                Target::Local => String::new(),
                Target::Docker(c) => tf!(" dans {c}", " in {c}"),
            };
            let port = cfg.port;
            self.debug_auto_note(Cat::Info, tf!("En attente du programme sur le port {port}{place}…", "Waiting for the program on port {port}{place}…"));
        }
        self.debug.session = Some(Session {
            client,
            title,
            request: p.request,
            args: p.args,
            mounts: p.mounts,
            pending,
            configured: false,
            paused: None,
            threads: Vec::new(),
            stacks: HashMap::new(),
            frame: None,
            scopes: Vec::new(),
            vars: HashMap::new(),
            watch: HashMap::new(),
            hover: None,
            remote_docs: HashMap::new(),
            stopping: None,
            exception: None,
            connected: false,
            auto,
            cfg,
        });
    }

    /// Shift+F5, ■: Florm no longer waits for the program by itself (F5 waits again).
    pub(in crate::app) fn debug_stop_by_hand(&mut self) {
        self.debug.auto_off = true;
        self.debug_stop();
    }

    pub(in crate::app) fn debug_stop(&mut self) {
        if self.debug.preparing.take().is_some() {
            self.set_status(t!("Débogage annulé", "Debugging cancelled"));
            return;
        }
        // still waiting for the program: nothing to say to it
        if self.debug.session.as_ref().is_some_and(|s| !s.connected) {
            self.debug.session = None;
            self.debug.auto_note = None;
            self.set_status(t!("Attente du programme arrêtée (F5 pour attendre à nouveau)", "Waiting for the program stopped (F5 to wait again)"));
            if let Some((cfg, title)) = self.debug.next.take() {
                self.debug_launch(cfg, title);
            }
            return;
        }
        let Some(s) = self.debug.session.as_mut() else { return };
        if s.stopping.is_some() {
            self.debug_end(t!("Débogage arrêté", "Debugging stopped").into());
            return;
        }
        let launched = s.request == "launch";
        let seq = s.client.request("disconnect", json!({ "terminateDebuggee": launched, "restart": false }));
        s.pending.insert(seq, Req::Disconnect);
        s.stopping = Some(Instant::now());
    }

    pub(in crate::app) fn debug_restart(&mut self) {
        match (&self.debug.session, self.debug.last.clone()) {
            (Some(_), last) => {
                self.debug.next = last;
                self.debug.auto_off = false;
                self.debug_stop();
            }
            (None, Some((cfg, title))) => self.debug_launch(cfg, title),
            (None, None) => self.debug_start_or_continue(),
        }
    }

    /// The session is over (the program ended, or it was stopped).
    pub(super) fn debug_end(&mut self, msg: String) {
        let Some(s) = self.debug.session.take() else { return };
        let waiting = !s.connected && s.cfg.wait;
        drop(s);
        if waiting {
            // the waiting ended without the program (container stopped…): tried again in a moment
            self.debug.auto_retry = Some(Instant::now() + AUTO_RETRY);
            self.egui_ctx.request_repaint_after(AUTO_RETRY);
            if let Some((cfg, title)) = self.debug.next.take() {
                self.debug_launch(cfg, title);
            }
            return;
        }
        for d in &mut self.docs {
            d.debug_value = None;
        }
        self.debug_note(Cat::Info, msg.clone());
        self.set_status(msg);
        if let Some((cfg, title)) = self.debug.next.take() {
            self.debug_launch(cfg, title);
        }
    }

    /// Continue, pause and the steps: `continue`, `pause`, `next`, `stepIn`, `stepOut`.
    pub(in crate::app) fn debug_step(&mut self, cmd: &'static str) {
        let Some(s) = self.debug.session.as_mut() else {
            if cmd == "continue" {
                self.debug_start_or_continue();
            } else {
                self.set_status(t!("Pas de débogage en cours (F5 pour démarrer)", "No debugging session (F5 to start)"));
            }
            return;
        };
        let thread = match (&s.paused, cmd) {
            (Some(p), "pause") => {
                let _ = p;
                self.set_status(t!("Déjà en pause", "Already paused"));
                return;
            }
            (None, "pause") => s.threads.first().map_or(0, |t| t.0),
            (Some(p), _) => p.thread,
            (None, _) => {
                self.set_status(t!("Le programme n'est pas en pause", "The program is not paused"));
                return;
            }
        };
        let seq = s.client.request(cmd, json!({ "threadId": thread }));
        s.pending.insert(seq, Req::Other);
        if cmd != "pause" {
            self.debug_resumed();
        }
    }

    /// The program runs again: nothing paused to show.
    pub(super) fn debug_resumed(&mut self) {
        let Some(s) = self.debug.session.as_mut() else { return };
        s.paused = None;
        s.stacks.clear();
        s.frame = None;
        s.scopes.clear();
        s.vars.clear();
        s.watch.clear();
        s.hover = None;
        s.exception = None;
        for d in &mut self.docs {
            d.debug_value = None;
        }
    }

    /// Every frame: the messages of the adapter, the breakpoints changed, the hovered value.
    pub(in crate::app) fn debug_frame(&mut self) {
        self.debug_sync_bps();
        self.debug_auto();
        let Some(s) = &self.debug.session else { return };
        for m in s.client.poll() {
            // the first answer of the adapter: the program listens, the debugger is attached
            if matches!(m, Incoming::Response { .. } | Incoming::Event { .. }) && self.debug.session.as_ref().is_some_and(|s| !s.connected) {
                self.debug_connected();
            }
            self.debug_handle(m);
        }
        if self.debug.session.as_ref().is_some_and(|s| s.configured) && !self.debug.dirty.is_empty() {
            for p in std::mem::take(&mut self.debug.dirty) {
                self.debug_send_bps(&p);
            }
        }
        if self.debug.session.as_ref().and_then(|s| s.stopping).is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
            self.debug_end(t!("Débogage arrêté", "Debugging stopped").into());
        }
        self.debug_hover();
    }
}
