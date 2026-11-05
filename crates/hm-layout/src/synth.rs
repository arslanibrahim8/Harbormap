//! Synthetic shop projects for property tests: nested feature folders, large flat
//! folders (orders, migrations), a single-folder chain, generated files, about three
//! links per file and a few hub files. Everything is a pure function of the seed, so a
//! failure reproduces from its seed.

use std::collections::BTreeSet;

use crate::input::{InFile, InLink, Input};

/// Files that most other files import, so they become hubs in larger projects.
const HUBS: [&str; 3] = [
    "src/lib/db.ts",
    "src/shared/utils/format.ts",
    "src/components/ui/Button.tsx",
];
/// The chance, in percent, that a file imports each hub.
const HUB_PERCENT: u64 = 20;

const ENTITIES: [&str; 12] = [
    "order", "cart", "user", "product", "payment", "invoice", "shipment", "review", "coupon",
    "address", "category", "stock",
];
const COMPONENTS: [&str; 9] = [
    "Card", "List", "Form", "Row", "Header", "Badge", "Dialog", "Table", "Summary",
];
const UI: [&str; 10] = [
    "Button", "Input", "Modal", "Select", "Tabs", "Toast", "Spinner", "Tooltip", "Avatar",
    "Checkbox",
];
const LOGIC: [&str; 7] = [
    "Service",
    "Controller",
    "Repository",
    "Mapper",
    "Validator",
    "Policy",
    "Events",
];
const VERBS: [&str; 5] = ["create", "add", "drop", "alter", "index"];
const UTILS: [&str; 8] = [
    "money", "date", "format", "slug", "retry", "logger", "cache", "http",
];
const FEATURE_PARTS: [&str; 4] = ["components", "hooks", "api", "state"];
const PARTS: [&str; 6] = ["View", "Panel", "Hook", "Query", "Store", "Item"];
const SERVICES: [&str; 4] = ["payments", "mail", "shipping", "search"];
const SERVICE_FILES: [&str; 4] = ["client", "webhook", "mapper", "types"];

/// A splitmix64 generator: small, fast and the same on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `lo..=hi`.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.range(0, items.len() as u64 - 1) as usize]
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().chain(chars).collect::<String>()
    })
}

/// One made-up file path, and whether it is generated.
fn path(rng: &mut Rng) -> (String, bool) {
    let e = rng.pick(&ENTITIES);
    let big = capitalized(e);
    // (weight, template) pairs; the weights add up to 80.
    let roll = rng.range(0, 79);
    let path = match roll {
        0..6 => match rng.range(0, 2) {
            0 => format!("src/app/{e}s/page.tsx"),
            1 => format!("src/app/{e}s/[id]/page.tsx"),
            _ => format!("src/app/{e}s/layout.tsx"),
        },
        6..18 => format!("src/components/{e}/{big}{}.tsx", rng.pick(&COMPONENTS)),
        18..22 => format!("src/components/ui/{}.tsx", rng.pick(&UI)),
        22..30 => format!("src/orders/{e}{}.ts", rng.pick(&LOGIC)),
        30..34 => match rng.range(0, 1) {
            0 => format!("src/api/{e}s/route.ts"),
            _ => format!("src/api/{e}s/handlers.ts"),
        },
        34..37 => format!("src/db/models/{e}.ts"),
        37..45 => format!(
            "src/db/migrations/{:04}_{}_{e}.ts",
            rng.range(0, 9999),
            rng.pick(&VERBS)
        ),
        45..47 => return (format!("src/db/generated/{e}.ts"), true),
        47..50 => format!("src/lib/{}.ts", rng.pick(&UTILS)),
        50..60 => format!(
            "src/features/{e}/{}/{big}{}.ts",
            rng.pick(&FEATURE_PARTS),
            rng.pick(&PARTS)
        ),
        60..64 => format!("src/features/{e}/server/actions/{big}Action.ts"),
        64..69 => format!(
            "src/integrations/{}/{}.ts",
            rng.pick(&SERVICES),
            rng.pick(&SERVICE_FILES)
        ),
        69..73 => format!("src/shared/utils/{}.ts", rng.pick(&UTILS)),
        73..76 => format!("src/types/{e}.ts"),
        _ => format!("src/{e}s/{e}{}.ts", rng.pick(&LOGIC)),
    };
    (path, false)
}

/// `path` with `n` added to its file stem (`a/b.ts`, 2 → `a/b2.ts`).
fn numbered(path: &str, n: u32) -> String {
    let file_start = path.rfind('/').map_or(0, |i| i + 1);
    match path[file_start..].rfind('.') {
        Some(dot) if dot > 0 => {
            let dot = file_start + dot;
            format!("{}{n}{}", &path[..dot], &path[dot..])
        }
        _ => format!("{path}{n}"),
    }
}

/// A synthetic shop project of exactly `files` files: sorted unique paths, links
/// sorted by `(src, dst)` with `src != dst`, about three per file, and the [`HUBS`]
/// imported by roughly a fifth of all files.
pub fn input(seed: u64, files: u32) -> Input {
    let mut rng = Rng(seed ^ 0x5348_4F50_5348_4F50);
    let mut paths: BTreeSet<String> = BTreeSet::new();
    let mut generated: BTreeSet<String> = BTreeSet::new();
    for hub in HUBS.iter().take(files as usize) {
        paths.insert(hub.to_string());
    }
    while paths.len() < files as usize {
        let (mut p, gen_file) = path(&mut rng);
        let base = p.clone();
        let mut n = 2;
        while paths.contains(&p) {
            p = numbered(&base, n);
            n += 1;
        }
        if gen_file {
            generated.insert(p.clone());
        }
        paths.insert(p);
    }
    let files: Vec<InFile> = paths
        .into_iter()
        .map(|path| InFile {
            loc: rng.range(5, 400) as u32,
            generated: generated.contains(&path),
            path,
        })
        .collect();

    let n = files.len() as u64;
    let hubs: Vec<u64> = HUBS
        .iter()
        .filter_map(|h| files.iter().position(|f| f.path == *h))
        .map(|i| i as u64)
        .collect();
    let mut pairs: BTreeSet<(u32, u32)> = BTreeSet::new();
    if n >= 2 {
        for src in 0..n {
            for _ in 0..rng.range(1, 4) {
                let dst = (src + rng.range(1, n - 1)) % n;
                pairs.insert((src as u32, dst as u32));
            }
            for &hub in &hubs {
                if hub != src && rng.chance(HUB_PERCENT) {
                    pairs.insert((src as u32, hub as u32));
                }
            }
        }
    }
    let links = pairs
        .into_iter()
        .map(|(src, dst)| InLink {
            src,
            dst,
            refs: rng.range(1, 5) as u32,
        })
        .collect();
    Input { files, links }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_adds_to_the_stem() {
        assert_eq!(numbered("src/a/b.ts", 2), "src/a/b2.ts");
        assert_eq!(numbered("src/a.b/c", 3), "src/a.b/c3");
        assert_eq!(numbered("x.tsx", 10), "x10.tsx");
    }
}
