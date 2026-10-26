//! The scanner. Walks a directory once and builds an arena tree: every file and
//! folder is a `Node` in one flat `Vec`, and nodes point at each other by index.
//! That keeps millions of entries cheap (no `Rc`, no per-node allocation of paths).

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: OsString,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: Kind,
    /// Size as reported by `ls` (file length), summed over the subtree.
    pub apparent: u64,
    /// Space actually used on disk (allocated blocks), summed over the subtree.
    pub disk: u64,
    /// Number of entries in the subtree, including this one.
    pub items: u64,
    /// The directory couldn't be read (usually permissions).
    pub error: bool,
}

impl Node {
    pub fn size(&self, apparent: bool) -> u64 {
        if apparent {
            self.apparent
        } else {
            self.disk
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    pub fn is_hidden(&self) -> bool {
        self.name.as_bytes().first() == Some(&b'.')
    }
}

#[derive(Debug)]
pub struct Tree {
    /// Absolute path of the scanned directory (node 0).
    pub root: PathBuf,
    pub nodes: Vec<Node>,
    /// Number of entries that couldn't be read.
    pub errors: u64,
}

impl Tree {
    /// Rebuild the full path of a node by walking up to the root.
    pub fn path(&self, idx: usize) -> PathBuf {
        let mut parts = Vec::new();
        let mut cur = idx;
        while let Some(parent) = self.nodes[cur].parent {
            parts.push(&self.nodes[cur].name);
            cur = parent;
        }
        let mut path = self.root.clone();
        for part in parts.into_iter().rev() {
            path.push(part);
        }
        path
    }

    pub fn name(&self, idx: usize) -> String {
        if idx == 0 {
            self.root.display().to_string()
        } else {
            self.nodes[idx].name.to_string_lossy().into_owned()
        }
    }

    /// Detach a node after it was deleted and subtract its sizes from every ancestor.
    pub fn remove(&mut self, idx: usize) {
        if idx == 0 {
            return;
        }
        let (apparent, disk, items) = {
            let n = &self.nodes[idx];
            (n.apparent, n.disk, n.items)
        };
        let parent = self.nodes[idx].parent;
        if let Some(p) = parent {
            self.nodes[p].children.retain(|&c| c != idx);
        }
        let mut cur = parent;
        while let Some(p) = cur {
            let n = &mut self.nodes[p];
            n.apparent = n.apparent.saturating_sub(apparent);
            n.disk = n.disk.saturating_sub(disk);
            n.items = n.items.saturating_sub(items);
            cur = n.parent;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Don't descend into directories on other filesystems (like `du -x`).
    pub one_filesystem: bool,
    /// Skip entries whose file name matches exactly.
    pub exclude: Vec<String>,
}

/// Live counters the UI reads while a scan runs on another thread.
#[derive(Debug, Default)]
pub struct Progress {
    pub items: AtomicU64,
    pub bytes: AtomicU64,
    pub current: Mutex<PathBuf>,
}

pub fn scan(root: &Path, opts: &Options, progress: &Progress) -> io::Result<Tree> {
    let root = fs::canonicalize(root)?;
    let meta = fs::metadata(&root)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a directory", root.display()),
        ));
    }
    let root_dev = meta.dev();

    let mut nodes = vec![Node {
        name: OsString::new(),
        parent: None,
        children: Vec::new(),
        kind: Kind::Dir,
        apparent: meta.len(),
        disk: meta.blocks() * 512,
        items: 1,
        error: false,
    }];
    // (device, inode) of files with several hard links we've already counted.
    let mut seen_links: HashSet<(u64, u64)> = HashSet::new();
    let mut errors = 0u64;

    // Iterative depth-first walk: no recursion, so deep trees can't overflow the stack.
    let mut stack = vec![(0usize, root.clone())];
    while let Some((idx, dir)) = stack.pop() {
        if let Ok(mut current) = progress.current.lock() {
            current.clone_from(&dir);
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                nodes[idx].error = true;
                errors += 1;
                continue;
            }
        };

        for entry in entries {
            let Ok(entry) = entry else {
                nodes[idx].error = true;
                errors += 1;
                continue;
            };
            let name = entry.file_name();
            if opts.exclude.iter().any(|x| name.as_os_str() == x.as_str()) {
                continue;
            }
            let path = entry.path();

            // symlink_metadata: never follow symlinks.
            let (kind, mut apparent, mut disk, dev, ino, nlink) = match fs::symlink_metadata(&path) {
                Ok(m) => {
                    let ft = m.file_type();
                    let kind = if ft.is_dir() {
                        Kind::Dir
                    } else if ft.is_symlink() {
                        Kind::Symlink
                    } else if ft.is_file() {
                        Kind::File
                    } else {
                        Kind::Other
                    };
                    (kind, m.len(), m.blocks() * 512, m.dev(), m.ino(), m.nlink())
                }
                Err(_) => {
                    errors += 1;
                    let child = nodes.len();
                    nodes.push(Node {
                        name,
                        parent: Some(idx),
                        children: Vec::new(),
                        kind: Kind::Other,
                        apparent: 0,
                        disk: 0,
                        items: 1,
                        error: true,
                    });
                    nodes[idx].children.push(child);
                    continue;
                }
            };

            // Hard links: the same data shows up under several names; count it once.
            if kind != Kind::Dir && nlink > 1 && !seen_links.insert((dev, ino)) {
                apparent = 0;
                disk = 0;
            }

            let child = nodes.len();
            nodes.push(Node {
                name,
                parent: Some(idx),
                children: Vec::new(),
                kind,
                apparent,
                disk,
                items: 1,
                error: false,
            });
            nodes[idx].children.push(child);
            progress.items.fetch_add(1, Ordering::Relaxed);
            progress.bytes.fetch_add(disk, Ordering::Relaxed);

            let other_fs = opts.one_filesystem && dev != root_dev;
            if kind == Kind::Dir && !other_fs {
                stack.push((child, path));
            }
        }
    }

    // Children always have a higher index than their parent, so walking backwards
    // adds every subtree into its parent after the subtree itself is complete.
    for i in (1..nodes.len()).rev() {
        let parent = nodes[i].parent.expect("non-root node has a parent");
        let (apparent, disk, items) = (nodes[i].apparent, nodes[i].disk, nodes[i].items);
        let p = &mut nodes[parent];
        p.apparent += apparent;
        p.disk += disk;
        p.items += items;
    }

    Ok(Tree { root, nodes, errors })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, len: usize) {
        fs::write(path, vec![b'x'; len]).unwrap();
    }

    fn own_len(path: &Path) -> u64 {
        fs::symlink_metadata(path).unwrap().len()
    }

    fn find(tree: &Tree, name: &str) -> usize {
        tree.nodes.iter().position(|n| n.name == name).unwrap()
    }

    fn scan_dir(path: &Path) -> Tree {
        scan(path, &Options::default(), &Progress::default()).unwrap()
    }

    #[test]
    fn sums_sizes_and_items() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("a"), 1000);
        fs::create_dir(t.path().join("sub")).unwrap();
        write(&t.path().join("sub/b"), 3000);

        let tree = scan_dir(t.path());
        let expected = own_len(t.path()) + own_len(&t.path().join("sub")) + 4000;
        assert_eq!(tree.nodes[0].apparent, expected);
        assert_eq!(tree.nodes[0].items, 4); // root, a, sub, b
        let sub = find(&tree, "sub");
        assert_eq!(tree.nodes[sub].apparent, own_len(&t.path().join("sub")) + 3000);
        assert!(tree.nodes[0].disk > 0);
    }

    #[test]
    fn hard_links_count_once() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("a"), 5000);
        fs::hard_link(t.path().join("a"), t.path().join("b")).unwrap();

        let tree = scan_dir(t.path());
        let files: u64 = tree.nodes.iter().filter(|n| n.kind == Kind::File).map(|n| n.apparent).sum();
        assert_eq!(files, 5000);
    }

    #[test]
    fn symlinks_are_not_followed() {
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("big"), 100_000);
        let t = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), t.path().join("link")).unwrap();

        let tree = scan_dir(t.path());
        let link = find(&tree, "link");
        assert_eq!(tree.nodes[link].kind, Kind::Symlink);
        assert!(tree.nodes[0].apparent < 100_000);
        assert_eq!(tree.nodes[0].items, 2);
    }

    #[test]
    fn excludes_by_name() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("node_modules")).unwrap();
        write(&t.path().join("node_modules/x"), 9000);
        write(&t.path().join("keep"), 10);

        let opts = Options { exclude: vec!["node_modules".into()], ..Default::default() };
        let tree = scan(t.path(), &opts, &Progress::default()).unwrap();
        assert_eq!(tree.nodes[0].items, 2);
    }

    #[test]
    fn paths_and_remove() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("sub")).unwrap();
        write(&t.path().join("sub/b"), 3000);
        write(&t.path().join("c"), 10);

        let mut tree = scan_dir(t.path());
        let b = find(&tree, "b");
        assert_eq!(tree.path(b), fs::canonicalize(t.path()).unwrap().join("sub/b"));

        let before = tree.nodes[0].apparent;
        let sub = find(&tree, "sub");
        let sub_size = tree.nodes[sub].apparent;
        tree.remove(sub);
        assert_eq!(tree.nodes[0].apparent, before - sub_size);
        assert_eq!(tree.nodes[0].items, 2); // root, c
        assert!(!tree.nodes[0].children.contains(&sub));
    }

    #[test]
    fn rejects_files_as_root() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("f"), 1);
        assert!(scan(&t.path().join("f"), &Options::default(), &Progress::default()).is_err());
    }
}
