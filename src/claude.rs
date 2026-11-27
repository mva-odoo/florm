//! What Florm asks Claude Code once (`claude -p`: a commit message, a review, a completion…)
//! and what it gives it (the context of the project, pictures). No API key: the user's own
//! Claude Code login is used, like `gh` for GitHub. The conversations are Claude Code itself, in
//! a terminal (`crate::claude_term`).

use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub fn not_installed() -> String {
    t!(
        "La commande claude (Claude Code) est introuvable. Installe-la (https://claude.com/claude-code), puis lance une fois « claude » dans un terminal pour te connecter.",
        "The claude command (Claude Code) was not found. Install it (https://claude.com/claude-code), then run “claude” once in a terminal to log in."
    )
    .to_string()
}

/// What Florm knows of the project, given to Claude at the start of each conversation (see
/// `florm_context`): the commit rules, where the user is.
#[derive(Default)]
pub struct FlormContext {
    /// The rules of the commit messages (the project's, else the user's).
    pub commit_rules: String,
    /// The repository of the file shown and its branch.
    pub repo: Option<(String, String)>,
    /// Its commands run in the sandbox (without the user's SSH keys).
    pub sandbox: bool,
}

/// The text added to Claude's system prompt (`--append-system-prompt`) from `ctx`, in the
/// language of the interface; empty when Florm knows nothing worth saying.
pub fn florm_context(ctx: &FlormContext) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !ctx.commit_rules.trim().is_empty() {
        parts.push(tf!(
            "## Messages de commit\nQuand tu fais un commit, suis ces consignes :\n{}",
            "## Commit messages\nWhen you make a commit, follow these rules:\n{}",
            ctx.commit_rules.trim()
        ));
    }
    let mut here = Vec::new();
    if let Some((repo, branch)) = &ctx.repo {
        here.push(tf!("dépôt `{repo}`, branche `{branch}`", "repository `{repo}`, branch `{branch}`"));
    }
    if !here.is_empty() {
        parts.push(tf!("## Où en est l'utilisateur\n{}", "## Where the user is\n{}", here.join(" ; ")));
    }
    if ctx.sandbox {
        parts.push(t!(
            "## Commandes hors du bac à sable\nTes commandes tournent dans un bac à sable, sans les clés SSH, les jetons (GitHub, cloud…) ni les accès de l'utilisateur. Une commande qui en a besoin, ou qui doit sortir du bac à sable (git push / pull / fetch par SSH, ssh, scp, gh, un déploiement…), ne marchera pas ici : ne l'essaie pas et ne demande pas de la taper avec `!`. Lance-la sous la forme `florm-terminal <commande>` (par exemple `florm-terminal git push origin ma-branche`) : Florm la tape dans le terminal de l'utilisateur, qui la relit et la lance lui-même. Puis attends qu'il te dise comment ça s'est passé.",
            "## Commands outside the sandbox\nYour commands run in a sandbox, without the user's SSH keys, tokens (GitHub, cloud…) nor access. A command that needs them, or has to leave the sandbox (git push / pull / fetch over SSH, ssh, scp, gh, a deployment…), will not work here: do not try it and do not ask to type it with `!`. Run it as `florm-terminal <command>` (for example `florm-terminal git push origin my-branch`): Florm types it in the user's terminal, who checks it and runs it. Then wait for the user to tell you how it went."
        ).to_string());
    }
    if parts.is_empty() {
        return String::new();
    }
    let head = t!(
        "# Contexte de l'éditeur Florm\nL'utilisateur travaille dans l'éditeur Florm, qui te donne ce qu'il sait du projet.",
        "# Context from the Florm editor\nThe user works in the Florm editor, which gives you what it knows of the project."
    );
    format!("{head}\n\n{}", parts.join("\n\n"))
}

/// The project's file of rules for the commit messages written by Claude.
pub fn commit_rules_path(repo: &Path) -> PathBuf {
    repo.join(".florm").join("commit.md")
}

/// The user's own rules for the commit messages, for the repositories without rules of their own.
pub fn my_commit_rules_path() -> PathBuf {
    crate::config::config_dir().join("commit.md")
}

/// A rules file without its comment lines (they are only for the reader); empty if missing.
fn read_rules(path: &Path) -> String {
    let t = std::fs::read_to_string(path).unwrap_or_default();
    t.lines().filter(|l| !l.trim_start().starts_with("<!--")).collect::<Vec<_>>().join("\n").trim().to_string()
}

/// The rules of the repository itself for its commit messages: `.florm/commit.md`, else the
/// section of its CLAUDE.md about commits (its title contains "commit"); empty if none.
pub fn project_commit_rules(repo: &Path) -> String {
    let root = crate::git::repo_root(repo).unwrap_or_else(|| repo.to_path_buf());
    let project = read_rules(&commit_rules_path(&root));
    if !project.is_empty() {
        return project;
    }
    claude_md_section(&std::fs::read_to_string(root.join("CLAUDE.md")).unwrap_or_default(), "commit")
}

/// The rules the commit messages of `repo` must follow: the repository's, else the user's own.
pub fn commit_rules(repo: &Path) -> String {
    let project = project_commit_rules(repo);
    if project.is_empty() { read_rules(&my_commit_rules_path()) } else { project }
}

/// The section of a Markdown text whose title contains `word` (any case), without its title,
/// down to the next title of the same level or above.
fn claude_md_section(text: &str, word: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut level = None;
    for l in text.lines() {
        let hashes = l.chars().take_while(|&c| c == '#').count();
        let title = hashes > 0 && l[hashes..].starts_with(' ');
        match level {
            Some(n) if title && hashes <= n => break,
            Some(_) => out.push(l),
            None if title && l.to_lowercase().contains(word) => level = Some(hashes),
            None => {}
        }
    }
    out.join("\n").trim().to_string()
}

/// The rules of the project, right after the task of a prompt that writes commit messages
/// (read first, they weigh more); returns them, for `push_reminder` and `finish_message`.
fn push_rules(prompt: &mut String, dir: &Path) -> String {
    let rules = commit_rules(dir);
    if !rules.is_empty() {
        prompt.push_str(t!(
            "\n\nConsignes OBLIGATOIRES du projet pour les messages de commit (elles passent avant tout le reste, y compris le style des derniers messages) :\n",
            "\n\nMANDATORY rules of the project for the commit messages (they come before everything else, including the style of the last messages):\n"
        ));
        prompt.push_str(&rules);
    }
    rules
}

/// The length of the first line, said again at the end of the prompt when the rules give one.
fn push_reminder(prompt: &mut String, rules: &str) {
    if let Some(max) = subject_limit(rules) {
        prompt.push_str(&tf!("\n\nRappel : la première ligne a {max} caractères au plus.", "\n\nReminder: the first line has at most {max} characters."));
    }
}

/// The longest first line the rules allow: a number (20 to 200) on a line about characters
/// (« Sujet de 72 caractères au plus », "subject: 50 chars max").
pub fn subject_limit(rules: &str) -> Option<usize> {
    rules.lines().map(str::to_lowercase).filter(|l| l.contains("caract") || l.contains("char")).find_map(|l| {
        l.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<usize>().ok()).find(|n| (20..=200).contains(n))
    })
}

/// 72 when the bodies of the last commits of `dir` are wrapped (as git does by habit: nearly
/// all their lines within 76 columns), else None.
fn body_width(dir: &Path) -> Option<usize> {
    let bodies = crate::git::stdout(dir, &["log", "-20", "--format=%b"]).ok()?;
    let lines: Vec<usize> = bodies.lines().filter(|l| !l.trim().is_empty()).map(|l| l.chars().count()).collect();
    let short = lines.iter().filter(|&&n| n <= 76).count();
    (lines.len() >= 5 && short * 10 >= lines.len() * 9).then_some(72)
}

/// `body` wrapped at `width` paragraph by paragraph: the lines of a paragraph are joined, then cut
/// at spaces (a list item goes on under its text). Code blocks and indented lines are left as
/// they are; a word longer than the width stays whole.
fn wrap_body(body: &str, width: usize) -> String {
    // a paragraph being read: its first-line prefix, the indentation of the next lines, its words
    struct Para {
        first: String,
        hang: String,
        words: Vec<String>,
    }
    fn flush(p: &mut Option<Para>, out: &mut Vec<String>, width: usize) {
        let Some(p) = p.take() else { return };
        let mut cur = p.first.clone();
        let mut empty = true;
        for w in p.words {
            if !empty && cur.chars().count() + 1 + w.chars().count() > width {
                out.push(std::mem::replace(&mut cur, p.hang.clone()));
                empty = true;
            }
            if !empty {
                cur.push(' ');
            }
            cur.push_str(&w);
            empty = false;
        }
        out.push(cur);
    }
    let mut out: Vec<String> = Vec::new();
    let mut para: Option<Para> = None;
    let mut code = false;
    for line in body.lines() {
        let t = line.trim_start();
        let lead = line.len() - t.len();
        if t.starts_with("```") || code {
            flush(&mut para, &mut out, width);
            if t.starts_with("```") {
                code = !code;
            }
            out.push(line.to_string());
            continue;
        }
        if t.is_empty() {
            flush(&mut para, &mut out, width);
            out.push(String::new());
            continue;
        }
        let marker = ["- ", "* ", "• "].iter().find(|m| t.starts_with(**m)).map(|m| m.len()).or_else(|| {
            // "1. ", "2) "
            let n = t.chars().take_while(|c| c.is_ascii_digit()).count();
            (n > 0 && t[n..].starts_with(['.', ')']) && t[n + 1..].starts_with(' ')).then_some(n + 2)
        });
        let words = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        match (marker, para.as_mut()) {
            (Some(m), _) => {
                flush(&mut para, &mut out, width);
                para = Some(Para { first: line[..lead + m].to_string(), hang: " ".repeat(lead + m), words: words(&t[m..]) });
            }
            // the next line of a list item (indented under it), or of a paragraph
            (None, Some(p)) if lead == p.hang.len() => p.words.extend(words(t)),
            (None, _) if lead > 0 => {
                flush(&mut para, &mut out, width);
                out.push(line.to_string());
            }
            (None, _) => {
                flush(&mut para, &mut out, width);
                para = Some(Para { first: String::new(), hang: String::new(), words: words(t) });
            }
        }
    }
    flush(&mut para, &mut out, width);
    out.join("\n").trim_end().to_string()
}

/// A message as the rules want it: its first line shortened by Claude when it is longer than
/// they allow (kept when Claude cannot), its body wrapped like the last ones of the repository.
fn finish_message(dir: &Path, msg: &str, rules: &str, width: Option<usize>) -> String {
    let (subject, body) = msg.split_once('\n').unwrap_or((msg, ""));
    let mut subject = subject.trim().to_string();
    if let Some(max) = subject_limit(rules).filter(|max| subject.chars().count() > *max) {
        let prompt = tf!(
            "Raccourcis cette première ligne de message de commit (reçue sur l'entrée standard) à {max} caractères au plus. Garde son préfixe, sa langue et son sens. Consignes du projet :\n{rules}\n\nRéponds uniquement par la ligne.",
            "Shorten this first line of a commit message (given on the standard input) to at most {max} characters. Keep its prefix, its language and its meaning. Rules of the project:\n{rules}\n\nAnswer only with the line."
        );
        let short = run_once(dir, &["-p", &prompt, "--output-format", "text", "--model", "haiku", "--tools", "", "--strict-mcp-config", "--setting-sources", "user", "--no-session-persistence"], subject.clone());
        if let Some(s) = short.ok().map(|t| clean_message(&t).lines().next().unwrap_or("").trim().to_string()).filter(|s| !s.is_empty() && s.chars().count() <= max) {
            subject = s;
        }
    }
    let body = body.trim();
    let body = match width {
        Some(w) => wrap_body(body, w),
        None => body.to_string(),
    };
    if body.is_empty() { subject } else { format!("{subject}\n\n{body}") }
}

/// A commit message for `diff`, in the style of the last messages of the repository; `wish`:
/// what was asked to Claude for these changes, if known.
pub fn commit_message(dir: &Path, diff: &str, recent: &str, wish: &str) -> Result<String, String> {
    let mut prompt = String::from(t!(
        "Écris le message de commit de ce diff (reçu sur l'entrée standard). Suis le style des derniers messages du dépôt \
         (préfixes comme [FIX] / [IMP] / [ADD] module: … s'ils en ont), une première ligne courte, puis une ligne vide et \
         en quelques lignes le pourquoi si ce n'est pas évident. Réponds uniquement par le message, sans guillemets ni bloc de code.",
        "Write the commit message of this diff (given on the standard input). Follow the style of the last messages of the repository \
         (prefixes like [FIX] / [IMP] / [ADD] module: … if they have some), a short first line, then an empty line and \
         in a few lines the why if it is not obvious. Answer only with the message, without quotes or code block."
    ));
    let rules = push_rules(&mut prompt, dir);
    if !recent.trim().is_empty() {
        prompt.push_str(t!("\n\nDerniers messages du dépôt :\n", "\n\nLast messages of the repository:\n"));
        prompt.push_str(recent.trim());
    }
    if !wish.trim().is_empty() {
        prompt.push_str(t!("\n\nCe que le développeur avait demandé pour ces changements :\n", "\n\nWhat the developer had asked for these changes:\n"));
        prompt.push_str(wish.trim());
    }
    push_reminder(&mut prompt, &rules);
    // a very large diff is cut: the beginning says enough
    let diff: String = diff.chars().take(30_000).collect();
    // with rules of the project: Sonnet, which follows them (Haiku often forgets the prefixes, the length)
    let model = if rules.is_empty() { "haiku" } else { "sonnet" };
    let msg = run_once(dir, &["-p", &prompt, "--output-format", "text", "--model", model, "--tools", "", "--strict-mcp-config", "--setting-sources", "user", "--no-session-persistence"], diff).map(|t| clean_message(&t))?;
    Ok(finish_message(dir, &msg, &rules, body_width(dir)))
}

/// The description of a pull request (Markdown), from the commits of its branch (`log`) and its
/// diff: what changes and why, and what the reviewer should check.
pub fn pr_description(dir: &Path, log: &str, diff: &str) -> Result<String, String> {
    let prompt = String::from(t!(
        "Écris la description de cette pull request. L'entrée standard donne ses commits, puis son diff. Format Markdown : \
         une section « ## Résumé » avec quelques puces sur ce qui change et pourquoi, puis, si c'est utile au relecteur, \
         une section « ## À vérifier » avec les points à regarder de près. Pas de compliments, pas de bloc de code autour de \
         la réponse. Réponds uniquement par la description.",
        "Write the description of this pull request. The standard input gives its commits, then its diff. Markdown format: \
         a section « ## Summary » with a few bullet points on what changes and why, then, if it helps the reviewer, \
         a section « ## To check » with the points to look at closely. No compliments, no code block around \
         the answer. Answer only with the description."
    ));
    let log: String = log.chars().take(10_000).collect();
    let diff: String = diff.chars().take(30_000).collect();
    let input = format!("Commits :\n{log}\n\nDiff :\n{diff}");
    run_once(dir, &["-p", &prompt, "--output-format", "text", "--model", "haiku", "--tools", "", "--strict-mcp-config", "--setting-sources", "user", "--no-session-persistence"], input).map(|t| clean_message(&t))
}

/// The longest side of an image sent to Claude: a bigger one only costs more tokens.
const MAX_IMAGE_SIDE: u32 = 1568;

/// An image (RGBA pixels) as PNG, its longest side at most `MAX_IMAGE_SIDE`.
pub fn png_of(width: usize, height: usize, rgba: Vec<u8>) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba).ok_or("image")?;
    let side = width.max(height) as u32;
    let img = if side > MAX_IMAGE_SIDE {
        let s = MAX_IMAGE_SIDE as f32 / side as f32;
        let (w, h) = (((width as f32 * s) as u32).max(1), ((height as f32 * s) as u32).max(1));
        image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// An image file's bytes (PNG, JPEG, GIF, WebP, BMP…) as a PNG ready to send.
pub fn png_of_file_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
    let (w, h) = img.dimensions();
    png_of(w as usize, h as usize, img.into_raw())
}

/// Saves an image given to Claude (PNG) where its messages can show it again, also after a restart.
pub fn save_image(png: &[u8]) -> Result<std::path::PathBuf, String> {
    save_media(png, "png")
}

/// A file given to Claude (`ext`: "png", "pdf"), kept to be shown again in the message: its
/// copy, named after its content.
pub fn save_media(bytes: &[u8], ext: &str) -> Result<std::path::PathBuf, String> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    let dir = crate::config::data_dir().join("claude_images");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{:016x}.{ext}", h.finish()));
    if !path.exists() {
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

/// A PDF, given to Claude as a document (it reads its text and its pages).
pub fn is_pdf(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// The largest PDF Claude takes (32 MB, at most 100 pages).
pub const PDF_MAX: u64 = 32 * 1024 * 1024;

/// Standard base64 decoded (`None`: not base64).
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in text.bytes().filter(|c| *c != b'=') {
        acc = (acc << 6) | value(c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Runs `claude` once with `args`, `input` on its standard input; returns its answer (its
/// spaces at the start kept: the indentation of code).
fn run_once(dir: &Path, args: &[&str], input: String) -> Result<String, String> {
    run_with(crate::claude_account::command(), dir, args, input)
}

/// `run_once` with the `claude` of a given account.
fn run_with(mut cmd: std::process::Command, dir: &Path, args: &[&str], input: String) -> Result<String, String> {
    let mut child = cmd
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| not_installed())?;
    if let Some(mut stdin) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    if out.status.success() && !text.trim().is_empty() {
        Ok(text)
    } else {
        Err(format!("{text}{}", String::from_utf8_lossy(&out.stderr)).trim().to_string())
    }
}

/// A remark of an AI review, about a line of the diff (`line` 0 / `path` empty: about the whole).
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewNote {
    pub path: String,
    pub line: usize,
    /// "RIGHT": the new version of the file, "LEFT": the old one (a removed line).
    pub side: String,
    /// "bug", "risk", "suggestion" or "nit".
    pub severity: String,
    pub body: String,
}

/// Remarks of Claude on `diff`: nothing is sent anywhere, the user picks the ones to keep.
/// `about`: what the changes are (title and description of the pull request…); `language`: of
/// the remarks. Claude may read the files of `dir` (only read) for the context.
pub fn review(dir: &Path, diff: &str, about: &str, language: &str) -> Result<Vec<ReviewNote>, String> {
    let mut prompt = String::from(t!(
        "Fais la revue de code de ce diff (reçu sur l'entrée standard). Chaque ligne du diff commence par sa position : \
         [R12] = ligne 12 de la nouvelle version du fichier (lignes ajoutées « + » et lignes de contexte), \
         [L7] = ligne 7 de l'ancienne version (lignes supprimées « - »). Tu peux lire les fichiers du dépôt pour \
         comprendre le contexte (ils ne sont pas forcément dans la version du diff).\n\
         Relève seulement ce qui mérite vraiment un commentaire : bugs, cas oubliés, risques (sécurité, performance, \
         données), incohérences avec le reste du code, puis les améliorations nettes. Pas de compliments, pas de \
         remarques de style quand le code suit celui du fichier. Au plus 15 remarques, les plus importantes d'abord.\n\
         Réponds uniquement par un tableau JSON, sans texte autour ni bloc de code : \
         [{\"path\": \"chemin/du/fichier\", \"line\": 12, \"side\": \"RIGHT\", \"severity\": \"bug\", \"body\": \"…\"}] \
         où line et side viennent de la position de la ligne commentée (R → \"RIGHT\", L → \"LEFT\"), severity est \
         \"bug\", \"risk\", \"suggestion\" ou \"nit\", et body la remarque en Markdown, courte, adressée à l'auteur ; \
         quand c'est utile, un bloc ```suggestion avec le code qui remplace la ligne commentée (une seule ligne, côté RIGHT). \
         Une remarque sur l'ensemble : path \"\" et line 0. [] s'il n'y a rien à redire.",
        "Review the code of this diff (given on the standard input). Each line of the diff starts with its position: \
         [R12] = line 12 of the new version of the file (added lines “+” and context lines), \
         [L7] = line 7 of the old version (removed lines “-”). You may read the files of the repository to \
         understand the context (they are not necessarily at the version of the diff).\n\
         Point out only what really deserves a comment: bugs, forgotten cases, risks (security, performance, \
         data), inconsistencies with the rest of the code, then the clear improvements. No compliments, no \
         style remarks when the code follows the one of the file. At most 15 remarks, the most important first.\n\
         Answer only with a JSON array, without text around it or code block: \
         [{\"path\": \"path/of/the/file\", \"line\": 12, \"side\": \"RIGHT\", \"severity\": \"bug\", \"body\": \"…\"}] \
         where line and side come from the position of the commented line (R → \"RIGHT\", L → \"LEFT\"), severity is \
         \"bug\", \"risk\", \"suggestion\" or \"nit\", and body the remark in Markdown, short, addressed to the author; \
         when useful, a ```suggestion block with the code replacing the commented line (a single line, RIGHT side). \
         A remark on the whole: path \"\" and line 0. [] if there is nothing to say."
    ));
    prompt.push_str(&tf!("\nLangue des remarques : {language}.", "\nLanguage of the remarks: {language}."));
    if !about.trim().is_empty() {
        prompt.push_str(t!("\n\nCe que font ces changements :\n", "\n\nWhat these changes do:\n"));
        prompt.push_str(about.trim());
    }
    let numbered: String = number_diff(diff).chars().take(200_000).collect();
    let text = run_once(dir, &["-p", &prompt, "--output-format", "text", "--tools", "Read,Grep,Glob", "--strict-mcp-config", "--setting-sources", "user", "--no-session-persistence"], numbered)?;
    parse_review(&text)
}

/// The part of `diff` about the file `path` (all of it when it is not found, or for a remark on
/// the whole), at most 60 000 characters.
pub fn diff_of_file(diff: &str, path: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for l in diff.split_inclusive('\n') {
        if l.starts_with("diff --git ") {
            inside = !path.is_empty() && (l.trim_end().ends_with(&format!(" b/{path}")) || l.contains(&format!(" a/{path} ")));
        }
        if inside {
            out.push_str(l);
        }
    }
    let out = if out.is_empty() { diff } else { &out };
    out.chars().take(60_000).collect()
}

/// A remark of a review talked over with the developer: Claude answers `thread` (the
/// developer's words and its own answers, `true` for the developer's), and rewrites the remark
/// when it should change. Returns its answer and the new text of the remark (None: unchanged).
pub fn refine_note(dir: &Path, note: &ReviewNote, diff: &str, about: &str, thread: &[(bool, String)], language: &str) -> Result<(String, Option<String>), String> {
    let place = if note.path.is_empty() { t!("l'ensemble des changements", "the whole of the changes").to_string() } else { format!("{}:{} ({})", note.path, note.line, note.side) };
    let mut prompt = tf!(
        "Tu as fait une revue de code ; voici une de tes remarques, sur {place} :\n\n{}\n\nLe développeur en discute avec toi pour l'affiner. \
         Le diff du fichier est sur l'entrée standard (chaque ligne commence par sa position : [R12] nouvelle version, [L7] ancienne) ; \
         tu peux lire les fichiers du dépôt. Réponds-lui, puis réécris la remarque si elle doit changer (plus juste, plus courte, \
         dans une autre langue, avec un bloc ```suggestion…), sinon laisse body vide.\n\
         Réponds uniquement par un objet JSON, sans texte autour ni bloc de code : {{\"reply\": \"ta réponse, courte\", \"body\": \"la remarque réécrite, ou \"\"\"}}.\n\
         Langue de la remarque : {language}.",
        "You made a code review; here is one of your remarks, on {place}:\n\n{}\n\nThe developer talks it over with you to refine it. \
         The diff of the file is on the standard input (each line starts with its position: [R12] new version, [L7] old one); \
         you may read the files of the repository. Answer them, then rewrite the remark when it should change (more accurate, \
         shorter, in another language, with a ```suggestion block…), else leave body empty.\n\
         Answer only with a JSON object, without text around it or code block: {{\"reply\": \"your answer, short\", \"body\": \"the remark rewritten, or \"\"\"}}.\n\
         Language of the remark: {language}.",
        note.body
    );
    if !about.trim().is_empty() {
        prompt.push_str(t!("\n\nCe que font ces changements :\n", "\n\nWhat these changes do:\n"));
        prompt.push_str(about.trim());
    }
    prompt.push_str(t!("\n\nLa discussion :", "\n\nThe talk:"));
    for (dev, text) in thread {
        let who = if *dev { t!("Développeur", "Developer") } else { t!("Toi", "You") };
        prompt.push_str(&format!("\n{who} : {text}"));
    }
    let numbered: String = number_diff(diff).chars().take(60_000).collect();
    let text = run_once(dir, &["-p", &prompt, "--output-format", "text", "--tools", "Read,Grep,Glob", "--strict-mcp-config", "--setting-sources", "user", "--no-session-persistence"], numbered)?;
    parse_refined(&text)
}

/// `{"reply": …, "body": …}`, found in the answer (a model may write around it).
fn parse_refined(text: &str) -> Result<(String, Option<String>), String> {
    let (a, b) = (text.find('{'), text.rfind('}'));
    let v: Value = match (a, b) {
        (Some(a), Some(b)) if a < b => serde_json::from_str(&text[a..=b]).map_err(|e| format!("{e} : {}", text.trim()))?,
        // no JSON: the whole answer is the reply
        _ => return Ok((text.trim().to_string(), None)),
    };
    let s = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let body = s("body");
    Ok((s("reply"), (!body.is_empty()).then_some(body)))
}

/// The diff with the position of each line in front of it, so the model does not count lines.
fn number_diff(diff: &str) -> String {
    let mut out = String::with_capacity(diff.len() + diff.len() / 4);
    for l in crate::history::parse_diff(diff) {
        match (l.kind, l.old, l.new) {
            (crate::history::DiffKind::Removed, Some(o), _) => out.push_str(&format!("[L{o}] ")),
            (_, _, Some(n)) => out.push_str(&format!("[R{n}] ")),
            _ => {}
        }
        out.push_str(&l.text);
        out.push('\n');
    }
    out
}

/// The JSON array of an answer (the model sometimes puts text or a code fence around it).
fn json_array(text: &str) -> Result<Value, String> {
    let end = text.rfind(']').unwrap_or(0);
    // the first '[' starting a valid array up to the last ']' (a "[R12]" may come before it)
    text.match_indices('[')
        .filter(|(a, _)| *a < end)
        .find_map(|(a, _)| serde_json::from_str::<Value>(&text[a..=end]).ok())
        .ok_or_else(|| tf!("Réponse inattendue de Claude : {}", "Unexpected answer from Claude: {}", text.chars().take(300).collect::<String>()))
}

fn parse_review(text: &str) -> Result<Vec<ReviewNote>, String> {
    let v = json_array(text)?;
    let s = |x: &Value| x.as_str().unwrap_or("").trim().to_string();
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .map(|n| ReviewNote {
                    path: s(&n["path"]),
                    line: n["line"].as_u64().or_else(|| n["line"].as_str().and_then(|l| l.trim_start_matches(['R', 'L']).parse().ok())).unwrap_or(0) as usize,
                    side: if s(&n["side"]).eq_ignore_ascii_case("LEFT") { "LEFT".into() } else { "RIGHT".into() },
                    severity: s(&n["severity"]).to_lowercase(),
                    body: s(&n["body"]),
                })
                .filter(|n| !n.body.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

/// Without the code fence or quotes a model sometimes puts around the message.
fn clean_message(t: &str) -> String {
    let t = t.trim();
    let t = t.strip_prefix("```").map(|r| r.split_once('\n').map_or(r, |(_, b)| b)).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    t.trim_matches('"').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_rules_are_found_and_kept() {
        assert_eq!(subject_limit("- En Anglais.\n- Sujet de 72 caractères au plus, puis une ligne vide"), Some(72));
        assert_eq!(subject_limit("Subject: 50 chars max"), Some(50));
        assert_eq!(subject_limit("- In English.\n- [FIX] module: 2 words"), None);
        // a paragraph already cut short is joined, then cut at the width; a list item hangs under
        // its text; code stays as it is
        let body = "The route\nis new and\nits name is long.\n\n- an item whose text is long enough to go past the width\n```\nlet a_very_long_line_of_code = it_is_left_as_it_is_even_past_the_width(1);\n```";
        let w = wrap_body(body, 30);
        assert_eq!(w.lines().take(2).collect::<Vec<_>>(), ["The route is new and its name", "is long."], "{w}");
        assert!(w.contains("- an item whose text is long\n  enough"), "{w}");
        assert!(w.lines().all(|l| l.chars().count() <= 30 || l.starts_with("let ")), "{w}");
    }

    #[test]
    fn commit_section_of_claude_md() {
        let md = "# P\n\n## Commandes\n\nx\n\n## Commits\n\nEn français, `[TAG] Zone : …`.\n\n### Exemple\n\nex\n\n## Réponses\n\ny";
        assert_eq!(claude_md_section(md, "commit"), "En français, `[TAG] Zone : …`.\n\n### Exemple\n\nex");
        assert_eq!(claude_md_section("# A\nrien", "commit"), "");
    }

    #[test]
    fn review_answers() {
        let diff = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ b/x.py\n@@ -1,3 +1,3 @@\n a = 1\n-b = 2\n+b = 3\n c = 4";
        let n = number_diff(diff);
        assert!(n.contains("[R1]  a = 1\n[L2] -b = 2\n[R2] +b = 3\n[R3]  c = 4"), "{n}");
        assert!(n.starts_with("diff --git"), "{n}");
        let t = "Voici :\n```json\n[{\"path\":\"x.py\",\"line\":2,\"side\":\"RIGHT\",\"severity\":\"Bug\",\"body\":\"b vaut 3 ?\"},\
                 {\"path\":\"x.py\",\"line\":\"L2\",\"side\":\"left\",\"severity\":\"nit\",\"body\":\"ok\"},{\"path\":\"\",\"line\":0,\"body\":\"\"}]\n```";
        assert_eq!(
            parse_review(t).unwrap(),
            vec![
                ReviewNote { path: "x.py".into(), line: 2, side: "RIGHT".into(), severity: "bug".into(), body: "b vaut 3 ?".into() },
                ReviewNote { path: "x.py".into(), line: 2, side: "LEFT".into(), severity: "nit".into(), body: "ok".into() },
            ]
        );
        assert_eq!(parse_review("Rien à redire (voir [R12]) : []").unwrap(), vec![]);
        assert!(parse_review("désolé").is_err());
    }


    #[test]
    fn a_remark_talked_over() {
        assert_eq!(parse_refined("Voilà :\n{\"reply\": \"Oui, plus court.\", \"body\": \"Ajoute `strip()`.\"}").unwrap(), ("Oui, plus court.".into(), Some("Ajoute `strip()`.".into())));
        assert_eq!(parse_refined("{\"reply\": \"Elle reste juste.\", \"body\": \"\"}").unwrap(), ("Elle reste juste.".into(), None));
        assert_eq!(parse_refined("juste du texte").unwrap(), ("juste du texte".into(), None));
        let diff = "diff --git a/x.py b/x.py\n+a\ndiff --git a/y.py b/y.py\n+b\n";
        assert_eq!(diff_of_file(diff, "y.py"), "diff --git a/y.py b/y.py\n+b\n");
        assert_eq!(diff_of_file(diff, ""), diff);
    }

    #[test]
    fn florm_tells_claude_what_it_knows() {
        assert!(florm_context(&FlormContext::default()).is_empty());
        let ctx = FlormContext {
            commit_rules: "[TAG] Zone : quoi".into(),
            repo: Some(("shop".into(), "2.0-fix".into())),
            sandbox: false,
        };
        let t = florm_context(&ctx);
        assert!(t.contains("[TAG] Zone : quoi") && t.contains("2.0-fix") && !t.contains("florm-terminal"), "{t}");
        // in the sandbox: how to hand a command over to the user
        let t = florm_context(&FlormContext { sandbox: true, ..Default::default() });
        assert!(t.contains("florm-terminal <"), "{t}");
    }

    #[test]
    fn image_files_become_smaller_pngs() {
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(2000, 100).write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap();
        let png = png_of_file_bytes(jpeg.get_ref()).unwrap();
        let back = image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
        assert_eq!((back.width(), back.height()), (1568, 78));
        assert!(png_of_file_bytes(b"not an image").is_err());
    }

}
