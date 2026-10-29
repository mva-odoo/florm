//! The files of a project for go to file: their paths relative to the project, sorted, all in one
//! text. A big folder holds close to a million files: one allocation per path (and one more per
//! entry of the list shown) would cost several times the size of the paths themselves.

use std::path::Path;

#[derive(Default, Clone, PartialEq, Eq, Debug)]
pub struct FileList {
    text: String,
    /// Where each path starts in `text` (it ends where the next one starts).
    starts: Vec<u32>,
}

impl FileList {
    /// The list of `paths` (relative to the project), sorted here.
    pub fn new(mut paths: Vec<String>) -> FileList {
        paths.sort_unstable();
        let mut list = FileList { text: String::with_capacity(paths.iter().map(String::len).sum()), starts: Vec::with_capacity(paths.len()) };
        for p in &paths {
            list.starts.push(list.text.len() as u32);
            list.text.push_str(p);
        }
        list
    }

    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// Path `i`, relative to the project.
    pub fn get(&self, i: usize) -> Option<&str> {
        let start = *self.starts.get(i)? as usize;
        let end = self.starts.get(i + 1).map_or(self.text.len(), |&e| e as usize);
        Some(&self.text[start..end])
    }

    /// Where the relative path `p` is in the list.
    pub fn position(&self, p: &Path) -> Option<usize> {
        let p = p.to_str()?;
        let mut k = 0..self.len();
        // binary search: the list is sorted
        while k.start < k.end {
            let mid = (k.start + k.end) / 2;
            match self.get(mid)?.cmp(p) {
                std::cmp::Ordering::Equal => return Some(mid),
                std::cmp::Ordering::Less => k.start = mid + 1,
                std::cmp::Ordering::Greater => k.end = mid,
            }
        }
        None
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        (0..self.len()).filter_map(|i| self.get(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_and_found() {
        let l = FileList::new(["b/x.py", "a.py", "b/a.py"].map(String::from).to_vec());
        assert_eq!(l.iter().collect::<Vec<_>>(), ["a.py", "b/a.py", "b/x.py"]);
        assert_eq!((l.get(1), l.get(3)), (Some("b/a.py"), None));
        assert_eq!(l.position(Path::new("b/x.py")), Some(2));
        assert_eq!(l.position(Path::new("a.py")), Some(0));
        assert_eq!(l.position(Path::new("c.py")), None);
        assert_eq!(l, FileList::new(["a.py", "b/x.py", "b/a.py"].map(String::from).to_vec()));
        assert!(FileList::default().is_empty() && FileList::default().position(Path::new("a")).is_none());
    }
}
