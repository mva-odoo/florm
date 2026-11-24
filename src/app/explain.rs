//! 🗺 Explain: changes (not committed, a pull request, a review comment) explained idea by idea by
//! Claude, in the Claude tab: a request sent to a conversation, nothing of its own to show.

use super::*;
use crate::github;

/// How the explanation is laid out, the same for changes and a pull request.
fn how() -> &'static str {
    t!(
        "idée par idée : d'abord le problème et l'idée en une ou deux phrases, puis pour chaque idée une courte explication, \
         les endroits du code concernés (chemin:ligne) et ce qui pourrait casser s'il y a un vrai risque. \
         Ne modifie aucun fichier : explique seulement.",
        "idea by idea: first the problem and the idea in one or two sentences, then for each idea a short explanation, \
         the places in the code (path:line) and what could break if there is a real risk. \
         Do not change any file: only explain."
    )
}

impl App {
    /// `text` sent to a conversation in `dir`: the one shown when it works there (`reuse_busy`: even
    /// when it already talked, it may know why the changes were made), else a new one.
    pub(super) fn claude_send_in(&mut self, dir: PathBuf, text: String, reuse_busy: bool) {
        let here = self.claude.dir.as_deref() == Some(dir.as_path());
        if !(here && (reuse_busy || self.claude.is_empty())) {
            self.claude_new_in(dir);
        }
        self.open_claude();
        if self.claude_send_text(text) {
            self.set_status(t!("✳ Demandé à Claude", "✳ Asked Claude"));
        }
    }

    /// 🗺 The changes not committed of the repository of `repo`.
    pub(super) fn explain_worktree(&mut self, repo: PathBuf) {
        let Some(root) = git::repo_root(&repo) else {
            self.set_status(t!("Pas un dépôt git", "Not a git repository"));
            return;
        };
        let text = tf!(
            "Explique-moi les changements non commités de ce dépôt (git status et git diff HEAD, nouveaux fichiers compris), {}",
            "Explain to me the uncommitted changes of this repository (git status and git diff HEAD, new files included), {}",
            how()
        );
        // the conversation shown may be the one that made them
        let dir = if self.claude.dir.as_deref() == Some(repo.as_path()) { repo } else { root };
        self.claude_send_in(dir, text, true);
    }

    /// 🗺 Pull request `d`.
    pub(super) fn explain_pr(&mut self, repo: PathBuf, d: &github::PrDetail) {
        let n = d.number;
        let text = tf!(
            "Explique-moi la pull request #{n} « {} » (gh pr view {n} et gh pr diff {n}), {}",
            "Explain to me pull request #{n} “{}” (gh pr view {n} and gh pr diff {n}), {}",
            d.title,
            how()
        );
        self.claude_send_in(repo, text, false);
    }

    /// 🗺 A conversation (`id`) of pull request `d`.
    pub(super) fn explain_thread(&mut self, repo: PathBuf, d: &github::PrDetail, id: &str) {
        let Some(t) = d.threads.iter().find(|t| t.id == id) else { return };
        let text = tf!(
            "Explique-moi ce commentaire de relecture de la pull request #{} « {} » : reformule ce que le relecteur demande, \
             dis s'il a raison et pourquoi, et ce que ça changerait. Ne modifie aucun fichier.\n\n{}",
            "Explain to me this review comment of pull request #{} “{}”: rephrase what the reviewer asks, \
             say whether they are right and why, and what it would change. Do not change any file.\n\n{}",
            d.number,
            d.title,
            super::pr_claude::thread_text(t)
        );
        self.claude_send_in(repo, text, false);
    }
}
