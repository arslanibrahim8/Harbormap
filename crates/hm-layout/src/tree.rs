//! The folder-first group tree (v0.2 spec §3, as scoped by the L5a plan).
//!
//! [`build`] turns the files of an [`Input`] into a tree of boxes:
//! 1. **Hubs:** a file imported by more than [`HUB_IN_DEGREE`] distinct files moves to
//!    one top-level "Shared" box.
//! 2. **Folders:** the other files form their folder tree. A folder whose only child
//!    is one folder merges with it into one box named `a/b`; the root descends
//!    through such a chain, so a project kept under `src/` starts inside `src`.
//! 3. **Budgets:** a box with too many children is split, stopping as soon as it
//!    fits: loose files sharing a first or last name word form "<word> files" boxes,
//!    the remaining loose files form one "files" box, and all but the biggest
//!    `budget − 1` items go into one "N more" box. New word and files boxes are split
//!    again the same way. A "more" box that still holds more than [`GROUP_BUDGET`]
//!    items is cut into at most that many "first … last" boxes: contiguous ranges of
//!    its items by name, of near-equal size, cut again until each fits. So a flat
//!    folder of `n` files is about log₂₅ `n` levels deep.
//!
//! Keys are paths (`src/orders`), `~shared`, or a parent's key plus `*word`,
//! `~files`, `~more` or `~first..last`. Ids, child order and hashes derive from the
//! input only, so the same input always gives the same tree.

use std::collections::{BTreeMap, BTreeSet};

use hm_core::ids::file_id;
use serde::{Deserialize, Serialize};

use crate::input::{InLink, Input};
use crate::roles::{Role, role_of, stem, words};

/// At most this many boxes on the overview, the Shared box included.
pub const OVERVIEW_BUDGET: usize = 12;
/// At most this many children in any group below the overview.
pub const GROUP_BUDGET: usize = 25;
/// A file imported by more than this many distinct files moves to "Shared".
pub const HUB_IN_DEGREE: usize = 20;

/// A word must start or end the names of at least this many loose files to group them.
const TOKEN_MIN_FILES: usize = 3;
/// The key of the Shared box.
const SHARED_KEY: &str = "~shared";

/// What a node of the tree stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// A folder (or a merged chain of folders); also the root.
    Folder,
    /// Loose files of one folder that share a name word.
    Token,
    /// The remaining loose files of one folder.
    Files,
    /// The smallest items of a box that had too many.
    More,
    /// The top-level box of hub files.
    Shared,
    /// One input file; always a leaf.
    File,
}

impl NodeKind {
    /// The lowercase name, as it is serialized.
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Folder => "folder",
            NodeKind::Token => "token",
            NodeKind::Files => "files",
            NodeKind::More => "more",
            NodeKind::Shared => "shared",
            NodeKind::File => "file",
        }
    }
}

/// One box of the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Node {
    /// `g_` + 12 hex of BLAKE3 over the key for groups, `hm_core::ids::file_id` for files.
    pub id: String,
    /// The path for folders and files, `""` for the root; see the module docs.
    pub key: String,
    pub kind: NodeKind,
    /// Never empty: the folder path relative to the enclosing folder, the file name,
    /// `"<word> files"`, `"other files"`/`"files"`, `"<N> more"`, `"Shared"`, or the
    /// project name for the root.
    pub name: String,
    pub role: Role,
    pub parent: Option<usize>,
    /// Ordered by key.
    pub children: Vec<usize>,
    /// Input files below this node (1 for a file).
    pub files: u32,
    /// Lines of code below this node.
    pub loc: u32,
    /// For a file node, its index into [`Input::files`].
    pub file: Option<u32>,
    /// BLAKE3 over the sorted keys of the direct children: changes only when the
    /// node gains or loses children, so it keys the position cache.
    pub shape_hash: [u8; 32],
    /// BLAKE3 over the node's content: its kind and key; for a file its path, loc and
    /// `generated` flag; the children's merkles in key order; and the links (with
    /// their `refs`) whose lowest common node this is, i.e. that run between two of
    /// its direct children. A link is hashed only at that node: the nodes it leaves
    /// on the way up do not hash it, so a link change marks only that node and its
    /// ancestors.
    pub merkle: [u8; 32],
}

/// The group tree of one input. `nodes` is in pre-order with children by key, so
/// two builds of the same input are equal field by field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root: usize,
    /// The node of each input file, by index into [`Input::files`].
    pub file_node: Vec<usize>,
    #[serde(skip)]
    by_id: BTreeMap<String, usize>,
}

impl Tree {
    /// The node with this id.
    pub fn node(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// The overview boxes: the root's children.
    pub fn top(&self) -> &[usize] {
        &self.nodes[self.root].children
    }
}

/// The id of a group with this key.
pub(crate) fn group_id(key: &str) -> String {
    format!("g_{}", &blake3::hash(key.as_bytes()).to_hex()[..12])
}

/// A node while the tree is being built; indexes are into `Builder::drafts`.
struct Draft {
    key: String,
    kind: NodeKind,
    name: String,
    role: Role,
    parent: Option<usize>,
    children: Vec<usize>,
    files: u32,
    loc: u32,
    file: Option<u32>,
}

/// One folder of the input's folder tree.
#[derive(Default)]
struct Folder {
    subs: BTreeSet<String>,
    files: Vec<u32>,
}

/// Groups the files of `input` into the tree described in the module docs.
pub fn build(input: &Input, project_name: &str) -> Tree {
    let n = input.files.len();
    let links = input.clean_links();

    // 1. Hubs: distinct importing files per file (`clean_links` has one entry per pair).
    let mut in_degree = vec![0usize; n];
    for l in &links {
        in_degree[l.dst as usize] += 1;
    }
    let is_hub: Vec<bool> = in_degree.iter().map(|&d| d > HUB_IN_DEGREE).collect();

    // 2. The folder tree of the other files.
    let mut folders: BTreeMap<String, Folder> = BTreeMap::new();
    folders.entry(String::new()).or_default();
    for (i, f) in input.files.iter().enumerate() {
        if is_hub[i] {
            continue;
        }
        let dir = parent_dir(&f.path);
        folders
            .entry(dir.to_string())
            .or_default()
            .files
            .push(i as u32);
        let mut child = dir;
        while !child.is_empty() {
            let up = parent_dir(child);
            folders
                .entry(up.to_string())
                .or_default()
                .subs
                .insert(child.to_string());
            child = up;
        }
    }

    let mut b = Builder {
        input,
        drafts: Vec::new(),
    };
    let root_name = if project_name.trim().is_empty() {
        "project"
    } else {
        project_name
    };
    let root = b.add(
        None,
        String::new(),
        NodeKind::Folder,
        root_name.into(),
        Role::Work,
    );
    let top = chain_end(&folders, "");
    let mut stack = vec![(top.clone(), root)];
    while let Some((path, node)) = stack.pop() {
        let folder = &folders[&path];
        for sub in &folder.subs {
            let end = chain_end(&folders, sub);
            let name = if path.is_empty() {
                end.clone()
            } else {
                end[path.len() + 1..].to_string()
            };
            let role = role_of(&end, false);
            let child = b.add(Some(node), end.clone(), NodeKind::Folder, name, role);
            stack.push((end, child));
        }
        for &f in &folder.files {
            b.add_file(node, f);
        }
    }
    b.sum_up();

    // 3. Budgets: the root first (leaving room for Shared), then every other group.
    let hubs: Vec<u32> = (0..n as u32).filter(|&i| is_hub[i as usize]).collect();
    let root_budget = OVERVIEW_BUDGET - usize::from(!hubs.is_empty());
    b.partition(root, root_budget, &top);
    if !hubs.is_empty() {
        let shared = b.add(
            Some(root),
            SHARED_KEY.into(),
            NodeKind::Shared,
            "Shared".into(),
            Role::Shared,
        );
        for &f in &hubs {
            let leaf = b.add_file(shared, f);
            b.drafts[shared].files += 1;
            b.drafts[shared].loc = b.drafts[shared].loc.saturating_add(b.drafts[leaf].loc);
        }
        b.drafts[root].files += b.drafts[shared].files;
        b.drafts[root].loc = b.drafts[root].loc.saturating_add(b.drafts[shared].loc);
    }
    let mut queue: Vec<usize> = (0..b.drafts.len())
        .filter(|&i| i != root && b.drafts[i].kind != NodeKind::File)
        .collect();
    while let Some(g) = queue.pop() {
        if b.drafts[g].kind == NodeKind::More {
            queue.extend(b.split_more(g));
        } else {
            let prefix = b.drafts[g].key.clone();
            queue.extend(b.partition(g, GROUP_BUDGET, &prefix));
        }
    }

    b.finish(root, &links)
}

/// The deepest folder of the single-folder chain that starts at `path`.
fn chain_end<'a>(folders: &'a BTreeMap<String, Folder>, mut path: &'a str) -> String {
    loop {
        let f = &folders[path];
        match (f.files.is_empty(), f.subs.len()) {
            (true, 1) => path = f.subs.iter().next().unwrap().as_str(),
            _ => return path.to_string(),
        }
    }
}

/// The folder part of a path (`""` for a file at the root).
fn parent_dir(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// `prefix/tail`, or `tail` when the prefix is empty.
fn join(prefix: &str, tail: &str) -> String {
    if prefix.is_empty() {
        tail.to_string()
    } else {
        format!("{prefix}/{tail}")
    }
}

struct Builder<'a> {
    input: &'a Input,
    drafts: Vec<Draft>,
}

impl Builder<'_> {
    /// Adds an empty node under `parent`.
    fn add(
        &mut self,
        parent: Option<usize>,
        key: String,
        kind: NodeKind,
        name: String,
        role: Role,
    ) -> usize {
        let i = self.drafts.len();
        self.drafts.push(Draft {
            key,
            kind,
            name,
            role,
            parent,
            children: Vec::new(),
            files: 0,
            loc: 0,
            file: None,
        });
        if let Some(p) = parent {
            self.drafts[p].children.push(i);
        }
        i
    }

    /// Adds the node of input file `f` under `parent`.
    fn add_file(&mut self, parent: usize, f: u32) -> usize {
        let file = &self.input.files[f as usize];
        let name = file.path.rsplit('/').next().unwrap_or("");
        let name = if name.is_empty() { &file.path } else { name };
        let role = role_of(&file.path, true);
        let i = self.add(
            Some(parent),
            file.path.clone(),
            NodeKind::File,
            name.to_string(),
            role,
        );
        let d = &mut self.drafts[i];
        (d.files, d.loc, d.file) = (1, file.loc, Some(f));
        i
    }

    /// Adds up `files` and `loc` from the leaves. Parents come before their children
    /// in `drafts`, so one backward pass does it.
    fn sum_up(&mut self) {
        for i in (0..self.drafts.len()).rev() {
            if let Some(p) = self.drafts[i].parent {
                let (files, loc) = (self.drafts[i].files, self.drafts[i].loc);
                self.drafts[p].files += files;
                self.drafts[p].loc = self.drafts[p].loc.saturating_add(loc);
            }
        }
    }

    /// Adds a group under `parent` holding `items` (current children of `parent`).
    fn regroup(
        &mut self,
        parent: usize,
        items: &[usize],
        key: String,
        kind: NodeKind,
        name: String,
    ) -> usize {
        let role = self.drafts[parent].role;
        let g = self.add(None, key, kind, name, role);
        self.drafts[g].parent = Some(parent);
        for &item in items {
            self.drafts[item].parent = Some(g);
            self.drafts[g].files += self.drafts[item].files;
            self.drafts[g].loc = self.drafts[g].loc.saturating_add(self.drafts[item].loc);
        }
        self.drafts[g].children = items.to_vec();
        g
    }

    /// Splits the children of `node` until there are at most `budget`; new group keys
    /// start with `prefix`. Returns the new groups, which still have to be split.
    fn partition(&mut self, node: usize, budget: usize, prefix: &str) -> Vec<usize> {
        let items = std::mem::take(&mut self.drafts[node].children);
        if items.len() <= budget {
            self.drafts[node].children = items;
            return Vec::new();
        }
        let (loose, others): (Vec<usize>, Vec<usize>) = items
            .iter()
            .copied()
            .partition(|&i| self.drafts[i].kind == NodeKind::File);
        let mut count = items.len();
        let mut kept: Vec<usize> = others.clone();
        let mut created = Vec::new();

        // Step 1: token groups. Each loose file offers its first and last stem word.
        let offers: Vec<Vec<(String, String)>> = loose
            .iter()
            .map(|&i| {
                let name = &self.drafts[i].name;
                let ws = words(stem(name));
                let mut out: Vec<(String, String)> = Vec::new();
                for w in ws.first().into_iter().chain(ws.last()) {
                    let lower = w.to_lowercase();
                    if !out.iter().any(|(l, _)| *l == lower) {
                        out.push((lower, w.to_string()));
                    }
                }
                out
            })
            .collect();
        let mut members: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (k, words) in offers.iter().enumerate() {
            for (lower, _) in words {
                members.entry(lower).or_default().push(k);
            }
        }
        let mut open: BTreeMap<&str, usize> = members.iter().map(|(w, m)| (*w, m.len())).collect();
        let mut claimed = vec![false; loose.len()];
        while count > budget {
            let best = open
                .iter()
                .filter(|(_, c)| **c >= TOKEN_MIN_FILES)
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(w, _)| *w);
            let Some(word) = best else { break };
            open.remove(word);
            let group: Vec<usize> = members[&word]
                .iter()
                .copied()
                .filter(|&k| !claimed[k])
                .collect();
            if group.len() == count {
                // A group of every item would change nothing.
                continue;
            }
            let mut spellings: BTreeMap<&str, usize> = BTreeMap::new();
            for &k in &group {
                claimed[k] = true;
                for (lower, spelled) in &offers[k] {
                    if lower.as_str() == word {
                        *spellings.entry(spelled.as_str()).or_default() += 1;
                    } else if let Some(c) = open.get_mut(lower.as_str()) {
                        *c -= 1;
                    }
                }
            }
            let spelled = spellings
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map_or(word, |(s, _)| *s)
                .to_string();
            let files: Vec<usize> = group.iter().map(|&k| loose[k]).collect();
            let key = join(prefix, &format!("*{word}"));
            let g = self.regroup(
                node,
                &files,
                key,
                NodeKind::Token,
                format!("{spelled} files"),
            );
            count = count - files.len() + 1;
            kept.push(g);
            created.push(g);
        }
        let rest: Vec<usize> = (0..loose.len())
            .filter(|&k| !claimed[k])
            .map(|k| loose[k])
            .collect();

        // Step 2: one group of the remaining loose files.
        if count > budget && rest.len() >= 2 && rest.len() < count {
            let has_folders = others
                .iter()
                .any(|&i| self.drafts[i].kind == NodeKind::Folder);
            let name = if has_folders { "other files" } else { "files" };
            let key = join(prefix, "~files");
            let g = self.regroup(node, &rest, key, NodeKind::Files, name.into());
            count = count - rest.len() + 1;
            kept.push(g);
            created.push(g);
        } else {
            kept.extend(rest);
        }
        debug_assert_eq!(kept.len(), count);

        // Step 3: all but the biggest `budget - 1` items go into one "more" box, which
        // `split_more` cuts into name ranges if it is still too big.
        if count > budget {
            kept.sort_by(|&a, &b| {
                let (a, b) = (&self.drafts[a], &self.drafts[b]);
                (b.files.cmp(&a.files))
                    .then_with(|| a.name.cmp(&b.name))
                    .then_with(|| a.key.cmp(&b.key))
            });
            let tail = kept.split_off(budget - 1);
            let key = join(prefix, "~more");
            let name = format!("{} more", tail.len());
            let g = self.regroup(node, &tail, key, NodeKind::More, name);
            kept.push(g);
            created.push(g);
        }
        self.drafts[node].children = kept;
        created
    }

    /// Cuts a "more" box holding more than [`GROUP_BUDGET`] items into at most that
    /// many "more" boxes of contiguous name ranges, of near-equal size. Returns them;
    /// the ones still too big are cut again.
    fn split_more(&mut self, node: usize) -> Vec<usize> {
        let mut items = std::mem::take(&mut self.drafts[node].children);
        if items.len() <= GROUP_BUDGET {
            self.drafts[node].children = items;
            return Vec::new();
        }
        items.sort_by(|&a, &b| {
            let (a, b) = (&self.drafts[a], &self.drafts[b]);
            a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key))
        });
        let buckets = items.len().div_ceil(GROUP_BUDGET).min(GROUP_BUDGET);
        let (size, extra) = (items.len() / buckets, items.len() % buckets);
        let prefix = self.drafts[node].key.clone();
        let mut keys = BTreeSet::new();
        let mut created = Vec::with_capacity(buckets);
        let mut start = 0;
        for b in 0..buckets {
            let part = &items[start..start + size + usize::from(b < extra)];
            start += part.len();
            let first = self.drafts[part[0]].name.clone();
            let last = self.drafts[part[part.len() - 1]].name.clone();
            let name = if first == last {
                first.clone()
            } else {
                format!("{first} … {last}")
            };
            // Names may repeat (hub files from different folders); keys may not.
            let mut key = join(&prefix, &format!("~{first}..{last}"));
            let mut k = 2;
            while !keys.insert(key.clone()) {
                key = join(&prefix, &format!("~{first}..{last}#{k}"));
                k += 1;
            }
            created.push(self.regroup(node, part, key, NodeKind::More, name));
        }
        self.drafts[node].children = created.clone();
        created
    }

    /// Orders children by key, renumbers the nodes in pre-order and computes ids and
    /// hashes.
    fn finish(mut self, root: usize, links: &[InLink]) -> Tree {
        let drafts = &mut self.drafts;
        for i in 0..drafts.len() {
            let mut children = std::mem::take(&mut drafts[i].children);
            children.sort_by(|&a, &b| drafts[a].key.cmp(&drafts[b].key));
            drafts[i].children = children;
        }
        let mut order = Vec::with_capacity(drafts.len());
        let mut new_index = vec![usize::MAX; drafts.len()];
        let mut stack = vec![root];
        while let Some(d) = stack.pop() {
            new_index[d] = order.len();
            order.push(d);
            stack.extend(drafts[d].children.iter().rev());
        }

        let mut nodes: Vec<Node> = order
            .iter()
            .map(|&d| {
                let d = &mut drafts[d];
                let id = match d.kind {
                    NodeKind::File => file_id(&d.key),
                    _ => group_id(&d.key),
                };
                Node {
                    id,
                    key: std::mem::take(&mut d.key),
                    kind: d.kind,
                    name: std::mem::take(&mut d.name),
                    role: d.role,
                    parent: d.parent.map(|p| new_index[p]),
                    children: d.children.iter().map(|&c| new_index[c]).collect(),
                    files: d.files,
                    loc: d.loc,
                    file: d.file,
                    shape_hash: [0; 32],
                    merkle: [0; 32],
                }
            })
            .collect();
        let mut file_node = vec![usize::MAX; self.input.files.len()];
        for (i, n) in nodes.iter().enumerate() {
            if let Some(f) = n.file {
                file_node[f as usize] = i;
            }
        }

        // Each link belongs to the lowest node holding both ends, where it runs
        // between two different direct children.
        let mut depth = vec![0usize; nodes.len()];
        for i in 1..nodes.len() {
            depth[i] = depth[nodes[i].parent.unwrap_or(0)] + 1;
        }
        let up = |i: usize| nodes[i].parent.unwrap_or(i);
        let mut inner: Vec<Vec<InLink>> = vec![Vec::new(); nodes.len()];
        for &link in links {
            let (mut a, mut b) = (file_node[link.src as usize], file_node[link.dst as usize]);
            while depth[a] > depth[b] {
                a = up(a);
            }
            while depth[b] > depth[a] {
                b = up(b);
            }
            while up(a) != up(b) {
                (a, b) = (up(a), up(b));
            }
            inner[up(a)].push(link);
        }

        let files = &self.input.files;
        for i in (0..nodes.len()).rev() {
            let mut shape = blake3::Hasher::new();
            let mut merkle = blake3::Hasher::new();
            let n = &nodes[i];
            merkle.update(n.kind.as_str().as_bytes());
            merkle.update(&[0]);
            merkle.update(n.key.as_bytes());
            merkle.update(&[0]);
            if let Some(f) = n.file {
                let f = &files[f as usize];
                merkle.update(f.path.as_bytes());
                merkle.update(&[0]);
                merkle.update(&f.loc.to_le_bytes());
                merkle.update(&[u8::from(f.generated)]);
            }
            merkle.update(&(n.children.len() as u64).to_le_bytes());
            for &c in &n.children {
                shape.update(nodes[c].key.as_bytes());
                shape.update(&[0]);
                merkle.update(&nodes[c].merkle);
            }
            let pairs = &mut inner[i];
            pairs.sort_by(|x, y| {
                let path = |k: u32| files[k as usize].path.as_str();
                (path(x.src), path(x.dst)).cmp(&(path(y.src), path(y.dst)))
            });
            merkle.update(&(pairs.len() as u64).to_le_bytes());
            for l in pairs.iter() {
                merkle.update(files[l.src as usize].path.as_bytes());
                merkle.update(&[0]);
                merkle.update(files[l.dst as usize].path.as_bytes());
                merkle.update(&[0]);
                merkle.update(&l.refs.to_le_bytes());
            }
            nodes[i].shape_hash = *shape.finalize().as_bytes();
            nodes[i].merkle = *merkle.finalize().as_bytes();
        }

        let by_id = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();
        Tree {
            nodes,
            root: 0,
            file_node,
            by_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_ids_hash_the_key() {
        assert_eq!(group_id("src/orders"), group_id("src/orders"));
        assert_ne!(group_id("src/orders"), group_id("src/cart"));
        assert_eq!(
            group_id("src/orders"),
            format!("g_{}", &blake3::hash(b"src/orders").to_hex()[..12])
        );
        assert_eq!(join("", "~more"), "~more");
        assert_eq!(join("src", "~more"), "src/~more");
        assert_eq!(parent_dir("a/b/c.ts"), "a/b");
        assert_eq!(parent_dir("c.ts"), "");
    }
}
