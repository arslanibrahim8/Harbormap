//! Precomputed positions (Learn L5a scope decision "Layout"): all integers, in pixels.
//!
//! - **Overview:** boxes of 200 × 72 in role lanes, one column per non-empty lane in
//!   the order see, work, keep, outside; `shared` boxes form one row under the
//!   tallest column. Inside a lane, boxes are ordered by the barycentre of the boxes
//!   they link to in earlier lanes (a forward sweep, then a backward one over later
//!   lanes), ties by files (most first) and name. Items outside the lanes (the
//!   `shared` boxes) never count as neighbours.
//! - **Inside a group:** children of 176 × 52 in a small layered layout. Ranks are
//!   longest paths from the sources of the condensation of the child links (Tarjan's
//!   strongly connected components); ranks from 4 up share the last of at most 5
//!   columns. Within a column the beginner children come first, then the order of
//!   two barycentre sweeps, ties by name. The group's size fits its grid plus
//!   padding and a header.
//! - **Wrapping:** a lane or rank column holds at most 6 boxes; a longer one wraps
//!   into sub-columns of its own, filled top to bottom in the order computed above.
//!
//! A [`Placement`] depends only on the children it is given and their links; the
//! map caches it by the node's shape hash, so a new link never moves a box.

use std::cmp::Ordering;

use crate::roles::Role;

/// The version of the layout parameters below. Cached placements carry it in their
/// key, so changing a parameter and bumping it drops them.
pub const PARAMS_VERSION: u32 = 2;

/// Overview box size.
pub(crate) const TOP_W: u32 = 200;
pub(crate) const TOP_H: u32 = 72;
/// Gaps between overview columns and rows.
const TOP_GAP_X: u32 = 120;
const TOP_GAP_Y: u32 = 40;

/// Group child size.
pub(crate) const CHILD_W: u32 = 176;
pub(crate) const CHILD_H: u32 = 52;
/// Gaps between group columns and rows, the padding around the grid and the header
/// above it.
const CHILD_GAP_X: u32 = 96;
const CHILD_GAP_Y: u32 = 24;
const PAD: u32 = 24;
const HEADER: u32 = 44;
/// Ranks from `MAX_COLUMNS - 1` up share the last column.
const MAX_COLUMNS: usize = 5;
/// A lane or rank column holds at most this many boxes before it wraps.
const MAX_ROWS: usize = 6;

/// The overview lanes, left to right.
const LANES: [Role; 4] = [Role::See, Role::Work, Role::Keep, Role::Outside];

/// The positions of a node's children, in the node's child order, relative to the
/// node (world coordinates for the overview), and the size that holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placement {
    pub w: u32,
    pub h: u32,
    pub at: Vec<(u32, u32)>,
}

/// What the layout knows about one child.
pub(crate) struct Item<'a> {
    pub name: &'a str,
    pub files: u32,
    pub role: Role,
    pub beginner: bool,
}

/// A directed link `from → to` between two items (indexes into the items) with its
/// weight. Pairs may repeat; their weights add up.
pub(crate) type Edge = (usize, usize, u64);

/// Places the overview boxes in role lanes.
pub(crate) fn overview(items: &[Item], edges: &[Edge]) -> Placement {
    let by_size = |a: usize, b: usize| {
        let (x, y) = (&items[a], &items[b]);
        y.files
            .cmp(&x.files)
            .then_with(|| x.name.cmp(y.name))
            .then_with(|| a.cmp(&b))
    };
    let mut columns: Vec<Vec<usize>> = LANES
        .iter()
        .map(|&lane| {
            let mut c: Vec<usize> = (0..items.len())
                .filter(|&i| items[i].role == lane)
                .collect();
            c.sort_by(|&a, &b| by_size(a, b));
            c
        })
        .filter(|c| !c.is_empty())
        .collect();
    sweeps(&mut columns, items.len(), edges, &by_size, &|_| 0);
    let mut shared: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].role == Role::Shared)
        .collect();
    shared.sort_by(|&a, &b| by_size(a, b));

    let (cells, _, tallest) = grid(&columns);
    let mut at = vec![(0, 0); items.len()];
    for (i, c, r) in cells {
        at[i] = (c * (TOP_W + TOP_GAP_X), r * (TOP_H + TOP_GAP_Y));
    }
    for (k, &i) in shared.iter().enumerate() {
        at[i] = (
            k as u32 * (TOP_W + TOP_GAP_X),
            tallest * (TOP_H + TOP_GAP_Y),
        );
    }
    let w = at.iter().map(|&(x, _)| x + TOP_W).max().unwrap_or(0);
    let h = at.iter().map(|&(_, y)| y + TOP_H).max().unwrap_or(0);
    Placement { w, h, at }
}

/// Places the children of one group in at most [`MAX_COLUMNS`] ranked columns, each
/// wrapped at [`MAX_ROWS`].
pub(crate) fn group(items: &[Item], edges: &[Edge]) -> Placement {
    let n = items.len();
    let column: Vec<usize> = ranks(n, edges)
        .into_iter()
        .map(|r| r.min(MAX_COLUMNS - 1))
        .collect();
    let ncols = column.iter().map(|&c| c + 1).max().unwrap_or(0);
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); ncols];
    for (i, &c) in column.iter().enumerate() {
        columns[c].push(i);
    }
    let by_name = |a: usize, b: usize| items[a].name.cmp(items[b].name).then_with(|| a.cmp(&b));
    let band = |i: usize| usize::from(!items[i].beginner);
    for c in &mut columns {
        c.sort_by(|&a, &b| band(a).cmp(&band(b)).then_with(|| by_name(a, b)));
    }
    sweeps(&mut columns, n, edges, &by_name, &band);

    let (cells, ncols, rows) = grid(&columns);
    let mut at = vec![(0, 0); n];
    for (i, c, r) in cells {
        at[i] = (
            PAD + c * (CHILD_W + CHILD_GAP_X),
            HEADER + r * (CHILD_H + CHILD_GAP_Y),
        );
    }
    let w = 2 * PAD + ncols * CHILD_W + ncols.saturating_sub(1) * CHILD_GAP_X;
    let h = HEADER + rows * CHILD_H + rows.saturating_sub(1) * CHILD_GAP_Y + PAD;
    Placement { w, h, at }
}

/// The grid cell `(item, column, row)` of every item of `columns`, each column cut
/// into sub-columns of at most [`MAX_ROWS`]; also the number of grid columns and of
/// rows used.
fn grid(columns: &[Vec<usize>]) -> (Vec<(usize, u32, u32)>, u32, u32) {
    let mut cells = Vec::new();
    let (mut base, mut rows) = (0, 0);
    for column in columns {
        for (k, &i) in column.iter().enumerate() {
            let (c, r) = ((base + k / MAX_ROWS) as u32, (k % MAX_ROWS) as u32);
            cells.push((i, c, r));
            rows = rows.max(r + 1);
        }
        base += column.len().div_ceil(MAX_ROWS);
    }
    (cells, base as u32, rows)
}

/// Reorders each column by the barycentre of its links: a forward sweep over earlier
/// columns, then a backward sweep over later ones. `band` sorts first (beginner
/// children ahead of the others); an item with no link to the columns looked at keeps
/// its current row as its barycentre; `tie` breaks equal barycentres. Items in no
/// column (the overview's `shared` boxes) are never neighbours. Rows are positions
/// in the whole column, before it wraps.
fn sweeps(
    columns: &mut [Vec<usize>],
    n: usize,
    edges: &[Edge],
    tie: &dyn Fn(usize, usize) -> Ordering,
    band: &dyn Fn(usize) -> usize,
) {
    // Items in no column keep `usize::MAX` and are skipped as neighbours.
    let mut column_of = vec![usize::MAX; n];
    let mut row = vec![0usize; n];
    for (c, column) in columns.iter().enumerate() {
        for (r, &i) in column.iter().enumerate() {
            (column_of[i], row[i]) = (c, r);
        }
    }
    // For ordering, a link pulls both of its ends.
    let mut adj: Vec<Vec<(usize, u64)>> = vec![Vec::new(); n];
    for &(a, b, w) in edges {
        if a != b {
            adj[a].push((b, w));
            adj[b].push((a, w));
        }
    }
    let k = columns.len();
    let forward = (1..k).map(|c| (c, true));
    let backward = (0..k.saturating_sub(1)).rev().map(|c| (c, false));
    for (c, earlier) in forward.chain(backward) {
        let mut order: Vec<(usize, f64)> = columns[c]
            .iter()
            .map(|&i| {
                let (mut sum, mut weight) = (0.0, 0.0);
                for &(j, w) in &adj[i] {
                    let looked_at = match column_of[j] {
                        usize::MAX => false,
                        cj if earlier => cj < c,
                        cj => cj > c,
                    };
                    if looked_at {
                        sum += w as f64 * row[j] as f64;
                        weight += w as f64;
                    }
                }
                let bary = if weight > 0.0 {
                    sum / weight
                } else {
                    row[i] as f64
                };
                (i, bary)
            })
            .collect();
        order.sort_by(|&(a, x), &(b, y)| {
            band(a)
                .cmp(&band(b))
                .then_with(|| x.total_cmp(&y))
                .then_with(|| tie(a, b))
        });
        columns[c] = order.into_iter().map(|(i, _)| i).collect();
        for (r, &i) in columns[c].iter().enumerate() {
            row[i] = r;
        }
    }
}

/// The rank of each item: the longest path to it from a source of the condensation
/// of `edges` (the items of one strongly connected component share a rank).
fn ranks(n: usize, edges: &[Edge]) -> Vec<usize> {
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b, _) in edges {
        if a != b {
            out_edges[a].push(b);
        }
    }
    for e in &mut out_edges {
        e.sort_unstable();
        e.dedup();
    }
    let comps = tarjan(&out_edges);
    let mut comp_of = vec![0usize; n];
    for (c, members) in comps.iter().enumerate() {
        for &i in members {
            comp_of[i] = c;
        }
    }
    // Components come out in reverse topological order: walk them backwards.
    let mut rank = vec![0usize; comps.len()];
    for c in (0..comps.len()).rev() {
        for &i in &comps[c] {
            for &j in &out_edges[i] {
                let d = comp_of[j];
                if d != c {
                    rank[d] = rank[d].max(rank[c] + 1);
                }
            }
        }
    }
    (0..n).map(|i| rank[comp_of[i]]).collect()
}

/// Tarjan's strongly connected components, without recursion. A component is
/// emitted only after every component it reaches, so the list is in reverse
/// topological order of the condensation.
fn tarjan(out_edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = out_edges.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut comps = Vec::new();
    let mut next = 0;
    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        // Each frame is a node and the next of its edges to follow.
        let mut call = vec![(root, 0usize)];
        (index[root], low[root]) = (next, next);
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(frame) = call.last_mut() {
            let v = frame.0;
            if let Some(&w) = out_edges[v].get(frame.1) {
                frame.1 += 1;
                if index[w] == usize::MAX {
                    (index[w], low[w]) = (next, next);
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            call.pop();
            if let Some(&(parent, _)) = call.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                let mut comp = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    comp.push(w);
                    if w == v {
                        break;
                    }
                }
                comp.sort_unstable();
                comps.push(comp);
            }
        }
    }
    comps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str) -> Item<'_> {
        Item {
            name,
            files: 1,
            role: Role::Work,
            beginner: true,
        }
    }

    #[test]
    fn cycles_share_a_rank_and_deep_chains_share_the_last_column() {
        // 0 → 1 → 2 → 1 (the cycle {1, 2}) → 3
        assert_eq!(
            ranks(4, &[(0, 1, 1), (1, 2, 1), (2, 1, 1), (2, 3, 1)]),
            [0, 1, 1, 2]
        );
        // A chain of 7: ranks 0..6, columns 0..4.
        let names: Vec<String> = (0..7).map(|i| format!("n{i}")).collect();
        let items: Vec<Item> = names.iter().map(|n| item(n)).collect();
        let edges: Vec<Edge> = (0..6).map(|i| (i, i + 1, 1)).collect();
        let p = group(&items, &edges);
        let xs: Vec<u32> = p.at.iter().map(|&(x, _)| x).collect();
        assert_eq!(xs[0], PAD);
        assert_eq!((xs[4], xs[5]), (xs[6], xs[6]));
        assert_eq!(p.w, 2 * PAD + 5 * CHILD_W + 4 * CHILD_GAP_X);
        assert_eq!(p.h, HEADER + 3 * CHILD_H + 2 * CHILD_GAP_Y + PAD);
    }

    #[test]
    fn a_child_follows_the_barycentre_of_its_links() {
        // Column 0: a, b, c (by name); column 1: x linked from c, y linked from a.
        let names = ["a", "b", "c", "x", "y"];
        let items: Vec<Item> = names.iter().map(|n| item(n)).collect();
        let p = group(&items, &[(2, 3, 1), (0, 4, 1), (1, 1, 1)]);
        assert_eq!(p.at[3].0, p.at[4].0);
        assert!(p.at[4].1 < p.at[3].1, "y (from a) sits above x (from c)");
    }
}
