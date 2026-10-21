//! Interactive rebase without a text editor: the plan (order and action of every commit, new
//! messages) is prepared in the UI, then git runs it with Florm as sequence editor (`PLAN_FLAG`).

use crate::git;
use std::path::{Path, PathBuf};

/// Argument that makes florm replace git's todo list by a prepared plan: `florm PLAN_FLAG <plan> <todo>`.
pub const PLAN_FLAG: &str = "--florm-rebase-plan";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
    /// Stop on the commit and split it in the assistant (`edit` + `git reset HEAD~1`).
    Split,
}

impl Action {
    pub const ALL: [Action; 6] = [Action::Pick, Action::Reword, Action::Squash, Action::Fixup, Action::Split, Action::Drop];

    pub fn label(self) -> &'static str {
        match self {
            Action::Pick => "Garder",
            Action::Reword => "Changer le message",
            Action::Squash => "Fusionner avec le précédent",
            Action::Fixup => "Fusionner (sans son message)",
            Action::Drop => "Supprimer",
            Action::Split => "Découper en plusieurs commits",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Step {
    pub hash: String,
    pub short: String,
    pub subject: String,
    /// Full message (edited for Reword).
    pub message: String,
    pub action: Action,
}

/// Commits from `from` (included) to HEAD, oldest first, after checking the rebase is safe.
pub fn load(repo: &Path, from: &str) -> Result<Vec<Step>, String> {
    if crate::split::rebase_in_progress(repo) {
        return Err("Un rebase est déjà en cours dans ce dépôt : termine-le ou annule-le d'abord.".into());
    }
    if !git::stdout(repo, &["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty() {
        return Err("Le dépôt a des modifications non validées : valide-les ou mets-les de côté (stash) d'abord.".into());
    }
    if git::run(repo, &["merge-base", "--is-ancestor", from, "HEAD"]).is_err() {
        return Err("Le commit doit faire partie de la branche courante.".into());
    }
    if git::stdout(repo, &["rev-list", "--parents", "-n", "1", from])?.split_whitespace().count() < 2 {
        return Err("Le premier commit du dépôt ne peut pas être réécrit ici.".into());
    }
    let range = format!("{from}^..HEAD");
    if !git::stdout(repo, &["rev-list", "--merges", &range])?.trim().is_empty() {
        return Err("Il y a des merges dans ces commits : le rebase les aplatirait, il est donc refusé.".into());
    }
    let out = git::stdout(repo, &["log", "--reverse", "--format=%H%x1f%h%x1f%s%x1f%B%x1e", &range])?;
    Ok(out
        .split('\x1e')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim_start_matches('\n').splitn(4, '\x1f').collect();
            (f.len() == 4).then(|| Step {
                hash: f[0].into(),
                short: f[1].into(),
                subject: f[2].into(),
                message: f[3].trim().into(),
                action: Action::Pick,
            })
        })
        .collect())
}

/// Why the plan cannot run (a fusion needs a kept commit before it).
pub fn check(steps: &[Step]) -> Result<(), String> {
    match steps.iter().find(|s| s.action != Action::Drop) {
        None => Err("Tous les commits seraient supprimés.".into()),
        Some(s) if matches!(s.action, Action::Squash | Action::Fixup) => {
            Err(format!("« {} » ne peut pas fusionner : il n'y a pas de commit gardé avant lui.", s.subject))
        }
        _ if steps.iter().filter(|s| s.action == Action::Split).count() > 1 => {
            Err("Un seul commit à découper par rebase (découpe les autres ensuite).".into())
        }
        _ => Ok(()),
    }
}

/// The todo list for git; new messages are written in `dir`.
pub fn todo(steps: &[Step], dir: &Path) -> Result<String, String> {
    let mut out = String::new();
    for (i, s) in steps.iter().enumerate() {
        match s.action {
            Action::Drop => out.push_str(&format!("drop {} {}\n", s.hash, s.subject)),
            Action::Squash => out.push_str(&format!("squash {} {}\n", s.hash, s.subject)),
            Action::Fixup => out.push_str(&format!("fixup {} {}\n", s.hash, s.subject)),
            Action::Pick => out.push_str(&format!("pick {} {}\n", s.hash, s.subject)),
            Action::Split => out.push_str(&format!("edit {} {}\n", s.hash, s.subject)),
            Action::Reword => {
                let msg = dir.join(format!("msg{i}.txt"));
                std::fs::write(&msg, &s.message).map_err(|e| e.to_string())?;
                out.push_str(&format!("pick {} {}\n", s.hash, s.subject));
                out.push_str(&format!("exec git commit --amend --allow-empty -q -F \"{}\"\n", msg.display().to_string().replace('\\', "/")));
            }
        }
    }
    Ok(out)
}

/// Runs the plan on the commits after `from`'s parent. `self_exe` is the program given to git
/// as sequence editor (florm itself; tests pass a shell command).
pub fn run(repo: &Path, from: &str, steps: &[Step], editor: impl FnOnce(&Path) -> String) -> Result<String, String> {
    check(steps)?;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let plan = dir.join("plan.txt");
    std::fs::write(&plan, todo(steps, &dir)?).map_err(|e| e.to_string())?;
    let seq = editor(&plan);
    let base = format!("{from}^");
    let r = git::run_with(
        repo,
        &["rebase", "-i", "--no-autosquash", &base],
        &[("GIT_SEQUENCE_EDITOR", &seq), ("GIT_EDITOR", "true")],
        None,
    );
    if r.is_ok() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    // stopped on the commit to split: its changes go back to the working tree, for the assistant
    if r.is_ok() && steps.iter().any(|s| s.action == Action::Split) && crate::split::stopped_commit(repo).is_some() {
        git::run(repo, &["reset", "-q", "HEAD~1"])?;
    }
    r
}

/// Whether the plan stops on a commit to split (the assistant opens when git stops on it).
pub fn splits(steps: &[Step]) -> bool {
    steps.iter().any(|s| s.action == Action::Split)
}

/// Sequence editor running this executable with `PLAN_FLAG`.
pub fn self_editor(plan: &Path) -> String {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "florm".into());
    format!("\"{}\" {PLAN_FLAG} \"{}\"", exe.replace('\\', "/"), plan.display().to_string().replace('\\', "/"))
}

fn scratch_dir() -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!("florm-rebase-{}-{n}", std::process::id()))
}

/// `florm PLAN_FLAG <plan> <todo>`: git's todo list becomes the prepared plan.
pub fn write_plan(plan: &Path, todo: &Path) -> Result<(), String> {
    std::fs::copy(plan, todo).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_reword_squash_drop() {
        let repo = std::env::temp_dir().join(format!("florm_rebase_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let g = |args: &[&str]| git::run(&repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.name", "t"]);
        g(&["config", "user.email", "t@t"]);
        for (f, m) in [("base", "base"), ("a", "add a"), ("b", "add b"), ("c", "add c"), ("d", "add d")] {
            std::fs::write(repo.join(f), f).unwrap();
            g(&["add", "."]);
            g(&["commit", "-qm", m]);
        }
        let first = g(&["rev-parse", "HEAD~3"]);
        let mut steps = load(&repo, first.trim()).unwrap();
        assert_eq!(steps.iter().map(|s| s.subject.as_str()).collect::<Vec<_>>(), ["add a", "add b", "add c", "add d"]);
        // d first (renamed), then a with b squashed into it, c dropped
        let d = steps.remove(3);
        steps.insert(0, d);
        steps[0].action = Action::Reword;
        steps[0].message = "D renommé\n\navec un corps".into();
        steps[2].action = Action::Fixup; // b into a
        steps[3].action = Action::Drop; // c
        // the shell sequence editor copies the plan, like `florm PLAN_FLAG`
        run(&repo, first.trim(), &steps, |plan| format!("cp \"{}\"", plan.display())).unwrap();
        assert_eq!(g(&["log", "--format=%s"]), "add a\nD renommé\nbase");
        assert!(g(&["log", "-1", "--format=%B", "HEAD~1"]).contains("avec un corps"));
        assert_eq!(g(&["ls-files"]), "a\nb\nbase\nd");

        // stop on a commit to split it: its changes are back in the working tree
        let mut steps = load(&repo, g(&["rev-parse", "HEAD~1"]).trim()).unwrap();
        steps[0].action = Action::Split;
        assert!(splits(&steps));
        run(&repo, steps[0].hash.clone().as_str(), &steps, |plan| format!("cp \"{}\"", plan.display())).unwrap();
        assert!(crate::split::rebase_in_progress(&repo));
        assert_eq!(g(&["status", "--porcelain"]), "?? d");
        crate::split::abort(&repo).unwrap();
        assert_eq!(g(&["log", "--format=%s"]), "add a\nD renommé\nbase");

        // a fusion needs a kept commit before it
        let mut bad = load(&repo, g(&["rev-parse", "HEAD~1"]).trim()).unwrap();
        bad[0].action = Action::Squash;
        assert!(check(&bad).is_err());
        let _ = std::fs::remove_dir_all(&repo);
    }
}
