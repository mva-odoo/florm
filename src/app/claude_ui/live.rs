//! The output of a command of Claude while it runs, like Claude Desktop shows it. Claude Code
//! writes what each Bash command prints to `<tmp>/claude-<uid>/<project>/<session>/tasks/<id>.output`
//! (in the foreground too); the result of a command sent to the background names that file.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// What is read of an output at most: its end.
const TAIL: u64 = 64 * 1024;

#[derive(Default)]
pub(in crate::app) struct Live {
    /// The tools whose output is shown (their id).
    pub(in crate::app) open: HashSet<String>,
    /// The file of each tool, once found.
    files: HashMap<String, PathBuf>,
    /// The end of each file, when it was read and the size it had then.
    read: HashMap<PathBuf, (Instant, u64, String)>,
}

/// The file a command sent to the background writes to, from its result ("… Output is being
/// written to: /tmp/…/tasks/b1x.output").
pub(in crate::app) fn background_file(result: &str) -> Option<PathBuf> {
    let (_, rest) = result.split_once("Output is being written to:")?;
    let path = rest.split_whitespace().next()?.trim_end_matches(['.', ',']);
    let path = if path.ends_with(".output") { path.to_string() } else { format!("{path}.output") };
    Some(PathBuf::from(path)).filter(|p| p.is_absolute())
}

/// The folders where Claude Code puts the outputs of the commands of session `session`.
fn tasks_dirs(session: &str) -> Vec<PathBuf> {
    let tmp = std::env::var_os("CLAUDE_CODE_TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let mut dirs = Vec::new();
    for user in std::fs::read_dir(&tmp).into_iter().flatten().flatten() {
        if !user.file_name().to_string_lossy().starts_with("claude") {
            continue;
        }
        for project in std::fs::read_dir(user.path()).into_iter().flatten().flatten() {
            let d = project.path().join(session).join("tasks");
            if d.is_dir() {
                dirs.push(d);
            }
        }
    }
    dirs
}

fn ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Of the outputs `(path, created in unix ms)`, the one of a command started at `start` (unix
/// ms): the first one made since, that no other command has.
fn pick(files: &[(PathBuf, u64)], start: u64, taken: &HashSet<&PathBuf>) -> Option<PathBuf> {
    files.iter().filter(|(p, t)| *t + 1500 >= start && !taken.contains(p)).min_by_key(|(_, t)| *t).map(|(p, _)| p.clone())
}

/// The end of `text`: its last `lines` lines.
fn last_lines(text: &str, lines: usize) -> &str {
    let mut cut = text.len();
    for _ in 0..lines {
        match text[..cut].trim_end_matches('\n').rfind('\n') {
            Some(i) => cut = i,
            None => return text,
        }
    }
    &text[cut + 1..]
}

impl Live {
    /// The output file of tool `id` (a Bash command started at `start`, unix ms): named by its
    /// result if it went to the background, else looked for among the outputs of the session.
    pub(in crate::app) fn file(&mut self, id: &str, start: u64, result: Option<&str>, session: Option<&str>) -> Option<PathBuf> {
        if let Some(p) = self.files.get(id) {
            return Some(p.clone());
        }
        let found = match result.and_then(background_file) {
            Some(p) => Some(p),
            None => {
                let session = session?;
                let files: Vec<(PathBuf, u64)> = tasks_dirs(session)
                    .iter()
                    .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
                    .filter(|e| e.path().extension().is_some_and(|x| x == "output"))
                    .filter_map(|e| {
                        let m = e.metadata().ok()?;
                        Some((e.path(), ms(m.created().or_else(|_| m.modified()).ok()?)))
                    })
                    .collect();
                let taken: HashSet<&PathBuf> = self.files.values().collect();
                pick(&files, start, &taken)
            }
        }?;
        self.files.insert(id.to_string(), found.clone());
        Some(found)
    }

    /// The end of `path` (read again twice a second at most), and how long ago it last grew.
    pub(in crate::app) fn text(&mut self, path: &Path) -> (String, Option<u64>) {
        use std::io::{Read, Seek, SeekFrom};
        let meta = std::fs::metadata(path).ok();
        let size = meta.as_ref().map_or(0, |m| m.len());
        let quiet = meta.and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
        let fresh = self.read.get(path).is_some_and(|(at, s, _)| *s == size || at.elapsed().as_millis() < 500);
        if !fresh {
            let mut text = String::new();
            if let Ok(mut f) = std::fs::File::open(path) {
                let _ = f.seek(SeekFrom::Start(size.saturating_sub(TAIL)));
                let mut bytes = Vec::new();
                let _ = f.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).into_owned();
                // the first line read may be cut
                if size > TAIL {
                    text = text.split_once('\n').map_or(text.clone(), |(_, r)| r.to_string());
                }
            }
            self.read.insert(path.to_path_buf(), (Instant::now(), size, last_lines(&text, 400).to_string()));
        }
        (self.read.get(path).map(|(_, _, t)| t.clone()).unwrap_or_default(), quiet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_result_names_its_file() {
        let r = "Command running in background with ID: b9yh5nk0r. Output is being written to: /tmp/claude-1000/-home-x/abc/tasks/b9yh5nk0r.output";
        assert_eq!(background_file(r), Some(PathBuf::from("/tmp/claude-1000/-home-x/abc/tasks/b9yh5nk0r.output")));
        assert_eq!(background_file("ok"), None);
    }

    #[test]
    fn a_command_gets_the_first_output_made_since_it_started() {
        let (a, b, c) = (PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c"));
        let files = vec![(a.clone(), 1_000), (b.clone(), 10_000), (c.clone(), 12_000)];
        assert_eq!(pick(&files, 9_500, &HashSet::new()), Some(b.clone()));
        // two commands at once: the second one gets the next file
        assert_eq!(pick(&files, 9_500, &HashSet::from([&b])), Some(c));
        assert_eq!(pick(&files, 20_000, &HashSet::new()), None);
    }

    #[test]
    fn the_end_of_an_output() {
        assert_eq!(last_lines("1\n2\n3\n4\n", 2), "3\n4\n");
        assert_eq!(last_lines("1\n2", 5), "1\n2");
    }
}
