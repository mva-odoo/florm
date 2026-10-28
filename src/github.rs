//! GitHub pull requests through the `gh` CLI: no token to create, `gh auth login` signs in with
//! the browser once and Florm uses that session (like git uses the user's own credentials).

use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
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
        .map_err(|_| not_installed())?;
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
        Err(if e.contains("gh auth login") { not_logged().to_string() } else { e })
    }
}

pub fn not_installed() -> String {
    t!(
        "La commande gh (GitHub CLI) est introuvable. Installe-la (paquet « github-cli » ou « gh »), puis connecte-toi avec : gh auth login",
        "The gh command (GitHub CLI) was not found. Install it (package “github-cli” or “gh”), then log in with: gh auth login"
    )
    .to_string()
}
pub fn not_logged() -> &'static str {
    t!("gh n'est pas connecté à GitHub. Lance une fois dans un terminal : gh auth login (connexion par le navigateur, sans token à créer)", "gh is not logged in to GitHub. Run once in a terminal: gh auth login (login through the browser, no token to create)")
}

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
    #[serde(default)]
    pub url: String,
}

/// Fields of `PrSummary` for `gh pr list --json`.
const SUMMARY: &str = "number,title,author,headRefName,baseRefName,isDraft,state,updatedAt,reviewDecision,url";

/// Which pull requests: all, mine, those where my review is asked, those I reviewed.
pub const WHO: [(&str, &str, &str); 4] = [
    ("", "Toutes", "All"),
    ("author:@me", "Les miennes", "Mine"),
    ("review-requested:@me", "À relire par moi", "To review by me"),
    ("reviewed-by:@me", "Relues par moi", "Reviewed by me"),
];

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
        SUMMARY,
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
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    /// GraphQL id: "IC_…" for a comment, "PRR_…" for a review.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author: Author,
    #[serde(default)]
    pub body: String,
    #[serde(default, alias = "submittedAt")]
    pub created_at: String,
    /// Reviews only: APPROVED, CHANGES_REQUESTED, COMMENTED…
    #[serde(default)]
    pub state: String,
    /// Written by me (to change or delete it).
    #[serde(skip)]
    pub mine: bool,
    #[serde(skip)]
    pub reactions: Vec<Reaction>,
}

impl Comment {
    /// A review (its text can be changed, not deleted).
    pub fn is_review(&self) -> bool {
        self.id.starts_with("PRR_")
    }
}

/// A reaction to a comment: its kind ("THUMBS_UP"…), how many gave it, and whether I did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reaction {
    pub content: String,
    pub count: u64,
    pub mine: bool,
}

/// The reactions of GitHub and their emoji.
pub const REACTIONS: [(&str, &str); 8] =
    [("THUMBS_UP", "👍"), ("THUMBS_DOWN", "👎"), ("LAUGH", "😄"), ("HOORAY", "🎉"), ("CONFUSED", "😕"), ("HEART", "❤"), ("ROCKET", "🚀"), ("EYES", "👀")];

pub fn emoji(content: &str) -> &'static str {
    REACTIONS.iter().find(|(c, _)| *c == content).map_or("?", |(_, e)| e)
}

/// My reaction `content` added (or taken back) in `list`, before GitHub answers.
pub fn toggle_reaction(list: &mut Vec<Reaction>, content: &str, add: bool) {
    match list.iter_mut().find(|r| r.content == content) {
        Some(r) if r.mine != add => {
            r.mine = add;
            r.count = if add { r.count + 1 } else { r.count.saturating_sub(1) };
        }
        Some(_) => {}
        None if add => list.push(Reaction { content: content.into(), count: 1, mine: true }),
        None => {}
    }
    list.retain(|r| r.count > 0);
}

/// A check of the last commit: a GitHub Actions job or a status sent by another service.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Check {
    pub name: String,
    /// The workflow of a GitHub Actions job.
    pub workflow: String,
    pub state: CheckState,
    pub url: String,
    pub description: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckState {
    Fail,
    #[default]
    Pending,
    Pass,
    Skip,
}

impl Check {
    /// (run, job) of a GitHub Actions job, from its address (".../actions/runs/1/job/2").
    pub fn job(&self) -> Option<(u64, u64)> {
        let (_, rest) = self.url.split_once("/actions/runs/")?;
        let mut it = rest.split('/');
        let run = it.next()?.parse().ok()?;
        (it.next()? == "job").then_some(())?;
        Some((run, it.next()?.split(['?', '#']).next()?.parse().ok()?))
    }
}

/// The checks of `statusCheckRollup` (`gh pr view --json`), failed ones first.
fn parse_checks(rollup: &[Value]) -> Vec<Check> {
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    let mut out: Vec<Check> = rollup
        .iter()
        .map(|c| {
            if c["__typename"] == "CheckRun" {
                let state = match (c["status"].as_str(), c["conclusion"].as_str()) {
                    (Some("COMPLETED"), Some("SUCCESS")) => CheckState::Pass,
                    (Some("COMPLETED"), Some("NEUTRAL" | "SKIPPED")) => CheckState::Skip,
                    (Some("COMPLETED"), _) => CheckState::Fail,
                    _ => CheckState::Pending,
                };
                Check { name: s(&c["name"]), workflow: s(&c["workflowName"]), state, url: s(&c["detailsUrl"]), description: String::new() }
            } else {
                let state = match c["state"].as_str() {
                    Some("SUCCESS") => CheckState::Pass,
                    Some("FAILURE" | "ERROR") => CheckState::Fail,
                    _ => CheckState::Pending,
                };
                Check { name: s(&c["context"]), workflow: String::new(), state, url: s(&c["targetUrl"]), description: s(&c["description"]) }
            }
        })
        .collect();
    out.sort_by(|a, b| a.state.cmp(&b.state).then_with(|| a.name.cmp(&b.name)));
    out
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Label {
    pub name: String,
    #[serde(default)]
    pub color: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Milestone {
    #[serde(default)]
    title: String,
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
    pub mine: bool,
    pub reactions: Vec<Reaction>,
}

#[derive(Clone, Debug, Default)]
pub struct PrDetail {
    /// GraphQL id (files viewed, reactions).
    pub id: String,
    pub url: String,
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
    /// APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED or empty.
    pub review_decision: String,
    /// MERGEABLE, CONFLICTING or UNKNOWN (still computed by GitHub).
    pub mergeable: String,
    /// CLEAN, BEHIND, BLOCKED, DIRTY, DRAFT, UNSTABLE, HAS_HOOKS or UNKNOWN.
    pub merge_state: String,
    /// Reviewers asked: logins, "org/team" for a team.
    pub reviewers: Vec<String>,
    pub assignees: Vec<String>,
    pub labels: Vec<Label>,
    /// Title of the milestone, empty when none.
    pub milestone: String,
    pub checks: Vec<Check>,
    /// My login.
    pub viewer: String,
    /// Reactions to the description.
    pub reactions: Vec<Reaction>,
    /// Files marked as viewed by me, by path.
    pub viewed: HashSet<String>,
}

impl PrDetail {
    /// Reactions of the description (`id` of the pull request) or of a comment.
    pub fn reactions_mut(&mut self, id: &str) -> Option<&mut Vec<Reaction>> {
        if id == self.id {
            return Some(&mut self.reactions);
        }
        if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
            return Some(&mut c.reactions);
        }
        self.threads.iter_mut().flat_map(|t| t.comments.iter_mut()).find(|c| c.id == id).map(|c| &mut c.reactions)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct View {
    #[serde(default)]
    id: String,
    #[serde(default)]
    url: String,
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
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    mergeable: String,
    #[serde(default)]
    merge_state_status: String,
    #[serde(default)]
    review_requests: Vec<Value>,
    #[serde(default)]
    assignees: Vec<Author>,
    #[serde(default)]
    labels: Vec<Label>,
    #[serde(default)]
    milestone: Option<Milestone>,
    #[serde(default)]
    status_check_rollup: Vec<Value>,
}

/// What `gh pr view` does not give: who I am, the conversations on lines, reactions, files viewed.
const EXTRA: &str = "query($owner:String!,$repo:String!,$n:Int!){viewer{login} repository(owner:$owner,name:$repo){pullRequest(number:$n){\
reactionGroups{...R} \
comments(last:100){nodes{id viewerDidAuthor reactionGroups{...R}}} \
reviews(last:100){nodes{id viewerDidAuthor reactionGroups{...R}}} \
files(first:100){nodes{path viewerViewedState}} \
reviewThreads(first:100){nodes{id isResolved isOutdated path line startLine originalLine diffSide \
comments(first:100){nodes{id databaseId author{login} body createdAt diffHunk viewerDidAuthor reactionGroups{...R}}}}}}}} \
fragment R on ReactionGroup{content viewerHasReacted reactors{totalCount}}";

/// Everything shown for pull request `n`: description, commits, comments, threads on lines, diff.
pub fn detail(dir: &Path, n: u64) -> Result<PrDetail, String> {
    let ns = n.to_string();
    let v: View = serde_json::from_str(&gh(
        dir,
        &[
            "pr",
            "view",
            &ns,
            "--json",
            "id,url,number,title,body,author,createdAt,state,isDraft,headRefName,headRefOid,baseRefName,commits,comments,reviews,\
reviewDecision,mergeable,mergeStateStatus,reviewRequests,assignees,labels,milestone,statusCheckRollup",
        ],
    )?)
    .map_err(|e| e.to_string())?;
    let extra: Value = gh(dir, &["api", "graphql", "-F", "owner={owner}", "-F", "repo={repo}", "-F", &format!("n={n}"), "-f", &format!("query={EXTRA}")])
        .ok()
        .and_then(|out| serde_json::from_str(&out).ok())
        .unwrap_or(Value::Null);
    let threads = parse_threads(&extra);
    let diff = gh(dir, &["pr", "diff", &ns, "--color", "never"]).unwrap_or_else(|e| e);
    let mut comments: Vec<Comment> = v.comments.into_iter().chain(v.reviews.into_iter().filter(|r| !r.body.trim().is_empty() || r.state == "APPROVED" || r.state == "CHANGES_REQUESTED")).collect();
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let pr = &extra["data"]["repository"]["pullRequest"];
    // mine and reactions, by id
    let mut extra_of: HashMap<&str, (bool, Vec<Reaction>)> = HashMap::new();
    for c in ["comments", "reviews"].iter().filter_map(|k| pr[k]["nodes"].as_array()).flatten() {
        extra_of.insert(c["id"].as_str().unwrap_or(""), (c["viewerDidAuthor"].as_bool().unwrap_or(false), parse_reactions(&c["reactionGroups"])));
    }
    for c in comments.iter_mut() {
        if let Some((mine, r)) = extra_of.remove(c.id.as_str()) {
            (c.mine, c.reactions) = (mine, r);
        }
    }
    let owner = v.url.split('/').nth(3).unwrap_or("").to_string();
    let reviewers = v
        .review_requests
        .iter()
        .filter_map(|r| match r["slug"].as_str() {
            Some(team) => Some(format!("{owner}/{team}")),
            None => r["login"].as_str().map(String::from),
        })
        .collect();
    let viewed = pr["files"]["nodes"]
        .as_array()
        .map(|a| a.iter().filter(|f| f["viewerViewedState"] == "VIEWED").filter_map(|f| f["path"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    Ok(PrDetail {
        id: v.id,
        url: v.url,
        review_decision: v.review_decision.unwrap_or_default(),
        mergeable: v.mergeable,
        merge_state: v.merge_state_status,
        reviewers,
        assignees: v.assignees.into_iter().map(|a| a.login).collect(),
        labels: v.labels,
        milestone: v.milestone.map(|m| m.title).unwrap_or_default(),
        checks: parse_checks(&v.status_check_rollup),
        viewer: extra["data"]["viewer"]["login"].as_str().unwrap_or("").to_string(),
        reactions: parse_reactions(&pr["reactionGroups"]),
        viewed,
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

/// `reactionGroups` of GraphQL: those given at least once.
fn parse_reactions(groups: &Value) -> Vec<Reaction> {
    groups
        .as_array()
        .map(|a| {
            a.iter()
                .map(|g| Reaction {
                    content: g["content"].as_str().unwrap_or("").to_string(),
                    count: g["reactors"]["totalCount"].as_u64().unwrap_or(0),
                    mine: g["viewerHasReacted"].as_bool().unwrap_or(false),
                })
                .filter(|r| r.count > 0)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_threads(v: &Value) -> Vec<Thread> {
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
                            mine: c["viewerDidAuthor"].as_bool().unwrap_or(false),
                            reactions: parse_reactions(&c["reactionGroups"]),
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
        &["pr", "list", "--head", branch, "--state", "all", "--limit", "10", "--json", SUMMARY],
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

/// Any `gh` command (merge, close, edit… built by the view).
pub fn run(dir: &Path, args: &[String]) -> Result<String, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    gh(dir, &args).map(|o| o.trim().to_string())
}

/// Commands one after the other, until one fails.
pub fn run_all(dir: &Path, cmds: &[Vec<String>]) -> Result<String, String> {
    let mut out = Vec::new();
    for c in cmds {
        out.push(run(dir, c)?);
    }
    Ok(out.join("\n").trim().to_string())
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// `gh pr merge`: "merge", "squash" or "rebase", and the branch deleted after or not.
pub fn merge_args(n: u64, method: &str, delete_branch: bool) -> Vec<String> {
    let mut a = args(&["pr", "merge", &n.to_string(), &format!("--{method}")]);
    if delete_branch {
        a.push("--delete-branch".into());
    }
    a
}

/// `gh pr <verb> n` and its options: close, reopen, ready, update-branch…
pub fn pr_args(verb: &str, n: u64, more: &[&str]) -> Vec<String> {
    let mut a = args(&["pr", verb, &n.to_string()]);
    a.extend(more.iter().map(|s| s.to_string()));
    a
}

/// A review: "approve", "request-changes" or "comment", with its message (needed but to approve).
pub fn review_args(n: u64, event: &str, body: &str) -> Vec<String> {
    let mut a = pr_args("review", n, &[&format!("--{event}")]);
    if !body.is_empty() {
        a.extend(["--body".into(), body.into()]);
    }
    a
}

fn graphql(dir: &Path, query: &str, vars: &[(&str, &str)]) -> Result<String, String> {
    let mut a = args(&["api", "graphql", "-f", &format!("query={query}")]);
    for (k, v) in vars {
        a.extend(["-f".into(), format!("{k}={v}")]);
    }
    run(dir, &a).map(|_| String::new())
}

/// My reaction to a comment (or to the pull request) added or taken back.
pub fn react(dir: &Path, subject: &str, content: &str, add: bool) -> Result<String, String> {
    let m = if add { "addReaction" } else { "removeReaction" };
    graphql(dir, &format!("mutation($id:ID!,$c:ReactionContent!){{{m}(input:{{subjectId:$id,content:$c}}){{clientMutationId}}}}"), &[("id", subject), ("c", content)])
}

/// The text of one of my comments ("IC_…"), comments on lines ("PRRC_…") or reviews ("PRR_…") changed.
pub fn edit_comment(dir: &Path, id: &str, body: &str) -> Result<String, String> {
    let (m, field) = if id.starts_with("PRRC_") {
        ("updatePullRequestReviewComment", "pullRequestReviewCommentId")
    } else if id.starts_with("PRR_") {
        ("updatePullRequestReview", "pullRequestReviewId")
    } else {
        ("updateIssueComment", "id")
    };
    graphql(dir, &format!("mutation($id:ID!,$b:String!){{{m}(input:{{{field}:$id,body:$b}}){{clientMutationId}}}}"), &[("id", id), ("b", body)])
}

/// The command deleting one of my comments (on the pull request or on a line), asked first.
pub fn delete_comment_args(id: &str) -> Vec<String> {
    let m = if id.starts_with("PRRC_") { "deletePullRequestReviewComment" } else { "deleteIssueComment" };
    args(&["api", "graphql", "-f", &format!("query=mutation($id:ID!){{{m}(input:{{id:$id}}){{clientMutationId}}}}"), "-f", &format!("id={id}")])
}

/// A file of the pull request marked as viewed (or not any more), like the box of GitHub.
pub fn set_viewed(dir: &Path, pr_id: &str, path: &str, viewed: bool) -> Result<String, String> {
    let m = if viewed { "markFileAsViewed" } else { "unmarkFileAsViewed" };
    graphql(dir, &format!("mutation($id:ID!,$p:String!){{{m}(input:{{pullRequestId:$id,path:$p}}){{clientMutationId}}}}"), &[("id", pr_id), ("p", path)])
}

/// Labels of the repository and titles of its open milestones, to choose from.
pub fn choices(dir: &Path) -> Result<(Vec<Label>, Vec<String>), String> {
    let labels = serde_json::from_str(&gh(dir, &["label", "list", "--limit", "300", "--json", "name,color"])?).map_err(|e| e.to_string())?;
    let milestones = gh(dir, &["api", "repos/{owner}/{repo}/milestones", "--jq", ".[].title"]).unwrap_or_default();
    Ok((labels, milestones.lines().map(String::from).filter(|l| !l.is_empty()).collect()))
}

/// The log of a GitHub Actions job: its failed steps only, or all when none failed.
pub fn job_log(dir: &Path, job: u64) -> Result<String, String> {
    let j = job.to_string();
    match gh(dir, &["run", "view", "--job", &j, "--log-failed"]) {
        Ok(out) if !out.trim().is_empty() => Ok(out),
        _ => gh(dir, &["run", "view", "--job", &j, "--log"]),
    }
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
        let t = parse_threads(&serde_json::from_str(out).unwrap());
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].line, t[0].start_line, t[0].comments[0].author.as_str()), (Some(12), Some(10), "bob"));
    }

    #[test]
    fn reactions() {
        let g: Value = serde_json::from_str(r#"[{"content":"THUMBS_UP","viewerHasReacted":true,"reactors":{"totalCount":2}},{"content":"EYES","viewerHasReacted":false,"reactors":{"totalCount":0}}]"#).unwrap();
        let mut r = parse_reactions(&g);
        assert_eq!(r, vec![Reaction { content: "THUMBS_UP".into(), count: 2, mine: true }]);
        toggle_reaction(&mut r, "THUMBS_UP", true);
        assert_eq!(r[0].count, 2);
        toggle_reaction(&mut r, "HEART", true);
        toggle_reaction(&mut r, "THUMBS_UP", false);
        assert_eq!(r.iter().map(|r| (r.content.as_str(), r.count, r.mine)).collect::<Vec<_>>(), [("THUMBS_UP", 1, false), ("HEART", 1, true)]);
        toggle_reaction(&mut r, "HEART", false);
        assert_eq!(r.len(), 1);
        assert_eq!(emoji("ROCKET"), "🚀");
    }

    #[test]
    fn checks() {
        let rollup: Vec<Value> = serde_json::from_str(
            r#"[{"__typename":"CheckRun","name":"lint","workflowName":"CI","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://github.com/o/r/actions/runs/11/job/22"},
                {"__typename":"CheckRun","name":"tests","workflowName":"CI","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://github.com/o/r/actions/runs/11/job/33?pr=1"},
                {"__typename":"CheckRun","name":"build","status":"IN_PROGRESS","conclusion":""},
                {"__typename":"StatusContext","context":"ci/runbot","state":"SUCCESS","targetUrl":"https://runbot.odoo.com/x","description":"ok"}]"#,
        )
        .unwrap();
        let c = parse_checks(&rollup);
        assert_eq!(c.iter().map(|c| (c.name.as_str(), c.state)).collect::<Vec<_>>(), [("tests", CheckState::Fail), ("build", CheckState::Pending), ("ci/runbot", CheckState::Pass), ("lint", CheckState::Pass)]);
        assert_eq!(c[0].job(), Some((11, 33)));
        assert_eq!(c[2].job(), None);
        assert_eq!(c[2].description, "ok");
    }

    #[test]
    fn command_args() {
        assert_eq!(merge_args(5, "squash", true), ["pr", "merge", "5", "--squash", "--delete-branch"]);
        assert_eq!(review_args(5, "approve", ""), ["pr", "review", "5", "--approve"]);
        assert_eq!(review_args(5, "request-changes", "non"), ["pr", "review", "5", "--request-changes", "--body", "non"]);
        assert_eq!(pr_args("ready", 5, &["--undo"]), ["pr", "ready", "5", "--undo"]);
    }
}

#[cfg(test)]
mod emoji_tests {
    use super::*;

    /// Every emoji of the reactions must be drawn by a font egui ships (else a box). Checked
    /// against the font files, by their character map.
    #[test]
    fn reactions_are_in_egui_fonts() {
        let dir = std::fs::read_dir(std::env::var("CARGO_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cargo")
        }).join("registry/src"));
        let Ok(dir) = dir else { return };
        let font = dir
            .flatten()
            .flat_map(|e| std::fs::read_dir(e.path()).into_iter().flatten().flatten())
            .find_map(|e| {
                let p = e.path().join("epaint_default_fonts-0.29.1/fonts/NotoEmoji-Regular.ttf");
                p.is_file().then_some(p)
            });
        let Some(font) = font else { return };
        let bytes = std::fs::read(font).unwrap();
        let face = ttf_parser_cmap(&bytes);
        let mut all: Vec<&str> = REACTIONS.iter().map(|(_, e)| *e).collect();
        all.push("☺");
        for e in all {
            let c = e.chars().next().unwrap() as u32;
            assert!(face.contains(&c), "{e} (U+{c:04X}) is not in the emoji font of egui");
        }
    }

    /// Code points of the format 4 subtables of a TrueType cmap (enough for the emoji font).
    fn ttf_parser_cmap(d: &[u8]) -> std::collections::HashSet<u32> {
        let u16at = |i: usize| u16::from_be_bytes([d[i], d[i + 1]]) as usize;
        let u32at = |i: usize| u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
        let mut out = std::collections::HashSet::new();
        let n = u16at(4);
        let mut cmap = 0;
        for i in 0..n {
            let rec = 12 + 16 * i;
            if &d[rec..rec + 4] == b"cmap" {
                cmap = u32at(rec + 8);
            }
        }
        let nt = u16at(cmap + 2);
        for i in 0..nt {
            let s = cmap + u32at(cmap + 4 + 8 * i + 4);
            if u16at(s) == 4 {
                let segx2 = u16at(s + 6);
                for k in 0..segx2 / 2 {
                    let end = u16at(s + 14 + 2 * k);
                    let start = u16at(s + 16 + segx2 + 2 * k);
                    if start <= end && end < 0xFFFF {
                        out.extend(start as u32..=end as u32);
                    }
                }
            }
        }
        out
    }
}
