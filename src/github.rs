//! GitHub pull requests through the `gh` CLI: no token to create, `gh auth login` signs in with
//! the browser once and Florm uses that session (like git uses the user's own credentials).

use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;

fn gh(dir: &Path, args: &[&str]) -> Result<String, String> {
    gh_with(dir, args, None)
}

/// Like `gh`, with `input` written on its standard input (`gh api --input -`).
fn gh_with(dir: &Path, args: &[&str], input: Option<String>) -> Result<String, String> {
    let mut child = crate::os::command("gh")
        .args(args)
        .current_dir(dir)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .spawn()
        .map_err(|_| NOT_INSTALLED.to_string())?;
    if let (Some(mut stdin), Some(text)) = (child.stdin.take(), input) {
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if e.contains("gh auth login") { NOT_LOGGED.to_string() } else { e })
    }
}

pub const NOT_INSTALLED: &str = "La commande gh (GitHub CLI) est introuvable. Installe-la (paquet « github-cli » ou « gh »), puis connecte-toi avec : gh auth login";
pub const NOT_LOGGED: &str = "gh n'est pas connecté à GitHub. Lance une fois dans un terminal : gh auth login (connexion par le navigateur, sans token à créer)";

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Author {
    #[serde(default)]
    pub login: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrSummary {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub author: Author,
    pub head_ref_name: String,
    pub base_ref_name: String,
    #[serde(default)]
    pub is_draft: bool,
    pub state: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub review_decision: Option<String>,
}

/// Which pull requests: all, mine, those where my review is asked, those I reviewed.
pub const WHO: [(&str, &str); 4] = [("", "Toutes"), ("author:@me", "Les miennes"), ("review-requested:@me", "À relire par moi"), ("reviewed-by:@me", "Relues par moi")];

/// Pull requests of the repository of `dir`: "open", "closed", "merged" or "all"; `search`: a
/// GitHub search filter of `WHO`.
pub fn list(dir: &Path, state: &str, search: &str) -> Result<Vec<PrSummary>, String> {
    let mut args = vec![
        "pr",
        "list",
        "--state",
        state,
        "--limit",
        "100",
        "--json",
        "number,title,author,headRefName,baseRefName,isDraft,state,updatedAt,reviewDecision",
    ];
    if !search.is_empty() {
        args.extend(["--search", search]);
    }
    serde_json::from_str(&gh(dir, &args)?).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub oid: String,
    pub message_headline: String,
    #[serde(default)]
    pub authors: Vec<CommitAuthor>,
    #[serde(default)]
    pub committed_date: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CommitAuthor {
    #[serde(default)]
    pub login: String,
    #[serde(default)]
    pub name: String,
}

impl Commit {
    pub fn author(&self) -> String {
        self.authors.first().map(|a| if a.login.is_empty() { a.name.clone() } else { a.login.clone() }).unwrap_or_default()
    }
}

/// A general comment, or the text of a review (approved, changes requested…).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    #[serde(default)]
    pub author: Author,
    #[serde(default)]
    pub body: String,
    #[serde(default, alias = "submittedAt")]
    pub created_at: String,
    /// Reviews only: APPROVED, CHANGES_REQUESTED, COMMENTED…
    #[serde(default)]
    pub state: String,
}

/// A conversation on lines of a file.
#[derive(Clone, Debug)]
pub struct Thread {
    /// GraphQL id (to resolve it).
    pub id: String,
    pub path: String,
    /// Lines (1-based) in the latest version of the file; None when the code has changed since.
    pub line: Option<usize>,
    pub start_line: Option<usize>,
    pub original_line: Option<usize>,
    /// "RIGHT": on the new version of the file, "LEFT": on the old one (a removed line).
    pub side: String,
    pub resolved: bool,
    pub outdated: bool,
    pub comments: Vec<LineComment>,
}

#[derive(Clone, Debug)]
pub struct LineComment {
    pub id: String,
    /// REST id (to reply to it).
    pub db_id: u64,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub diff_hunk: String,
}

#[derive(Clone, Debug)]
pub struct PrDetail {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    /// When it was opened.
    pub created_at: String,
    pub state: String,
    pub is_draft: bool,
    pub head: String,
    /// Last commit of the pull request (where new comments on lines go).
    pub head_oid: String,
    pub base: String,
    pub commits: Vec<Commit>,
    /// General comments and reviews with a text, oldest first.
    pub comments: Vec<Comment>,
    pub threads: Vec<Thread>,
    /// Unified diff of the whole pull request.
    pub diff: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct View {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    author: Author,
    #[serde(default)]
    created_at: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    head_ref_name: String,
    #[serde(default)]
    head_ref_oid: String,
    base_ref_name: String,
    #[serde(default)]
    commits: Vec<Commit>,
    #[serde(default)]
    comments: Vec<Comment>,
    #[serde(default)]
    reviews: Vec<Comment>,
}

const THREADS: &str = "query($owner:String!,$repo:String!,$n:Int!){repository(owner:$owner,name:$repo){pullRequest(number:$n){\
reviewThreads(first:100){nodes{id isResolved isOutdated path line startLine originalLine diffSide \
comments(first:100){nodes{id databaseId author{login} body createdAt diffHunk}}}}}}}";

/// Everything shown for pull request `n`: description, commits, comments, threads on lines, diff.
pub fn detail(dir: &Path, n: u64) -> Result<PrDetail, String> {
    let ns = n.to_string();
    let v: View = serde_json::from_str(&gh(
        dir,
        &["pr", "view", &ns, "--json", "number,title,body,author,createdAt,state,isDraft,headRefName,headRefOid,baseRefName,commits,comments,reviews"],
    )?)
    .map_err(|e| e.to_string())?;
    let threads = gh(dir, &["api", "graphql", "-F", "owner={owner}", "-F", "repo={repo}", "-F", &format!("n={n}"), "-f", &format!("query={THREADS}")])
        .map(|out| parse_threads(&out))
        .unwrap_or_default();
    let diff = gh(dir, &["pr", "diff", &ns, "--color", "never"]).unwrap_or_else(|e| e);
    let mut comments: Vec<Comment> = v.comments.into_iter().chain(v.reviews.into_iter().filter(|r| !r.body.trim().is_empty() || r.state == "APPROVED" || r.state == "CHANGES_REQUESTED")).collect();
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(PrDetail {
        number: v.number,
        title: v.title,
        body: v.body,
        author: v.author.login,
        created_at: v.created_at,
        state: v.state,
        is_draft: v.is_draft,
        head: v.head_ref_name,
        head_oid: v.head_ref_oid,
        base: v.base_ref_name,
        commits: v.commits,
        comments,
        threads,
        diff,
    })
}

fn parse_threads(out: &str) -> Vec<Thread> {
    let v: Value = serde_json::from_str(out).unwrap_or(Value::Null);
    let nodes = v["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"].as_array().cloned().unwrap_or_default();
    let line = |x: &Value| x.as_u64().map(|n| n as usize);
    nodes
        .iter()
        .map(|t| Thread {
            id: t["id"].as_str().unwrap_or("").to_string(),
            path: t["path"].as_str().unwrap_or("").to_string(),
            line: line(&t["line"]),
            start_line: line(&t["startLine"]),
            original_line: line(&t["originalLine"]),
            side: t["diffSide"].as_str().unwrap_or("RIGHT").to_string(),
            resolved: t["isResolved"].as_bool().unwrap_or(false),
            outdated: t["isOutdated"].as_bool().unwrap_or(false),
            comments: t["comments"]["nodes"]
                .as_array()
                .map(|cs| {
                    cs.iter()
                        .map(|c| LineComment {
                            id: c["id"].as_str().unwrap_or("").to_string(),
                            db_id: c["databaseId"].as_u64().unwrap_or(0),
                            author: c["author"]["login"].as_str().unwrap_or("").to_string(),
                            body: c["body"].as_str().unwrap_or("").to_string(),
                            created_at: c["createdAt"].as_str().unwrap_or("").to_string(),
                            diff_hunk: c["diffHunk"].as_str().unwrap_or("").to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .filter(|t: &Thread| !t.comments.is_empty())
        .collect()
}

/// Diff of one commit of the pull request.
pub fn commit_diff(dir: &Path, sha: &str) -> Result<String, String> {
    gh(dir, &["api", "-H", "Accept: application/vnd.github.diff", &format!("repos/{{owner}}/{{repo}}/commits/{sha}")])
}

/// `gh pr checkout n`: the branch of the pull request in the working copy.
pub fn checkout(dir: &Path, n: u64) -> Result<String, String> {
    gh(dir, &["pr", "checkout", &n.to_string()]).map(|o| o.trim().to_string())
}

/// Opens the pull request in the browser.
pub fn open_web(dir: &Path, n: u64) -> Result<String, String> {
    gh(dir, &["pr", "view", &n.to_string(), "--web"])
}

/// Pull requests made from branch `branch` (any state, newest first).
pub fn for_branch(dir: &Path, branch: &str) -> Result<Vec<PrSummary>, String> {
    let out = gh(
        dir,
        &["pr", "list", "--head", branch, "--state", "all", "--limit", "10", "--json", "number,title,author,headRefName,baseRefName,isDraft,state,updatedAt,reviewDecision"],
    )?;
    serde_json::from_str(&out).map_err(|e| e.to_string())
}

/// A pull request containing a commit.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkedPr {
    pub number: u64,
    pub title: String,
    /// "OPEN", "MERGED" or "CLOSED", as in `PrSummary`.
    pub state: String,
}

/// Pull requests of `repo` ("odoo/odoo") that contain commit `sha` (GitHub also gives those of
/// the forks: left out).
pub fn for_commit(dir: &Path, repo: &str, sha: &str) -> Result<Vec<LinkedPr>, String> {
    let out = gh(dir, &["api", &format!("repos/{repo}/commits/{sha}/pulls")])?;
    Ok(parse_linked(&out, repo))
}

fn parse_linked(out: &str, repo: &str) -> Vec<LinkedPr> {
    let v: Value = serde_json::from_str(out).unwrap_or(Value::Null);
    v.as_array()
        .map(|a| {
            a.iter()
                .filter(|p| p["base"]["repo"]["full_name"].as_str() == Some(repo))
                .map(|p| LinkedPr {
                    number: p["number"].as_u64().unwrap_or(0),
                    title: p["title"].as_str().unwrap_or("").to_string(),
                    state: match (p["state"].as_str(), p["merged_at"].is_string()) {
                        (_, true) => "MERGED".into(),
                        (Some("open"), _) => "OPEN".into(),
                        _ => "CLOSED".into(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Pull requests named in a commit message by the merge bot: "closes odoo/odoo#123" (or "#123").
pub fn closed_by_message(message: &str, repo: &str) -> Vec<u64> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"(?im)^\s*(?:closes|fixes|fixed|closed)\s+([\w.-]+/[\w.-]+)?#(\d+)").unwrap());
    re.captures_iter(message)
        .filter(|c| c.get(1).map_or(true, |r| r.as_str().eq_ignore_ascii_case(repo)))
        .filter_map(|c| c[2].parse().ok())
        .collect()
}

/// The repository pull requests go to ("odoo/odoo") and its default branch.
pub fn base_repo(dir: &Path) -> Result<(String, String), String> {
    let out = gh(dir, &["repo", "view", "--json", "nameWithOwner,defaultBranchRef"])?;
    let v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok((v["nameWithOwner"].as_str().unwrap_or("").to_string(), v["defaultBranchRef"]["name"].as_str().unwrap_or("").to_string()))
}

/// Opens a pull request of `head` ("branch", or "owner:branch" from a fork) into `base` of
/// `repo`; returns its address.
pub fn create(dir: &Path, repo: &str, base: &str, head: &str, title: &str, body: &str, draft: bool) -> Result<String, String> {
    let mut args = vec!["pr", "create", "--repo", repo, "--base", base, "--head", head, "--title", title, "--body", body];
    if draft {
        args.push("--draft");
    }
    gh(dir, &args).map(|o| o.trim().lines().last().unwrap_or("").to_string())
}

/// The base branch a branch is most likely made from: the longest one its name starts with
/// ("17.0-fix-x" → "17.0", "saas-17.2-y" → "saas-17.2"), else the default branch.
pub fn guess_base(branch: &str, bases: &[String], default: &str) -> String {
    bases
        .iter()
        .filter(|b| branch.len() > b.len() && branch.starts_with(b.as_str()) && branch[b.len()..].starts_with(['-', '_', '/']))
        .max_by_key(|b| b.len())
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

/// Number of a pull request from its address (".../pull/123").
pub fn number_of(url: &str) -> Option<u64> {
    url.trim().rsplit('/').next()?.parse().ok()
}

/// A comment on the whole pull request.
pub fn comment(dir: &Path, n: u64, body: &str) -> Result<String, String> {
    gh(dir, &["pr", "comment", &n.to_string(), "--body", body])
}

/// A reply in the conversation of a comment on lines.
pub fn reply(dir: &Path, n: u64, comment_id: u64, body: &str) -> Result<String, String> {
    let url = format!("repos/{{owner}}/{{repo}}/pulls/{n}/comments/{comment_id}/replies");
    gh(dir, &["api", "-X", "POST", &url, "-f", &format!("body={body}")])
}

/// A new comment on line `line` of `path` ("RIGHT": the new version, "LEFT": the old one) at `commit`.
pub fn line_comment(dir: &Path, n: u64, commit: &str, path: &str, line: usize, side: &str, body: &str) -> Result<String, String> {
    let url = format!("repos/{{owner}}/{{repo}}/pulls/{n}/comments");
    gh(
        dir,
        &[
            "api",
            "-X",
            "POST",
            &url,
            "-f",
            &format!("body={body}"),
            "-f",
            &format!("commit_id={commit}"),
            "-f",
            &format!("path={path}"),
            "-F",
            &format!("line={line}"),
            "-f",
            &format!("side={side}"),
        ],
    )
}

/// A review (without approving nor asking for changes) at `commit`: `body` on the whole pull
/// request and comments on lines (path, line, "RIGHT" / "LEFT", text), all sent at once.
pub fn review(dir: &Path, n: u64, commit: &str, body: &str, comments: &[(String, usize, String, String)]) -> Result<String, String> {
    let url = format!("repos/{{owner}}/{{repo}}/pulls/{n}/reviews");
    let comments: Vec<Value> = comments.iter().map(|(path, line, side, text)| serde_json::json!({"path": path, "line": line, "side": side, "body": text})).collect();
    let mut input = serde_json::json!({"commit_id": commit, "event": "COMMENT", "comments": comments});
    if !body.is_empty() {
        input["body"] = body.into();
    }
    gh_with(dir, &["api", "-X", "POST", &url, "--input", "-"], Some(input.to_string())).map(|_| String::new())
}

/// Marks a conversation on lines as resolved (or not any more).
pub fn resolve(dir: &Path, thread_id: &str, resolved: bool) -> Result<String, String> {
    let m = if resolved { "resolveReviewThread" } else { "unresolveReviewThread" };
    let q = format!("mutation($id:ID!){{{m}(input:{{threadId:$id}}){{thread{{isResolved}}}}}}");
    gh(dir, &["api", "graphql", "-f", &format!("id={thread_id}"), "-f", &format!("query={q}")])
}

/// A part of a comment: text (Markdown) or a suggested replacement of the commented lines.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Suggestion(String),
}

/// Splits a comment body around its ```suggestion blocks.
pub fn parts(body: &str) -> Vec<Part> {
    let body = body.replace("\r\n", "\n");
    let mut out = Vec::new();
    let mut text = String::new();
    let mut sugg: Option<String> = None;
    for l in body.split('\n') {
        match &mut sugg {
            None if l.trim_start().starts_with("```suggestion") => {
                if !text.trim().is_empty() {
                    out.push(Part::Text(std::mem::take(&mut text)));
                }
                text.clear();
                sugg = Some(String::new());
            }
            None => {
                text.push_str(l);
                text.push('\n');
            }
            Some(s) if l.trim() == "```" => {
                out.push(Part::Suggestion(s.strip_suffix('\n').unwrap_or(s).to_string()));
                sugg = None;
            }
            Some(s) => {
                s.push_str(l);
                s.push('\n');
            }
        }
    }
    if !text.trim().is_empty() {
        out.push(Part::Text(text.trim_end().to_string()));
    }
    out
}

/// The commented lines as the reviewer saw them: the last `n` lines of the new side of the hunk.
pub fn commented_lines(diff_hunk: &str, n: usize) -> Vec<String> {
    let new: Vec<String> = diff_hunk
        .lines()
        .skip(1)
        .filter(|l| !l.starts_with('-') && !l.starts_with('\\'))
        .map(|l| l.get(1..).unwrap_or("").to_string())
        .collect();
    new[new.len().saturating_sub(n)..].to_vec()
}

/// The last `n` lines of a hunk with their numbers: (old, new, '+' / '-' / ' ', text).
pub fn hunk_tail(diff_hunk: &str, n: usize) -> Vec<(Option<usize>, Option<usize>, char, String)> {
    let mut lines = diff_hunk.lines();
    let head = lines.next().unwrap_or("");
    // "@@ -12,5 +12,6 @@"
    let num = |sign: char| -> usize {
        head.split_whitespace()
            .find_map(|w| w.strip_prefix(sign))
            .and_then(|w| w.split(',').next())
            .and_then(|w| w.parse().ok())
            .unwrap_or(1)
    };
    let (mut old, mut new) = (num('-'), num('+'));
    let mut out = Vec::new();
    for l in lines.filter(|l| !l.starts_with('\\')) {
        let kind = l.chars().next().unwrap_or(' ');
        let text = l.get(1..).unwrap_or("").to_string();
        match kind {
            '+' => {
                out.push((None, Some(new), '+', text));
                new += 1;
            }
            '-' => {
                out.push((Some(old), None, '-', text));
                old += 1;
            }
            _ => {
                out.push((Some(old), Some(new), ' ', text));
                old += 1;
                new += 1;
            }
        }
    }
    out.split_off(out.len().saturating_sub(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_in_a_comment() {
        let b = "Plutôt comme ça :\r\n```suggestion\n    x = 1\n    y = 2\n```\nmerci";
        assert_eq!(
            parts(b),
            vec![Part::Text("Plutôt comme ça :\n".into()), Part::Suggestion("    x = 1\n    y = 2".into()), Part::Text("merci".into())]
        );
        // an empty suggestion deletes the lines
        assert_eq!(parts("```suggestion\n```"), vec![Part::Suggestion(String::new())]);
        assert_eq!(parts("rien"), vec![Part::Text("rien".into())]);
    }

    #[test]
    fn lines_of_the_hunk() {
        let h = "@@ -1,4 +1,4 @@\n a\n-b\n+c\n d";
        assert_eq!(commented_lines(h, 2), ["c", "d"]);
        assert_eq!(commented_lines(h, 9), ["a", "c", "d"]);
    }

    #[test]
    fn base_and_links() {
        let bases: Vec<String> = ["master", "17.0", "saas-17.2", "saas-17"].iter().map(|s| s.to_string()).collect();
        assert_eq!(guess_base("17.0-fix-stuff-mva", &bases, "master"), "17.0");
        assert_eq!(guess_base("saas-17.2-imp-x", &bases, "master"), "saas-17.2");
        assert_eq!(guess_base("20-test", &bases, "master"), "master");
        assert_eq!(guess_base("17.0", &bases, "master"), "master");
        assert_eq!(number_of("https://github.com/odoo/odoo/pull/123\n"), Some(123));
        let base = r#"{"repo":{"full_name":"odoo/odoo"}}"#;
        let fork = r#"{"repo":{"full_name":"someone/odoo"}}"#;
        let l = parse_linked(
            &format!(r#"[{{"number":5,"title":"x","state":"closed","merged_at":"2026","base":{base}}},{{"number":6,"title":"y","state":"open","merged_at":null,"base":{base}}},{{"number":1,"title":"z","state":"open","base":{fork}}}]"#),
            "odoo/odoo",
        );
        assert_eq!(l.iter().map(|p| (p.number, p.state.as_str())).collect::<Vec<_>>(), [(5, "MERGED"), (6, "OPEN")]);
        let msg = "[FIX] x: y\n\nblah\n\ncloses odoo/odoo#231452\n\nX-original-commit: abc\nSigned-off-by: a\ncloses other/repo#1\nCloses #7";
        assert_eq!(closed_by_message(msg, "odoo/odoo"), [231452, 7]);
    }

    #[test]
    fn numbered_hunk() {
        let t = hunk_tail("@@ -10,3 +10,3 @@ def f():\n a\n-b\n+c", 2);
        assert_eq!(t, vec![(Some(11), None, '-', "b".to_string()), (None, Some(11), '+', "c".to_string())]);
    }

    #[test]
    fn threads_from_graphql() {
        let out = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[
            {"isResolved":false,"isOutdated":false,"path":"a.py","line":12,"startLine":10,"originalLine":12,
             "comments":{"nodes":[{"id":"C1","author":{"login":"bob"},"body":"hm","createdAt":"2026","diffHunk":"@@","url":"u"}]}},
            {"isResolved":true,"isOutdated":true,"path":"b.py","line":null,"startLine":null,"originalLine":3,"comments":{"nodes":[]}}]}}}}}"#;
        let t = parse_threads(out);
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].line, t[0].start_line, t[0].comments[0].author.as_str()), (Some(12), Some(10), "bob"));
    }
}
