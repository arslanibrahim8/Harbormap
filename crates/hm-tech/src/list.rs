//! The technologies list: what a project was built with, per manifest root.
//!
//! `build` merges declared dependencies, imports, languages, config files and Docker images
//! into one ranked list of items per root. It is pure: no I/O, no clock, and the result does
//! not depend on the order of the input vectors.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::catalog::Catalog;
use crate::input::{DeclaredDep, TechInput};
use crate::model::{Category, Ecosystem, Level, Tech};
use crate::names::{is_builtin, is_package_specifier, normalize, normalize_image};

/// How many items the beginner level shows before folding the rest into "more".
pub const BEGINNER_LIMIT: usize = 12;

/// The technologies of a whole repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TechList {
    /// Sorted by root path; roots with no items are left out.
    pub roots: Vec<RootList>,
}

/// The technologies of one manifest root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootList {
    /// Repository-relative path, `""` for the repository root.
    pub root: String,
    /// In rank order.
    pub items: Vec<TechItem>,
}

/// One technology, or one unknown package, language or image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TechItem {
    /// The catalog id; `None` for things the catalog does not know.
    pub tech: Option<String>,
    pub name: String,
    pub category: Category,
    /// The larger of the number of importing files and the number of files in the language.
    pub score: u32,
    pub evidence: Vec<Evidence>,
}

/// One reason an item is on the list. The declaration order is the order inside an item.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    Language {
        files: u32,
    },
    Imported {
        files: u32,
    },
    /// Imported, but no manifest declares it.
    NotDeclared {
        packages: Vec<String>,
    },
    /// Declared in a measured ecosystem, but nothing imports it.
    NotImported,
    Declared {
        file: String,
        line: u32,
        name: String,
        declared: Option<String>,
        resolved: Option<String>,
        dev: bool,
    },
    Config {
        path: String,
    },
    Image {
        image: String,
        file: String,
        line: u32,
    },
}

/// The items of a root as one level shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shown<'a> {
    /// Non-empty groups in category order, items in rank order.
    pub groups: Vec<(Category, Vec<&'a TechItem>)>,
    /// Items left out.
    pub more: usize,
}

impl RootList {
    /// Builder shows everything. Beginner shows at most [`BEGINNER_LIMIT`] items, catalog
    /// technologies first, then unknown items while there is room.
    pub fn shown(&self, level: Level) -> Shown<'_> {
        let mut chosen = vec![level == Level::Builder; self.items.len()];
        if level == Level::Beginner {
            let mut room = BEGINNER_LIMIT;
            for known in [true, false] {
                for (index, item) in self.items.iter().enumerate() {
                    if room > 0 && item.tech.is_some() == known {
                        chosen[index] = true;
                        room -= 1;
                    }
                }
            }
        }
        let mut groups: BTreeMap<Category, Vec<&TechItem>> = BTreeMap::new();
        let mut shown = 0;
        for (item, _) in self
            .items
            .iter()
            .zip(&chosen)
            .filter(|(_, chosen)| **chosen)
        {
            groups.entry(item.category).or_default().push(item);
            shown += 1;
        }
        Shown {
            groups: groups.into_iter().collect(),
            more: self.items.len() - shown,
        }
    }
}

/// Builds the list: one item per technology (or unknown thing) per root.
pub fn build(catalog: &Catalog, input: &TechInput) -> TechList {
    Builder::new(catalog, input).run()
}

/// What identifies an item: its key inside a root and how it is shown.
#[derive(Debug, Clone)]
struct Ident {
    key: String,
    tech: Option<(String, String)>,
    category: Category,
    /// The name of an unknown item.
    name: String,
}

impl Ident {
    fn known(tech: &Tech) -> Ident {
        Ident {
            key: format!("tech:{}", tech.id),
            tech: Some((tech.id.clone(), tech.name.clone())),
            category: tech.category,
            name: tech.name.clone(),
        }
    }

    fn unknown(key: String, category: Category, name: &str) -> Ident {
        Ident {
            key,
            tech: None,
            category,
            name: name.to_string(),
        }
    }
}

/// An item with its rank key: score (descending), dev-only, lowercase name.
type Ranked = (Reverse<u32>, bool, String, TechItem);

/// Everything collected for one item.
#[derive(Debug)]
struct Acc {
    tech: Option<(String, String)>,
    category: Category,
    /// Candidate names of an unknown item; declared names come first, then the smallest.
    names: BTreeSet<(bool, String)>,
    importers: BTreeSet<String>,
    language_files: u32,
    declared: BTreeSet<Evidence>,
    declared_dev: usize,
    declared_measured: bool,
    not_declared: BTreeSet<String>,
    configs: BTreeSet<String>,
    images: BTreeSet<Evidence>,
}

struct Builder<'a> {
    catalog: &'a Catalog,
    input: &'a TechInput,
    roots: BTreeSet<&'a str>,
    own: BTreeSet<(Ecosystem, String)>,
    items: BTreeMap<(String, String), Acc>,
    /// The declared packages of a root and ecosystem, by normalised name.
    index: BTreeMap<(&'a str, Ecosystem), BTreeMap<String, Ident>>,
}

impl<'a> Builder<'a> {
    fn new(catalog: &'a Catalog, input: &'a TechInput) -> Self {
        let mut roots: BTreeSet<&str> = input.roots.iter().map(String::as_str).collect();
        roots.extend(input.declared.iter().map(|dep| dep.root.as_str()));
        roots.insert("");
        let own = input
            .own_packages
            .iter()
            .map(|package| {
                (
                    package.ecosystem,
                    normalize(package.ecosystem, &package.name),
                )
            })
            .collect();
        Builder {
            catalog,
            input,
            roots,
            own,
            items: BTreeMap::new(),
            index: BTreeMap::new(),
        }
    }

    fn run(mut self) -> TechList {
        self.add_declared();
        self.add_imports();
        self.add_languages();
        self.add_configs();
        self.add_images();
        self.finish()
    }

    /// The roots that contain a path, deepest first.
    fn roots_of(&self, path: &str) -> Vec<&'a str> {
        let mut found: Vec<&str> = self
            .roots
            .iter()
            .copied()
            .filter(|root| {
                root.is_empty()
                    || path == *root
                    || path
                        .strip_prefix(root)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .collect();
        found.sort_by_key(|root| Reverse(root.len()));
        found
    }

    fn root_of(&self, path: &str) -> &'a str {
        self.roots_of(path).first().copied().unwrap_or("")
    }

    fn package_ident(&self, ecosystem: Ecosystem, name: &str) -> Ident {
        match self.catalog.package(ecosystem, name) {
            Some(tech) => Ident::known(tech),
            None => Ident::unknown(
                format!("{}:{}", ecosystem.as_str(), normalize(ecosystem, name)),
                Category::Other,
                name,
            ),
        }
    }

    fn is_own(&self, ecosystem: Ecosystem, name: &str, with_subpaths: bool) -> bool {
        self.own.iter().any(|(own_ecosystem, own)| {
            *own_ecosystem == ecosystem
                && (own == name
                    || (with_subpaths
                        && name
                            .strip_prefix(own.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))))
        })
    }

    fn acc(&mut self, root: &str, ident: &Ident, declared_name: bool) -> &mut Acc {
        let acc = self
            .items
            .entry((root.to_string(), ident.key.clone()))
            .or_insert_with(|| Acc {
                tech: ident.tech.clone(),
                category: ident.category,
                names: BTreeSet::new(),
                importers: BTreeSet::new(),
                language_files: 0,
                declared: BTreeSet::new(),
                declared_dev: 0,
                declared_measured: false,
                not_declared: BTreeSet::new(),
                configs: BTreeSet::new(),
                images: BTreeSet::new(),
            });
        acc.names.insert((!declared_name, ident.name.clone()));
        acc
    }

    fn add_declared(&mut self) {
        let input = self.input;
        for dep in &input.declared {
            if self.is_own(dep.ecosystem, &normalize(dep.ecosystem, &dep.name), false) {
                continue;
            }
            let mut ident = self.package_ident(dep.ecosystem, &dep.name);
            ident.name = dep.name.clone();
            let measured = input.import_coverage.contains(&dep.ecosystem);
            let acc = self.acc(&dep.root, &ident, true);
            acc.declared.insert(declared_evidence(dep));
            acc.declared_dev += usize::from(dep.dev);
            acc.declared_measured |= measured;
            self.index
                .entry((dep.root.as_str(), dep.ecosystem))
                .or_default()
                .insert(normalize(dep.ecosystem, &dep.name), ident);
        }
    }

    fn add_imports(&mut self) {
        let input = self.input;
        for import in &input.imports {
            let ecosystem = import.ecosystem;
            let package = match ecosystem {
                Ecosystem::Npm => npm_package_name(&import.package),
                _ => import.package.as_str(),
            };
            if is_builtin(ecosystem, package) || !is_package_specifier(ecosystem, package) {
                continue;
            }
            let name = normalize(ecosystem, package);
            if self.is_own(ecosystem, &name, ecosystem == Ecosystem::Go) {
                continue;
            }
            let roots = self.roots_of(&import.importer);
            let declared = roots.iter().find_map(|root| {
                let declared = self.index.get(&(*root, ecosystem))?;
                let ident = if ecosystem == Ecosystem::Go {
                    segment_prefixes(&name).find_map(|prefix| declared.get(prefix))
                } else {
                    declared.get(&name)
                }?;
                Some((*root, ident.clone()))
            });
            match declared {
                Some((root, ident)) => {
                    self.acc(root, &ident, true)
                        .importers
                        .insert(import.importer.clone());
                }
                None => {
                    let root = roots.first().copied().unwrap_or("");
                    let ident = self.package_ident(ecosystem, package);
                    let acc = self.acc(root, &ident, false);
                    acc.importers.insert(import.importer.clone());
                    acc.not_declared.insert(package.to_string());
                }
            }
        }
    }

    fn add_languages(&mut self) {
        let input = self.input;
        for count in &input.languages {
            let ident = match self.catalog.language(&count.language) {
                Some(tech) => Ident::known(tech),
                None => Ident::unknown(
                    format!("language:{}", count.language.to_lowercase()),
                    Category::Language,
                    &count.language,
                ),
            };
            let root = self.root_of(&count.path);
            self.acc(root, &ident, false).language_files += count.files;
        }
    }

    fn add_configs(&mut self) {
        let input = self.input;
        for path in &input.configs {
            if let Some(tech) = self.catalog.config_file(path) {
                let root = self.root_of(path);
                self.acc(root, &Ident::known(tech), false)
                    .configs
                    .insert(path.clone());
            }
        }
    }

    fn add_images(&mut self) {
        let input = self.input;
        for image in &input.images {
            let name = normalize_image(&image.image);
            if name == "scratch" || image.image.contains('$') {
                continue;
            }
            let ident = match self.catalog.image(&image.image) {
                Some(tech) => Ident::known(tech),
                None => Ident::unknown(format!("image:{name}"), Category::Other, &name),
            };
            let root = self.root_of(&image.file);
            self.acc(root, &ident, false)
                .images
                .insert(Evidence::Image {
                    image: image.image.clone(),
                    file: image.file.clone(),
                    line: image.line,
                });
        }
    }

    fn finish(self) -> TechList {
        let mut by_root: BTreeMap<String, Vec<Ranked>> = BTreeMap::new();
        for ((root, _), acc) in self.items {
            let item = acc.into_item();
            let dev_only = item
                .evidence
                .iter()
                .any(|evidence| matches!(evidence, Evidence::Declared { .. }))
                && item
                    .evidence
                    .iter()
                    .all(|evidence| !matches!(evidence, Evidence::Declared { dev: false, .. }));
            by_root.entry(root).or_default().push((
                Reverse(item.score),
                dev_only,
                item.name.to_lowercase(),
                item,
            ));
        }
        let roots = by_root
            .into_iter()
            .map(|(root, mut items)| {
                // Stable, so equal keys keep their key order from the map.
                items.sort_by(|a, b| {
                    (&a.0, a.1, &a.2, &a.3.name).cmp(&(&b.0, b.1, &b.2, &b.3.name))
                });
                RootList {
                    root,
                    items: items.into_iter().map(|(.., item)| item).collect(),
                }
            })
            .collect();
        TechList { roots }
    }
}

impl Acc {
    fn into_item(self) -> TechItem {
        let name = match &self.tech {
            Some((_, name)) => name.clone(),
            None => self
                .names
                .into_iter()
                .next()
                .map(|(_, name)| name)
                .unwrap_or_default(),
        };
        let importers = u32::try_from(self.importers.len()).unwrap_or(u32::MAX);
        let mut evidence = Vec::new();
        if self.language_files > 0 {
            evidence.push(Evidence::Language {
                files: self.language_files,
            });
        }
        if importers > 0 {
            evidence.push(Evidence::Imported { files: importers });
        } else if self.declared_measured {
            evidence.push(Evidence::NotImported);
        }
        if !self.not_declared.is_empty() {
            evidence.push(Evidence::NotDeclared {
                packages: self.not_declared.into_iter().collect(),
            });
        }
        evidence.extend(self.declared);
        evidence.extend(
            self.configs
                .into_iter()
                .map(|path| Evidence::Config { path }),
        );
        evidence.extend(self.images);
        evidence.sort();
        TechItem {
            tech: self.tech.map(|(id, _)| id),
            name,
            category: self.category,
            score: importers.max(self.language_files),
            evidence,
        }
    }
}

fn declared_evidence(dep: &DeclaredDep) -> Evidence {
    Evidence::Declared {
        file: dep.file.clone(),
        line: dep.line,
        name: dep.name.clone(),
        declared: dep.declared.clone(),
        resolved: dep.resolved.clone(),
        dev: dep.dev,
    }
}

/// The package an npm import specifier names: `lodash/fp` is `lodash`, `@a/b/c` is `@a/b`.
fn npm_package_name(specifier: &str) -> &str {
    let mut segments = specifier.split('/');
    let first = segments.next().unwrap_or("");
    if first.starts_with('@') {
        match segments.next() {
            Some(second) => &specifier[..first.len() + 1 + second.len()],
            None => first,
        }
    } else {
        first
    }
}

/// A path and each of its `/`-segment prefixes, longest first.
fn segment_prefixes(path: &str) -> impl Iterator<Item = &str> {
    std::iter::once(path).chain(path.rmatch_indices('/').map(|(slash, _)| &path[..slash]))
}
