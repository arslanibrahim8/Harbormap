//! The map the server holds: the group tree of the current facts, the links it counts
//! arrows from and the precomputed positions of every box. The views the panel API
//! sends ([`OverviewView`], [`GroupView`], [`FileLinks`]) are cut from it on request.
//!
//! - **Arrows:** every link is lifted to the visible node holding each end (overview:
//!   the top boxes; a group: its children). Self-arrows are dropped, `refs` add up
//!   per ordered pair (A→B and B→A stay apart), arrows sort by count (most first),
//!   then `from`, then `to`, and only the first [`BUILDER_ARROWS`] are sent; the rest
//!   are counted in `hidden_arrows`.
//! - **Chips:** in a group view, a link with one end outside the group is lifted to
//!   the top box holding that end and counted on that box's chip, `out` of the group
//!   or `in` to it.
//! - **Beginner:** the [`BEGINNER_CHILDREN`] children of a group with the most link
//!   weight crossing their border (then most files, then name) are flagged; every
//!   overview box is. The browser shows only those at the Beginner level.
//! - **Positions** are cached by (shape hash, [`PARAMS_VERSION`]): a rebuild with the
//!   previous map lays out again only the nodes whose child set changed, so a new
//!   link never moves a box (L5a scope decision "Position stability beats
//!   freshness").

use std::collections::{BTreeMap, BTreeSet};

use hm_core::ids::file_id;
use serde::Serialize;

use crate::input::{InLink, Input};
use crate::place::{self, CHILD_H, CHILD_W, Edge, Item, PARAMS_VERSION, Placement, TOP_H, TOP_W};
use crate::roles::{Role, role_word};
use crate::tree::{self, NodeKind, Tree};

/// Arrows the browser shows at the Beginner level.
pub const BEGINNER_ARROWS: usize = 10;
/// Arrows a view sends (all shown at the Builder level); the rest are only counted.
pub const BUILDER_ARROWS: usize = 40;
/// Children of a group flagged `beginner`.
pub const BEGINNER_CHILDREN: usize = 12;
/// No view holds more items (boxes + arrows + chips) than this.
pub const MAX_ITEMS: usize = 150;
/// Chips a group view sends; the rest are counted in `elsewhere_more`.
pub const MAX_CHIPS: usize = 8;

/// One build of the map. Two cold builds of the same input compare equal. A warm
/// build (with `previous`) equals the cold one only when every node it reuses a
/// placement for had the same links in the previous map, for example when the
/// previous map was built from the same input; otherwise it keeps the older
/// positions on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    tree: Tree,
    /// The input links as the tree reads them ([`Input::clean_links`]).
    links: Vec<InLink>,
    /// The `generated` flag of each input file.
    generated: Vec<bool>,
    /// The subtree of node `i` is `i..end[i]` (nodes are in pre-order).
    end: Vec<usize>,
    /// The refs of the links with exactly one end below each node.
    weight: Vec<u64>,
    /// The placement of every node that has children, by its key.
    placements: BTreeMap<PlaceKey, Placement>,
}

/// What a placement is cached by. The overview and a group are laid out differently,
/// so the root's placement is never reused for a group with the same children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PlaceKey {
    shape: [u8; 32],
    version: u32,
    overview: bool,
}

/// A box of the overview or of a group view (the API's `Box`). Overview boxes are in
/// world coordinates; group children are relative to the group's top-left corner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoxView {
    pub id: String,
    pub name: String,
    pub kind: NodeKind,
    pub role: Role,
    pub role_word: String,
    pub files: u32,
    pub loc: u32,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// `false` only for a file.
    pub expandable: bool,
    pub beginner: bool,
    /// The file's path; files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Whether the file is generated; files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated: Option<bool>,
}

/// A counted arrow between two boxes of one view (the API's `Arrow`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArrowView {
    pub from: String,
    pub to: String,
    /// The refs of the links it stands for.
    pub count: u64,
}

/// A top box linked to a group from outside it (the API's `Chip`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChipView {
    pub id: String,
    pub name: String,
    pub role: Role,
    /// Refs from files in the group to files in this box.
    pub out: u64,
    /// Refs from files in this box to files in the group.
    #[serde(rename = "in")]
    pub into: u64,
}

/// One step of a group's breadcrumb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Crumb {
    pub id: String,
    pub name: String,
}

/// The overview's boxes and arrows; the server adds `v`, `seq` and the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverviewView {
    pub boxes: Vec<BoxView>,
    pub arrows: Vec<ArrowView>,
    pub hidden_arrows: usize,
}

/// One group's content; the server adds `v` and `seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub role_word: String,
    /// The size that holds the children's grid.
    pub w: u32,
    pub h: u32,
    /// From the top box down to the group itself; the root is left out.
    pub breadcrumb: Vec<Crumb>,
    /// In key order.
    pub children: Vec<BoxView>,
    pub arrows: Vec<ArrowView>,
    pub hidden_arrows: usize,
    pub elsewhere: Vec<ChipView>,
    pub elsewhere_more: usize,
}

/// A box a file's links lead to or come from, with their refs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkEnd {
    pub id: String,
    pub count: u64,
}

/// A file's links lifted to the boxes visible in one view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileLinks {
    /// `"overview"` or a group id, as asked.
    pub view: String,
    /// Sorted by count (most first), then id.
    pub incoming: Vec<LinkEnd>,
    pub outgoing: Vec<LinkEnd>,
}

/// What changed between two builds, for the live loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// Nodes appeared, disappeared or moved to another parent.
    pub regrouped: bool,
    /// The groups (never files, never the root) whose view may differ, sorted.
    pub groups: Vec<String>,
}

impl Map {
    /// Builds the map of `input`; the root box is named `project_name`. `previous` is
    /// the map of the last build in this run: its placements are reused for every
    /// node whose child set did not change.
    pub fn build(input: &Input, project_name: &str, previous: Option<&Map>) -> Map {
        let tree = tree::build(input, project_name);
        let links = input.clean_links();
        let nodes = &tree.nodes;
        let n = nodes.len();
        let parent = |i: usize| nodes[i].parent.unwrap_or(i);

        let mut end: Vec<usize> = (1..=n).collect();
        let mut depth = vec![0usize; n];
        for i in 1..n {
            depth[i] = depth[parent(i)] + 1;
        }
        for i in (1..n).rev() {
            end[parent(i)] = end[parent(i)].max(end[i]);
        }

        // Each link runs between two children of the lowest node holding both ends;
        // every node it leaves on the way up counts its refs as crossing weight.
        let mut weight = vec![0u64; n];
        let mut inner: Vec<Vec<(usize, usize, u64)>> = vec![Vec::new(); n];
        for l in &links {
            let r = u64::from(l.refs);
            let (mut a, mut b) = (
                tree.file_node[l.src as usize],
                tree.file_node[l.dst as usize],
            );
            while depth[a] > depth[b] {
                weight[a] += r;
                a = parent(a);
            }
            while depth[b] > depth[a] {
                weight[b] += r;
                b = parent(b);
            }
            while parent(a) != parent(b) {
                weight[a] += r;
                weight[b] += r;
                (a, b) = (parent(a), parent(b));
            }
            weight[a] += r;
            weight[b] += r;
            inner[parent(a)].push((a, b, r));
        }

        let mut placements = BTreeMap::new();
        for (g, node) in nodes.iter().enumerate() {
            if node.children.is_empty() {
                continue;
            }
            let key = PlaceKey {
                shape: node.shape_hash,
                version: PARAMS_VERSION,
                overview: g == tree.root,
            };
            let cached = previous
                .and_then(|m| m.placements.get(&key))
                .filter(|p| p.at.len() == node.children.len());
            let placement = match cached {
                Some(p) => p.clone(),
                None => {
                    let beginner = beginners(&tree, &weight, g);
                    let items: Vec<Item> = node
                        .children
                        .iter()
                        .zip(&beginner)
                        .map(|(&c, &beginner)| Item {
                            name: &nodes[c].name,
                            files: nodes[c].files,
                            role: nodes[c].role,
                            beginner,
                        })
                        .collect();
                    let at = |c: usize| {
                        let k = node.children.binary_search(&c);
                        debug_assert!(k.is_ok(), "an inner link ends at a child of its node");
                        k.unwrap_or(0)
                    };
                    let edges: Vec<Edge> = inner[g]
                        .iter()
                        .map(|&(a, b, r)| (at(a), at(b), r))
                        .collect();
                    if g == tree.root {
                        place::overview(&items, &edges)
                    } else {
                        place::group(&items, &edges)
                    }
                }
            };
            placements.insert(key, placement);
        }

        Map {
            generated: input.files.iter().map(|f| f.generated).collect(),
            tree,
            links,
            end,
            weight,
            placements,
        }
    }

    /// The group tree.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The links the map was built from: valid, one per `(src, dst)`, refs added up,
    /// sorted by `(src, dst)`.
    pub fn links(&self) -> &[InLink] {
        &self.links
    }

    /// The overview: the top boxes and the arrows between them.
    pub fn overview(&self) -> OverviewView {
        let root = self.tree.root;
        let boxes = match self.placement(root) {
            Some(p) => (self.tree.nodes[root].children.iter())
                .zip(&p.at)
                .map(|(&c, &(x, y))| self.box_view(c, (x, y, TOP_W, TOP_H), true))
                .collect(),
            None => Vec::new(),
        };
        let mut pairs = BTreeMap::new();
        for (a, b, r) in self.link_nodes() {
            let (a, b) = (self.top_of(a), self.top_of(b));
            if a != b {
                *pairs.entry((a, b)).or_insert(0) += r;
            }
        }
        let (arrows, hidden_arrows) = self.arrows(pairs);
        OverviewView {
            boxes,
            arrows,
            hidden_arrows,
        }
    }

    /// The view of group `id`: its children, the arrows between them and chips for
    /// its links to other top boxes. `None` for an unknown id, a file or the root.
    pub fn group(&self, id: &str) -> Option<GroupView> {
        let g = self.group_index(id)?;
        let node = &self.tree.nodes[g];
        let p = self.placement(g)?;
        let beginner = beginners(&self.tree, &self.weight, g);
        let children = (node.children.iter().zip(&p.at).zip(beginner))
            .map(|((&c, &(x, y)), b)| self.box_view(c, (x, y, CHILD_W, CHILD_H), b))
            .collect();

        let mut pairs = BTreeMap::new();
        let mut chips: BTreeMap<usize, (u64, u64)> = BTreeMap::new();
        for (a, b, r) in self.link_nodes() {
            match (self.inside(g, a), self.inside(g, b)) {
                (true, true) => {
                    let (a, b) = (self.child_holding(g, a), self.child_holding(g, b));
                    if a != b {
                        *pairs.entry((a, b)).or_insert(0) += r;
                    }
                }
                (true, false) => chips.entry(self.top_of(b)).or_default().0 += r,
                (false, true) => chips.entry(self.top_of(a)).or_default().1 += r,
                (false, false) => {}
            }
        }
        let (arrows, hidden_arrows) = self.arrows(pairs);
        let mut elsewhere: Vec<ChipView> = chips
            .into_iter()
            .map(|(t, (out, into))| {
                let t = &self.tree.nodes[t];
                ChipView {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    role: t.role,
                    out,
                    into,
                }
            })
            .collect();
        elsewhere.sort_by(|a, b| {
            (b.out + b.into)
                .cmp(&(a.out + a.into))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        let elsewhere_more = elsewhere.len().saturating_sub(MAX_CHIPS);
        elsewhere.truncate(MAX_CHIPS);

        let breadcrumb = (self.path_down_to(g).into_iter())
            .map(|i| Crumb {
                id: self.tree.nodes[i].id.clone(),
                name: self.tree.nodes[i].name.clone(),
            })
            .collect();
        Some(GroupView {
            id: node.id.clone(),
            name: node.name.clone(),
            role: node.role,
            role_word: role_word(node.role, &node.name).to_string(),
            w: p.w,
            h: p.h,
            breadcrumb,
            children,
            arrows,
            hidden_arrows,
            elsewhere,
            elsewhere_more,
        })
    }

    /// The links of the file at `path`, lifted to the boxes visible in `view`
    /// (`"overview"` or a group id): for a group, its children for ends inside it and
    /// the top boxes for ends outside it. Links to the box that holds the file itself
    /// are left out. `None` when the file is not on the map or the view is unknown.
    pub fn file_links(&self, path: &str, view: &str) -> Option<FileLinks> {
        let f = self.file_index(path)?;
        let g = match view {
            "overview" => None,
            id => Some(self.group_index(id)?),
        };
        let lift = |n: usize| match g {
            Some(g) if self.inside(g, n) => self.child_holding(g, n),
            _ => self.top_of(n),
        };
        let holder = lift(f);
        let (mut incoming, mut outgoing) = (BTreeMap::new(), BTreeMap::new());
        for (a, b, r) in self.link_nodes() {
            if a == f && lift(b) != holder {
                *outgoing.entry(lift(b)).or_insert(0) += r;
            }
            if b == f && lift(a) != holder {
                *incoming.entry(lift(a)).or_insert(0) += r;
            }
        }
        let ends = |counts: BTreeMap<usize, u64>| {
            let mut v: Vec<LinkEnd> = counts
                .into_iter()
                .map(|(n, count)| LinkEnd {
                    id: self.tree.nodes[n].id.clone(),
                    count,
                })
                .collect();
            v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.id.cmp(&b.id)));
            v
        };
        Some(FileLinks {
            view: view.to_string(),
            incoming: ends(incoming),
            outgoing: ends(outgoing),
        })
    }

    /// The ids of the groups holding the file at `path`, from its top box down to its
    /// parent (empty for a file that is itself a top box). `None` when the file is
    /// not on the map.
    pub fn ancestors(&self, path: &str) -> Option<Vec<String>> {
        let f = self.file_index(path)?;
        let mut down = self.path_down_to(f);
        down.pop();
        Some(
            down.into_iter()
                .map(|i| self.tree.nodes[i].id.clone())
                .collect(),
        )
    }

    /// What changed since `old`. A group is reported when its merkle changed (its
    /// content or the links between its children), when it is new, or when a link
    /// with an end inside it was added, removed or changed its refs, since its chips
    /// and arrows can change without its merkle. Node names derive from keys and
    /// child sets, so a renamed node always comes with `regrouped`.
    pub fn changed_since(&self, old: &Map) -> Changed {
        // Ids are unique, so equal counts and every new id known with the same parent
        // mean the same nodes in the same places.
        let (new_nodes, old_nodes) = (&self.tree.nodes, &old.tree.nodes);
        let regrouped = new_nodes.len() != old_nodes.len()
            || new_nodes.iter().any(|n| match old.tree.node(&n.id) {
                None => true,
                Some(j) => {
                    n.parent.map(|p| new_nodes[p].id.as_str())
                        != old_nodes[j].parent.map(|p| old_nodes[p].id.as_str())
                }
            });

        let mut marked: BTreeSet<String> = BTreeSet::new();
        for n in &self.tree.nodes {
            let same = old
                .tree
                .node(&n.id)
                .is_some_and(|j| old.tree.nodes[j].merkle == n.merkle);
            if !same {
                marked.insert(n.id.clone());
            }
        }
        // Both lists are sorted by path pair: one merge finds the differing links.
        let (new_links, old_links) = (self.links_by_path(), old.links_by_path());
        let mut differ = Vec::new();
        let (mut i, mut j) = (0, 0);
        loop {
            match (new_links.get(i), old_links.get(j)) {
                (Some(a), Some(b)) if a.0 == b.0 => {
                    if a.1 != b.1 {
                        differ.push(a.0);
                    }
                    (i, j) = (i + 1, j + 1);
                }
                (Some(a), Some(b)) if a.0 < b.0 => {
                    differ.push(a.0);
                    i += 1;
                }
                (Some(a), None) => {
                    differ.push(a.0);
                    i += 1;
                }
                (_, Some(b)) => {
                    differ.push(b.0);
                    j += 1;
                }
                (None, None) => break,
            }
        }
        // One ancestor walk per file, however many of its links differ.
        let files: BTreeSet<&str> = differ.into_iter().flat_map(|(s, d)| [s, d]).collect();
        for path in files {
            for m in [self, old] {
                marked.extend(m.ancestors(path).unwrap_or_default());
            }
        }
        let groups = marked
            .into_iter()
            .filter(|id| self.group_index(id).is_some())
            .collect();
        Changed { regrouped, groups }
    }

    /// The placement of node `i`.
    fn placement(&self, i: usize) -> Option<&Placement> {
        let key = PlaceKey {
            shape: self.tree.nodes[i].shape_hash,
            version: PARAMS_VERSION,
            overview: i == self.tree.root,
        };
        self.placements.get(&key)
    }

    /// The node of group `id`: not a file and not the root.
    fn group_index(&self, id: &str) -> Option<usize> {
        let g = self.tree.node(id)?;
        let is_group = g != self.tree.root && self.tree.nodes[g].kind != NodeKind::File;
        is_group.then_some(g)
    }

    /// The node of the file at `path`.
    fn file_index(&self, path: &str) -> Option<usize> {
        let f = self.tree.node(&file_id(path))?;
        let n = &self.tree.nodes[f];
        (n.kind == NodeKind::File && n.key == path).then_some(f)
    }

    /// The links as `(src node, dst node, refs)`.
    fn link_nodes(&self) -> impl Iterator<Item = (usize, usize, u64)> + '_ {
        let file_node = &self.tree.file_node;
        self.links.iter().map(|l| {
            (
                file_node[l.src as usize],
                file_node[l.dst as usize],
                u64::from(l.refs),
            )
        })
    }

    /// The links as `((src path, dst path), refs)`, sorted by path pair.
    fn links_by_path(&self) -> Vec<((&str, &str), u32)> {
        let path = |f: u32| {
            self.tree.nodes[self.tree.file_node[f as usize]]
                .key
                .as_str()
        };
        let mut out: Vec<((&str, &str), u32)> = (self.links.iter())
            .map(|l| ((path(l.src), path(l.dst)), l.refs))
            .collect();
        // Already sorted when the input files are sorted by path, as they should be.
        out.sort_unstable();
        out
    }

    /// Whether node `n` is strictly below node `g`.
    fn inside(&self, g: usize, n: usize) -> bool {
        g < n && n < self.end[g]
    }

    /// The child of `g` whose subtree holds `n` (which must be below `g`). Children
    /// are in pre-order, so their indexes increase.
    fn child_holding(&self, g: usize, n: usize) -> usize {
        let children = &self.tree.nodes[g].children;
        let k = children.partition_point(|&c| c <= n);
        debug_assert!(k > 0 && self.inside(g, n), "node {n} is not below {g}");
        children[k.saturating_sub(1)]
    }

    /// The top box holding node `n`.
    fn top_of(&self, n: usize) -> usize {
        self.child_holding(self.tree.root, n)
    }

    /// The nodes from the top box down to `n` itself; the root is left out.
    fn path_down_to(&self, n: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut i = n;
        while i != self.tree.root {
            out.push(i);
            match self.tree.nodes[i].parent {
                Some(p) => i = p,
                None => break,
            }
        }
        out.reverse();
        out
    }

    /// The arrows of counted `(from, to)` node pairs: sorted by count (most first),
    /// then ids, cut at [`BUILDER_ARROWS`]; also the number cut off.
    fn arrows(&self, pairs: BTreeMap<(usize, usize), u64>) -> (Vec<ArrowView>, usize) {
        let id = |i: usize| self.tree.nodes[i].id.clone();
        let mut arrows: Vec<ArrowView> = pairs
            .into_iter()
            .map(|((a, b), count)| ArrowView {
                from: id(a),
                to: id(b),
                count,
            })
            .collect();
        arrows.sort_by(|a, b| {
            (b.count.cmp(&a.count))
                .then_with(|| a.from.cmp(&b.from))
                .then_with(|| a.to.cmp(&b.to))
        });
        let hidden = arrows.len().saturating_sub(BUILDER_ARROWS);
        arrows.truncate(BUILDER_ARROWS);
        (arrows, hidden)
    }

    /// The box of node `i` at `(x, y, w, h)`.
    fn box_view(&self, i: usize, (x, y, w, h): (u32, u32, u32, u32), beginner: bool) -> BoxView {
        let n = &self.tree.nodes[i];
        BoxView {
            id: n.id.clone(),
            name: n.name.clone(),
            kind: n.kind,
            role: n.role,
            role_word: role_word(n.role, &n.name).to_string(),
            files: n.files,
            loc: n.loc,
            x,
            y,
            w,
            h,
            expandable: !(n.kind == NodeKind::File && n.children.is_empty()),
            beginner,
            path: n.file.map(|_| n.key.clone()),
            generated: n.file.map(|f| self.generated[f as usize]),
        }
    }
}

/// Which children of `g` are flagged `beginner`, in child order: all of them on the
/// overview; in a group, the [`BEGINNER_CHILDREN`] with the most crossing link weight,
/// then most files, then name.
fn beginners(tree: &Tree, weight: &[u64], g: usize) -> Vec<bool> {
    let children = &tree.nodes[g].children;
    if g == tree.root || children.len() <= BEGINNER_CHILDREN {
        return vec![true; children.len()];
    }
    let mut order: Vec<usize> = (0..children.len()).collect();
    order.sort_by(|&a, &b| {
        let (ca, cb) = (children[a], children[b]);
        let (na, nb) = (&tree.nodes[ca], &tree.nodes[cb]);
        (weight[cb].cmp(&weight[ca]))
            .then_with(|| nb.files.cmp(&na.files))
            .then_with(|| na.name.cmp(&nb.name))
            .then_with(|| na.key.cmp(&nb.key))
    });
    let mut out = vec![false; children.len()];
    for &k in order.iter().take(BEGINNER_CHILDREN) {
        out[k] = true;
    }
    out
}
