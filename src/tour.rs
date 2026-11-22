//! A guided tour of changes: Claude shares the blocks of a diff (changes not committed, a pull
//! request) out into ideas, in the order to read them, each with a short explanation, its risks
//! when there are some, and a note on each of its blocks. The code shown is always the real diff:
//! Claude only groups and explains it. An idea of the changes not committed can be dropped as a whole (all its blocks taken out of
//! the files), and put back. A review comment is explained the same way, without blocks.

use crate::git;
use crate::split::{self, Block, FileChange};
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step {
    pub title: String,
    /// Why this idea and what it does, in a few sentences.
    pub text: String,
    /// What could break; empty when nothing in particular.
    pub risks: String,
    /// "path:line" places to look at (clickable), for a tour without blocks (a comment).
    pub places: Vec<String>,
    /// Its blocks (indices in the blocks of the tour).
    pub blocks: Vec<usize>,
    /// What each of its blocks does (same order as `blocks`, empty when Claude said nothing).
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tour {
    pub title: String,
    pub problem: String,
    pub idea: String,
    pub steps: Vec<Step>,
    /// What the developer has to decide, and what was checked or not.
    pub decide: String,
}

fn language() -> &'static str {
    t!("français", "English")
}

/// The answer format, shared by the prompts.
fn format_rules() -> &'static str {
    t!(
        "Réponds uniquement par un objet JSON, sans texte autour ni bloc de code :\n\
         {\"title\": \"titre court du changement\", \"problem\": \"…\", \"idea\": \"…\", \
         \"steps\": [{\"title\": \"…\", \"text\": \"…\", \"risks\": \"\", \"places\": [], \
         \"blocks\": [{\"n\": 1, \"note\": \"…\"}, {\"n\": 3, \"note\": \"…\"}]}], \"decide\": \"…\"}\n\
         Sois bref : le développeur lit le code, tes textes l'aident à le lire, ils ne le racontent pas.\n\
         - problem : 1-2 phrases, ce qui ne va pas ou ce qui manque, vu par quelqu'un qui utilise le programme.\n\
         - idea : une phrase, la solution en une image simple.\n\
         - steps : une étape = UNE IDÉE (un concept, un mécanisme, une décision), même si elle touche plusieurs fichiers ; jamais une étape par fichier. \
         Dans l'ordre où il faut les lire : d'abord ce qui porte l'idée principale, à la fin le nettoyage et les tests. \
         Deux à cinq étapes en général, une seule pour un petit changement.\n\
         - text : 2-3 phrases courtes, pourquoi cette idée et ce qu'elle fait dans l'ensemble ; pas le détail du code (il va dans les notes des blocs).\n\
         - risks : une phrase sur ce qui pourrait vraiment casser, sinon \"\".\n\
         - blocks : chaque bloc de l'étape avec sa note : une phrase concrète sur ce que fait CE bloc (« ajoute… », « remplace… par… », « appelle… quand… »).\n\
         - places : seulement quand il n'y a pas de blocs, les endroits à lire sous la forme chemin:ligne ; sinon [].\n\
         - decide : 1 à 3 points courts, ce que le développeur doit trancher ou vérifier (\"\" s'il n'y a rien).\n\
         Textes en Markdown, phrases simples, sans jargon inutile.",
        "Answer only with a JSON object, without text around it or code block:\n\
         {\"title\": \"short title of the change\", \"problem\": \"…\", \"idea\": \"…\", \
         \"steps\": [{\"title\": \"…\", \"text\": \"…\", \"risks\": \"\", \"places\": [], \
         \"blocks\": [{\"n\": 1, \"note\": \"…\"}, {\"n\": 3, \"note\": \"…\"}]}], \"decide\": \"…\"}\n\
         Be brief: the developer reads the code, your texts help them read it, they do not tell it again.\n\
         - problem: 1-2 sentences, what is wrong or missing, as seen by someone using the program.\n\
         - idea: one sentence, the solution as a simple picture.\n\
         - steps: a step = ONE IDEA (a concept, a mechanism, a decision), even when it touches several files; never one step per file. \
         In the order to read them: first what carries the main idea, at the end the cleanup and the tests. \
         Two to five steps usually, a single one for a small change.\n\
         - text: 2-3 short sentences, why this idea and what it does as a whole; not the details of the code (they go in the notes of the blocks).\n\
         - risks: one sentence on what could really break, else \"\".\n\
         - blocks: each block of the step with its note: one concrete sentence on what THIS block does (“adds…”, “replaces… with…”, “calls… when…”).\n\
         - places: only when there are no blocks, the places to read as path:line; else [].\n\
         - decide: 1 to 3 short points, what the developer has to decide or check (\"\" if there is nothing).\n\
         Texts in Markdown, simple sentences, without needless jargon."
    )
}

/// The options of every call: Sonnet (good at sorting by idea, much faster than Opus), reading
/// the repository allowed, nothing kept.
fn run(dir: &Path, prompt: &str, input: String) -> Result<String, String> {
    crate::claude::run_once(dir, &["-p", prompt, "--output-format", "text", "--model", "sonnet", "--tools", "Read,Grep,Glob", "--setting-sources", "user", "--no-session-persistence"], input)
}

/// The tour of `blocks` (numbered B1, B2… for Claude). `about`: what the changes are meant to do
/// (the request made to Claude, the title and description of a pull request…). Claude may read
/// the files of `dir`.
pub fn explain(dir: &Path, blocks: &[Block], about: &str) -> Result<Tour, String> {
    let mut prompt = String::from(t!(
        "Explique pas à pas ces changements de code à un développeur qui veut comprendre ce qui est proposé et pourquoi, \
         assez pour décider idée par idée. L'entrée standard donne le diff en blocs numérotés (B1, B2…). Tu peux lire les \
         fichiers du dépôt pour comprendre le contexte. Ne modifie rien.\n\
         Chaque bloc va dans exactement UNE étape ; aucun n'est oublié. Un bloc qui ne sert aucune idée (changement \
         accidentel) va dans une étape « Changements sans rapport », signalée dans decide.\n",
        "Explain step by step these code changes to a developer who wants to understand what is proposed and why, \
         enough to decide idea by idea. The standard input gives the diff in numbered blocks (B1, B2…). You may read the \
         files of the repository to understand the context. Change nothing.\n\
         Each block goes into exactly ONE step; none is forgotten. A block serving no idea (accidental change) goes \
         into a step “Unrelated changes”, pointed out in decide.\n"
    ));
    prompt.push_str(format_rules());
    prompt.push_str(&tf!("\nLangue des textes : {}.", "\nLanguage of the texts: {}.", language()));
    if !about.trim().is_empty() {
        prompt.push_str(t!("\n\nCe que ces changements doivent faire :\n", "\n\nWhat these changes are meant to do:\n"));
        prompt.push_str(about.trim());
    }
    let input: String = split::blocks_text(blocks).chars().take(200_000).collect();
    parse_tour(&run(dir, &prompt, input)?, blocks.len())
}

/// The tour of a review comment (`comment`: the comment, its replies and the code commented):
/// what is asked, whether it is right, what it would change. No blocks.
pub fn explain_comment(dir: &Path, comment: &str, about: &str) -> Result<Tour, String> {
    let mut prompt = String::from(t!(
        "Un développeur ne comprend pas bien ce commentaire de revue de code (sur l'entrée standard, avec le code commenté). \
         Explique-le pas à pas : dans problem, reformule ce que le relecteur demande vraiment et pourquoi ; dans idea, ce \
         qu'il faudrait faire ; dans les étapes, une par idée de changement, en disant à chaque fois s'il a raison et pourquoi \
         (dans text). Tu peux lire les fichiers du dépôt pour comprendre le contexte (ils sont peut-être déjà dans une autre \
         version). Ne modifie rien. \"blocks\" : toujours []. \"places\" : les endroits à changer ou à lire, sous la forme chemin:ligne.\n",
        "A developer does not quite understand this code review comment (on the standard input, with the code commented). \
         Explain it step by step: in problem, rephrase what the reviewer really asks and why; in idea, what should be done; \
         in the steps, one per idea of change, saying each time whether the reviewer is right and why (in text). You may read \
         the files of the repository for the context (they may be at another version already). Change nothing. \
         \"blocks\": always []. \"places\": the places to change or to read, as path:line.\n"
    ));
    prompt.push_str(format_rules());
    prompt.push_str(&tf!("\nLangue des textes : {}.", "\nLanguage of the texts: {}.", language()));
    if !about.trim().is_empty() {
        prompt.push_str(t!("\n\nCe que fait la pull request :\n", "\n\nWhat the pull request does:\n"));
        prompt.push_str(about.trim());
    }
    let input: String = comment.chars().take(60_000).collect();
    parse_tour(&run(dir, &prompt, input)?, 0)
}

/// A step of the tour talked over: Claude answers the developer's last question (`thread`: the
/// developer's words and its answers, `true` for the developer's). Returns its answer (Markdown).
pub fn ask(dir: &Path, tour: &Tour, step: usize, blocks: &[Block], thread: &[(bool, String)]) -> Result<String, String> {
    let Some(s) = tour.steps.get(step) else { return Err("?".into()) };
    let mut prompt = tf!(
        "Tu as expliqué des changements de code étape par étape. Le changement : {} — le problème : {} — l'idée : {}\n\n\
         Le développeur pose une question sur l'étape « {} » :\n{}\nRisques : {}\n\n\
         Le code de cette étape est sur l'entrée standard ; tu peux lire les fichiers du dépôt. Ne modifie rien. \
         Réponds-lui simplement et concrètement, en Markdown, court (un exemple vaut mieux qu'une définition). Langue : {}.",
        "You explained code changes step by step. The change: {} — the problem: {} — the idea: {}\n\n\
         The developer asks a question about the step “{}”:\n{}\nRisks: {}\n\n\
         The code of this step is on the standard input; you may read the files of the repository. Change nothing. \
         Answer them simply and concretely, in Markdown, short (an example is better than a definition). Language: {}.",
        tour.title,
        tour.problem,
        tour.idea,
        s.title,
        s.text,
        s.risks,
        language()
    );
    prompt.push_str(t!("\n\nLa discussion :", "\n\nThe talk:"));
    for (dev, text) in thread {
        let who = if *dev { t!("Développeur", "Developer") } else { t!("Toi", "You") };
        prompt.push_str(&format!("\n{who} : {text}"));
    }
    let mine: Vec<Block> = s.blocks.iter().filter_map(|&b| blocks.get(b).cloned()).collect();
    let input: String = split::blocks_text(&mine).chars().take(60_000).collect();
    run(dir, &prompt, input).map(|t| t.trim().to_string())
}

/// The tour in an answer of Claude, with `n` blocks: each block in exactly one step (the first
/// that names it); the blocks no step names go into a last step of their own.
fn parse_tour(text: &str, n: usize) -> Result<Tour, String> {
    let unexpected = || tf!("Réponse inattendue de Claude : {}", "Unexpected answer from Claude: {}", text.chars().take(300).collect::<String>());
    let (a, b) = (text.find('{').ok_or_else(unexpected)?, text.rfind('}').ok_or_else(unexpected)?);
    let v: Value = serde_json::from_str(text.get(a..=b).ok_or_else(unexpected)?).map_err(|_| unexpected())?;
    let s = |x: &Value| x.as_str().unwrap_or("").trim().to_string();
    let mut seen = vec![false; n];
    let number = |x: &Value| x.as_u64().map(|k| k as usize).or_else(|| x.as_str().and_then(|s| s.trim().trim_start_matches(['B', 'b']).parse().ok()));
    // a block is {"n": 1, "note": "…"}, or only its number (1 or "B1")
    let block = |x: &Value| match x.get("n") {
        Some(n) => number(n).map(|k| (k, s(&x["note"]))),
        None => number(x).map(|k| (k, String::new())),
    };
    let mut steps: Vec<Step> = v["steps"]
        .as_array()
        .map(|a| {
            a.iter()
                // a step without title is dropped before its blocks are counted
                .filter(|st| !s(&st["title"]).is_empty())
                .map(|st| {
                    let (blocks, notes) = st["blocks"]
                        .as_array()
                        .map(|bs| bs.iter().filter_map(block).filter(|(k, _)| (1..=n).contains(k) && !std::mem::replace(&mut seen[k - 1], true)).map(|(k, note)| (k - 1, note)).unzip())
                        .unwrap_or_default();
                    Step {
                        title: s(&st["title"]),
                        text: s(&st["text"]),
                        risks: s(&st["risks"]),
                        places: st["places"].as_array().map(|p| p.iter().map(s).filter(|p| !p.is_empty()).collect()).unwrap_or_default(),
                        blocks,
                        notes,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let left: Vec<usize> = (0..n).filter(|&k| !seen[k]).collect();
    if !left.is_empty() {
        steps.push(Step {
            title: t!("Autres changements", "Other changes").into(),
            text: t!("Claude ne les a rangés dans aucune idée : à regarder de près.", "Claude put them in no idea: to look at closely.").into(),
            blocks: left,
            ..Default::default()
        });
    }
    if steps.is_empty() {
        return Err(unexpected());
    }
    Ok(Tour { title: s(&v["title"]), problem: s(&v["problem"]), idea: s(&v["idea"]), steps, decide: s(&v["decide"]) })
}

/// The blocks of a unified diff (a pull request).
pub fn blocks_of_diff(diff: &str) -> Vec<Block> {
    split::blocks(&split::parse_diff(diff))
}

/// How to put back what `discard` took out of a file.
#[derive(Clone, Debug)]
pub enum Undo {
    /// Hunks reversed in the file: applied again.
    Patch(String),
    /// A whole file: its bytes written again (None: it did not exist, removed again).
    File(String, Option<Vec<u8>>),
}

/// Where each block of `planned` is in `files`: (file, its hunks; None: the whole file).
fn locate(files: &[FileChange], planned: &[Block]) -> Result<Vec<(usize, Option<Vec<usize>>)>, String> {
    let mut out: Vec<(usize, Option<Vec<usize>>)> = Vec::new();
    for b in planned {
        let gone = || tf!("{} : ce bloc n'est plus dans les fichiers (modifiés depuis ?)", "{}: this block is no longer in the files (changed since?)", b.path);
        let i = files.iter().position(|f| f.path == b.path).ok_or_else(gone)?;
        let f = &files[i];
        let hunk = if b.changed.is_empty() || !f.partial_ok() {
            None
        } else {
            Some(f.hunks.iter().position(|h| (1..h.lines.len()).filter(|&j| h.is_change(j)).map(|j| &h.lines[j]).eq(b.changed.iter())).ok_or_else(gone)?)
        };
        match (out.iter_mut().find(|(k, _)| *k == i), hunk) {
            (Some((_, Some(hs))), Some(h)) => hs.push(h),
            (Some((_, whole)), None) => *whole = None,
            (Some(_), Some(_)) => {}
            (None, h) => out.push((i, h.map(|h| vec![h]))),
        }
    }
    Ok(out)
}

/// Takes the blocks `planned` (of an earlier reading of the changes not committed) out of the
/// files of `repo` (its top folder): the hunks reversed, the whole files back as in HEAD (a new
/// file removed). Returns how to put them back; on failure, what was done is put back.
pub fn discard(repo: &Path, planned: &[Block]) -> Result<Vec<Undo>, String> {
    let files = split::worktree_changes(repo)?;
    let mut undo = Vec::new();
    for (i, hunks) in locate(&files, planned)? {
        let f = &files[i];
        let r = match hunks {
            Some(hs) if f.partial_ok() => {
                let mut patch = f.header.join("\n") + "\n";
                for &h in &hs {
                    patch.push_str(&f.hunks[h].lines.join("\n"));
                    patch.push('\n');
                }
                git::run_with(repo, &["apply", "-R", "--recount", "-"], &[], Some(&patch)).map(|_| Undo::Patch(patch))
            }
            _ => {
                let path = repo.join(&f.path);
                let before = std::fs::read(&path).ok();
                let r = if f.kind == 'A' {
                    if !f.untracked {
                        let _ = git::run(repo, &["rm", "-q", "--cached", "-f", "--", &f.path]);
                    }
                    std::fs::remove_file(&path).map_err(|e| e.to_string())
                } else {
                    git::run(repo, &["checkout", "HEAD", "--", &f.path]).map(|_| ())
                };
                r.map(|()| Undo::File(f.path.clone(), before))
            }
        };
        match r {
            Ok(u) => undo.push(u),
            Err(e) => {
                let _ = restore(repo, &undo);
                return Err(format!("{} : {e}", f.path));
            }
        }
    }
    Ok(undo)
}

/// Puts back what `discard` took out (the last first).
pub fn restore(repo: &Path, undo: &[Undo]) -> Result<(), String> {
    for u in undo.iter().rev() {
        match u {
            Undo::Patch(patch) => {
                git::run_with(repo, &["apply", "--recount", "-"], &[], Some(patch))?;
            }
            Undo::File(path, Some(bytes)) => {
                let p = repo.join(path);
                if let Some(dir) = p.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(&p, bytes).map_err(|e| format!("{path} : {e}"))?;
            }
            Undo::File(path, None) => {
                let _ = std::fs::remove_file(repo.join(path));
            }
        }
    }
    Ok(())
}

/// Where a copy of what `discard` takes out is kept: closing the tour, or a crash, loses the way
/// back (↶), not the code. In the folder of Florm, `tours/`, one folder per idea taken out.
pub fn copies_dir() -> std::path::PathBuf {
    crate::config::config_dir().join("tours")
}

/// A copy of what was taken out, in a new folder of `copies_dir()` named after the repository
/// and the time: the patches (`git apply` puts them back) and the whole files as they were. The
/// copies older than 30 days are removed. Returns the folder.
pub fn save_copy(repo: &Path, step: &str, undo: &[Undo]) -> Result<std::path::PathBuf, String> {
    save_copy_in(&copies_dir(), repo, step, undo)
}

fn save_copy_in(root: &Path, repo: &Path, step: &str, undo: &[Undo]) -> Result<std::path::PathBuf, String> {
    let now = std::time::SystemTime::now();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|d| d.as_secs() > 30 * 86_400);
            if old {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let secs = now.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = root.join(format!("{name}-{secs}"));
    let files = dir.join("files");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut readme = format!("{step}\n{}\n", repo.display());
    for (k, u) in undo.iter().enumerate() {
        match u {
            Undo::Patch(p) => {
                std::fs::write(dir.join(format!("{}.patch", k + 1)), p).map_err(|e| e.to_string())?;
                readme.push_str(&format!("{}.patch : git apply {}.patch\n", k + 1, k + 1));
            }
            Undo::File(path, Some(bytes)) => {
                let to = files.join(path);
                if let Some(d) = to.parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                std::fs::write(&to, bytes).map_err(|e| e.to_string())?;
                readme.push_str(&format!("files/{path}\n"));
            }
            Undo::File(path, None) => readme.push_str(&format!("{path} : {}\n", t!("supprimé, à supprimer de nouveau", "deleted, to delete again"))),
        }
    }
    std::fs::write(dir.join("README.txt"), readme).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// "path:line" (or "path", "path:12-20") read back: the path and the line (1-based, 0 unknown).
pub fn place(s: &str) -> (String, usize) {
    let s = s.trim().trim_matches('`');
    match s.rsplit_once(':') {
        Some((p, l)) if !p.is_empty() => match l.split(['-', ',']).next().unwrap_or("").trim().parse() {
            Ok(n) => (p.to_string(), n),
            Err(_) => (s.to_string(), 0),
        },
        _ => (s.to_string(), 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_block_goes_into_one_step() {
        let text = r#"Voici : {"title": "Cache", "problem": "Lent", "idea": "Garder en mémoire",
            "steps": [{"title": "Le cache", "text": "t", "risks": "r", "places": ["a.py:3", ""], "blocks": [{"n": 1, "note": "ajoute"}, "B3", 3, 9]},
                      {"title": "Tests", "blocks": [1, 2]},
                      {"title": "", "blocks": [4]}],
            "decide": "d"}"#;
        let t = parse_tour(text, 5).unwrap();
        assert_eq!(t.title, "Cache");
        assert_eq!(t.steps.len(), 3);
        assert_eq!(t.steps[0].blocks, vec![0, 2]);
        assert_eq!(t.steps[0].notes, vec!["ajoute".to_string(), String::new()]);
        assert_eq!(t.steps[0].places, vec!["a.py:3".to_string()]);
        // 1 is already in the first step
        assert_eq!(t.steps[1].blocks, vec![1]);
        // 4 (its step has no title) and 5: in a step of their own
        assert_eq!(t.steps[2].blocks, vec![3, 4]);
        assert!(parse_tour("rien", 2).is_err());
        assert_eq!(parse_tour(r#"{"steps": [{"title": "Le commentaire", "blocks": []}]}"#, 0).unwrap().steps.len(), 1);
    }

    #[test]
    fn places() {
        assert_eq!(place("src/a.rs:12"), ("src/a.rs".into(), 12));
        assert_eq!(place("`src/a.rs:12-20`"), ("src/a.rs".into(), 12));
        assert_eq!(place("src/a.rs"), ("src/a.rs".into(), 0));
        assert_eq!(place("C:"), ("C:".into(), 0));
    }

    fn g(repo: &Path, args: &[&str]) -> String {
        git::run(repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"))
    }

    #[test]
    fn an_idea_is_dropped_and_put_back() {
        let repo = std::env::temp_dir().join(format!("florm_tour_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        g(&repo, &["init", "-q", "-b", "main"]);
        g(&repo, &["config", "user.name", "t"]);
        g(&repo, &["config", "user.email", "t@t"]);
        let lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        std::fs::write(repo.join("a.txt"), lines.join("\n") + "\n").unwrap();
        std::fs::write(repo.join("old.txt"), "old\n").unwrap();
        g(&repo, &["add", "."]);
        g(&repo, &["commit", "-qm", "base"]);
        let mut l2 = lines.clone();
        l2[2] = "top".into();
        l2.insert(30, "added below".into());
        let changed = l2.join("\n") + "\n";
        std::fs::write(repo.join("a.txt"), &changed).unwrap();
        std::fs::write(repo.join("new.txt"), "new\n").unwrap();
        std::fs::remove_file(repo.join("old.txt")).unwrap();
        let b = split::blocks(&split::worktree_changes(&repo).unwrap());
        assert_eq!(b.len(), 4);
        // the second hunk of a.txt, the new file and the deleted one: out
        let undo = discard(&repo, &[b[1].clone(), b[2].clone(), b[3].clone()]).unwrap();
        let mut only_top = lines.clone();
        only_top[2] = "top".into();
        assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), only_top.join("\n") + "\n");
        assert!(!repo.join("new.txt").exists());
        assert_eq!(std::fs::read_to_string(repo.join("old.txt")).unwrap(), "old\n");
        // the copy on the disk: the patch and the new file
        let copies = std::env::temp_dir().join(format!("florm_tour_copies_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&copies);
        let dir = save_copy_in(&copies, &repo, "Cache — Le cache", &undo).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("files/new.txt")).unwrap(), "new\n");
        assert!(std::fs::read_to_string(dir.join("1.patch")).unwrap().contains("+added below"));
        assert!(std::fs::read_to_string(dir.join("README.txt")).unwrap().starts_with("Cache — Le cache\n"));
        let _ = std::fs::remove_dir_all(&copies);
        // the first hunk too, then everything put back
        let undo1 = discard(&repo, &[b[0].clone()]).unwrap();
        assert!(g(&repo, &["status", "--porcelain"]).trim().is_empty());
        restore(&repo, &undo).unwrap();
        restore(&repo, &undo1).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), changed);
        assert!(repo.join("new.txt").exists());
        assert!(!repo.join("old.txt").exists());
        // gone since: an error, nothing changed
        assert!(discard(&repo, &[b[0].clone(), Block { path: "nope".into(), ..b[0].clone() }]).is_err());
        assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), changed);
        let _ = std::fs::remove_dir_all(&repo);
    }
}
