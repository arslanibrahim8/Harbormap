//! The technologies catalog: packs compiled into the binary, validated on load and indexed
//! for lookups. A broken entry is skipped with a warning; it never stops Harbormap.
//!
//! A pack file holds any of three tables:
//!
//! - `[[tech]]`: `id`, `name`, `category`, texts (`en.beginner`, `en.builder`, `tr.beginner`,
//!   `tr.builder`), package keys per ecosystem (`npm`, `pypi`, `go`, `maven`, `nuget`,
//!   `crates`; a key ending in `*` is a prefix pattern) and `languages`.
//! - `[[config]]`: `tech` and `files`, file-name patterns (no `/`, `*` matches any run).
//! - `[[image]]`: `tech` and `names`, Docker image names without tag or registry default.
//!
//! Any other top-level key is a warning.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::OnceLock;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::model::{Ecosystem, Tech};
use crate::names::{is_package_specifier, normalize, normalize_image};

/// The pack files compiled into the binary, in load order.
pub const PACK_FILES: &[(&str, &str)] = &[
    (
        "tech/languages.toml",
        include_str!("../../../packs/tech/languages.toml"),
    ),
    (
        "tech/shared.toml",
        include_str!("../../../packs/tech/shared.toml"),
    ),
    (
        "tech/npm-web.toml",
        include_str!("../../../packs/tech/npm-web.toml"),
    ),
    (
        "tech/npm-data.toml",
        include_str!("../../../packs/tech/npm-data.toml"),
    ),
    (
        "tech/npm-tooling.toml",
        include_str!("../../../packs/tech/npm-tooling.toml"),
    ),
    (
        "tech/npm-other.toml",
        include_str!("../../../packs/tech/npm-other.toml"),
    ),
    (
        "tech/pypi.toml",
        include_str!("../../../packs/tech/pypi.toml"),
    ),
    ("tech/go.toml", include_str!("../../../packs/tech/go.toml")),
    (
        "tech/maven.toml",
        include_str!("../../../packs/tech/maven.toml"),
    ),
    (
        "tech/nuget.toml",
        include_str!("../../../packs/tech/nuget.toml"),
    ),
    (
        "tech/crates.toml",
        include_str!("../../../packs/tech/crates.toml"),
    ),
    (
        "tech/files.toml",
        include_str!("../../../packs/tech/files.toml"),
    ),
];

const BEGINNER_EN_WORDS: usize = 18;
const BEGINNER_TR_WORDS: usize = 15;
const BUILDER_WORDS: usize = 30;

/// A `[[config]]` entry before its tech id is resolved.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigEntry {
    tech: String,
    files: Vec<String>,
}

/// An `[[image]]` entry before its tech id is resolved.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageEntry {
    tech: String,
    names: Vec<String>,
}

/// An entry that names a tech, kept until every file is loaded (it may name a tech from a
/// later file).
struct Pending<T> {
    file: String,
    number: usize,
    entry: T,
}

/// The package keys of one ecosystem, by normalised key.
#[derive(Debug, Default)]
struct Keys {
    exact: HashMap<String, usize>,
    /// Pattern prefixes (the key without its trailing `*`).
    prefixes: HashMap<String, usize>,
}

impl Keys {
    fn contains(&self, normalized: &str) -> bool {
        match normalized.strip_suffix('*') {
            Some(prefix) => self.prefixes.contains_key(prefix),
            None => self.exact.contains_key(normalized),
        }
    }

    fn insert(&mut self, normalized: &str, tech: usize) {
        match normalized.strip_suffix('*') {
            Some(prefix) => self.prefixes.insert(prefix.to_string(), tech),
            None => self.exact.insert(normalized.to_string(), tech),
        };
    }

    /// The longest pattern prefix that starts `name`.
    fn longest_prefix(&self, name: &str) -> Option<(usize, usize)> {
        self.prefixes
            .iter()
            .filter(|(prefix, _)| name.starts_with(prefix.as_str()))
            .map(|(prefix, tech)| (prefix.len(), *tech))
            .max_by_key(|(len, _)| *len)
    }

    /// The longest exact key that is a whole `/`-segment prefix of `name`.
    fn longest_segment_key(&self, name: &str) -> Option<(usize, usize)> {
        name.match_indices('/')
            .filter_map(|(slash, _)| self.exact.get(&name[..slash]).map(|tech| (slash, *tech)))
            .next_back()
    }
}

/// The loaded catalog: techs in file order, with indexes for the lookups.
#[derive(Debug, Default)]
pub struct Catalog {
    techs: Vec<Tech>,
    by_id: HashMap<String, usize>,
    packages: [Keys; 6],
    configs: Vec<(String, usize)>,
    images: HashMap<String, usize>,
    languages: HashMap<String, usize>,
    warnings: Vec<String>,
}

impl Catalog {
    /// The catalog compiled into the binary, loaded once.
    pub fn embedded() -> &'static Catalog {
        static EMBEDDED: OnceLock<Catalog> = OnceLock::new();
        EMBEDDED.get_or_init(|| Catalog::load(PACK_FILES))
    }

    /// Loads pack files entry by entry, so one bad entry does not hide the rest. Never panics.
    pub fn load(files: &[(&str, &str)]) -> Catalog {
        let mut catalog = Catalog::default();
        let mut configs = Vec::new();
        let mut images = Vec::new();
        for (file, text) in files {
            catalog.load_file(file, text, &mut configs, &mut images);
        }
        for pending in configs {
            catalog.add_config(pending);
        }
        for pending in images {
            catalog.add_image(pending);
        }
        catalog.warn_about_unmatched();
        catalog
    }

    /// What was skipped or dropped while loading, one line each.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// A technology by id.
    pub fn tech(&self, id: &str) -> Option<&Tech> {
        self.by_id.get(id).map(|&index| &self.techs[index])
    }

    /// All technologies, in file order.
    pub fn iter(&self) -> impl Iterator<Item = &Tech> {
        self.techs.iter()
    }

    /// The technology a package belongs to: an exact key, else the longest `prefix*`
    /// pattern. For Go a key also matches the packages below it, so `github.com/jackc/pgx`
    /// covers `github.com/jackc/pgx/v5/pgxpool`; that key beats a shorter pattern.
    pub fn package(&self, ecosystem: Ecosystem, name: &str) -> Option<&Tech> {
        if !is_package_specifier(ecosystem, name) {
            return None;
        }
        let name = normalize(ecosystem, name);
        let keys = &self.packages[ecosystem as usize];
        if let Some(&index) = keys.exact.get(&name) {
            return Some(&self.techs[index]);
        }
        let pattern = keys.longest_prefix(&name);
        let segment = if ecosystem == Ecosystem::Go {
            keys.longest_segment_key(&name)
        } else {
            None
        };
        let best = match (segment, pattern) {
            (Some(segment), Some(pattern)) => Some(if segment.0 >= pattern.0 {
                segment
            } else {
                pattern
            }),
            (segment, pattern) => segment.or(pattern),
        };
        best.map(|(_, index)| &self.techs[index])
    }

    /// The technology a config file belongs to, by its last path segment. The most specific
    /// pattern (most characters besides `*`) wins; ties go to the earlier one.
    pub fn config_file(&self, rel_path: &str) -> Option<&Tech> {
        let file_name = rel_path.rsplit('/').next().unwrap_or(rel_path);
        let mut best: Option<(usize, usize)> = None;
        for (pattern, index) in &self.configs {
            let specificity = pattern.chars().filter(|&c| c != '*').count();
            if best.is_none_or(|(top, _)| specificity > top) && glob_match(pattern, file_name) {
                best = Some((specificity, *index));
            }
        }
        best.map(|(_, index)| &self.techs[index])
    }

    /// The technology a Docker image belongs to, ignoring tag, digest and default registry.
    pub fn image(&self, image: &str) -> Option<&Tech> {
        self.images
            .get(&normalize_image(image))
            .map(|&index| &self.techs[index])
    }

    /// The technology of a source language name (lowercase).
    pub fn language(&self, name: &str) -> Option<&Tech> {
        self.languages
            .get(&name.to_lowercase())
            .map(|&index| &self.techs[index])
    }

    /// How many package keys (exact names and patterns) the catalog has for an ecosystem.
    pub fn key_count(&self, ecosystem: Ecosystem) -> usize {
        let keys = &self.packages[ecosystem as usize];
        keys.exact.len() + keys.prefixes.len()
    }

    fn load_file(
        &mut self,
        file: &str,
        text: &str,
        configs: &mut Vec<Pending<ConfigEntry>>,
        images: &mut Vec<Pending<ImageEntry>>,
    ) {
        let table: toml::Table = match toml::from_str(text) {
            Ok(table) => table,
            Err(error) => {
                self.warnings
                    .push(format!("{file}: {}", error.to_string().trim()));
                return;
            }
        };
        for (key, value) in &table {
            match key.as_str() {
                "tech" | "config" | "image" => {
                    let Some(entries) = value.as_array() else {
                        self.warnings
                            .push(format!("{file}: `{key}` must be an array of tables"));
                        continue;
                    };
                    for (index, entry) in entries.iter().enumerate() {
                        let number = index + 1;
                        match key.as_str() {
                            "tech" => self.add_tech(file, number, entry),
                            "config" => self.collect(file, number, entry, "config", configs),
                            _ => self.collect(file, number, entry, "image", images),
                        }
                    }
                }
                other => self
                    .warnings
                    .push(format!("{file}: unknown top-level key `{other}`")),
            }
        }
    }

    /// Parses a `[[config]]` or `[[image]]` entry and keeps it for the end of loading.
    fn collect<T: DeserializeOwned>(
        &mut self,
        file: &str,
        number: usize,
        entry: &toml::Value,
        table: &str,
        pending: &mut Vec<Pending<T>>,
    ) {
        match parse_entry::<T>(entry) {
            Ok(entry) => pending.push(Pending {
                file: file.to_string(),
                number,
                entry,
            }),
            Err(problem) => self
                .warnings
                .push(format!("{file}: {table} #{number} skipped: {problem}")),
        }
    }

    fn add_tech(&mut self, file: &str, number: usize, entry: &toml::Value) {
        let label = entry.get("id").and_then(toml::Value::as_str).unwrap_or("?");
        let parsed = parse_entry::<Tech>(entry).and_then(|tech| self.check(&tech).map(|()| tech));
        match parsed {
            Ok(tech) => self.register(file, number, tech),
            Err(problem) => self.warnings.push(format!(
                "{file}: tech #{number} ({label}) skipped: {problem}"
            )),
        }
    }

    /// Everything that makes a whole tech entry unusable.
    fn check(&self, tech: &Tech) -> Result<(), String> {
        let id_ok = !tech.id.is_empty()
            && tech
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err("id must use lowercase letters, digits and dashes".into());
        }
        if self.by_id.contains_key(&tech.id) {
            return Err("id is already used by another tech".into());
        }
        if tech.name.trim().is_empty() {
            return Err("name is empty".into());
        }
        check_line("en.beginner", &tech.en.beginner, BEGINNER_EN_WORDS)?;
        check_line("en.builder", &tech.en.builder, BUILDER_WORDS)?;
        check_line("tr.beginner", &tech.tr.beginner, BEGINNER_TR_WORDS)?;
        check_line("tr.builder", &tech.tr.builder, BUILDER_WORDS)?;
        if tech.tr.beginner == tech.en.beginner {
            return Err("tr.beginner is the same as en.beginner".into());
        }
        if tech.tr.builder == tech.en.builder {
            return Err("tr.builder is the same as en.builder".into());
        }
        for ecosystem in Ecosystem::ALL {
            for key in tech.keys(ecosystem) {
                check_key(ecosystem, key)?;
            }
        }
        for language in &tech.languages {
            if language.is_empty() || *language != language.to_lowercase() {
                return Err(format!("language `{language}` must be lowercase"));
            }
        }
        Ok(())
    }

    /// Adds a valid tech to the indexes. A key or language that is already taken is dropped
    /// from the tech, with a warning.
    fn register(&mut self, file: &str, number: usize, mut tech: Tech) {
        let index = self.techs.len();
        let id = tech.id.clone();
        for ecosystem in Ecosystem::ALL {
            let keys = &mut self.packages[ecosystem as usize];
            let list = key_list(&mut tech, ecosystem);
            let mut kept = Vec::with_capacity(list.len());
            for key in list.drain(..) {
                let normalized = normalize(ecosystem, &key);
                if keys.contains(&normalized) {
                    self.warnings.push(format!(
                        "{file}: tech #{number} ({id}): duplicate {} key `{key}` dropped",
                        ecosystem.as_str()
                    ));
                } else {
                    keys.insert(&normalized, index);
                    kept.push(key);
                }
            }
            *list = kept;
        }
        let mut kept = Vec::with_capacity(tech.languages.len());
        for language in tech.languages.drain(..) {
            match self.languages.entry(language.clone()) {
                Entry::Occupied(_) => self.warnings.push(format!(
                    "{file}: tech #{number} ({id}): duplicate language `{language}` dropped"
                )),
                Entry::Vacant(slot) => {
                    slot.insert(index);
                    kept.push(language);
                }
            }
        }
        tech.languages = kept;
        self.by_id.insert(id, index);
        self.techs.push(tech);
    }

    fn add_config(&mut self, pending: Pending<ConfigEntry>) {
        let Pending {
            file,
            number,
            entry,
        } = pending;
        let skipped = |problem: String| format!("{file}: config #{number} skipped: {problem}");
        let Some(&index) = self.by_id.get(&entry.tech) else {
            self.warnings
                .push(skipped(format!("unknown tech id {}", entry.tech)));
            return;
        };
        if entry.files.is_empty() {
            self.warnings.push(skipped("no files listed".into()));
            return;
        }
        if let Some(bad) = entry
            .files
            .iter()
            .find(|pattern| pattern.is_empty() || pattern.contains('/'))
        {
            self.warnings.push(skipped(format!(
                "pattern `{bad}` must be a non-empty file name without `/`"
            )));
            return;
        }
        for pattern in entry.files {
            if self.configs.iter().any(|(known, _)| *known == pattern) {
                self.warnings.push(format!(
                    "{file}: config #{number}: duplicate pattern `{pattern}` dropped"
                ));
            } else {
                self.configs.push((pattern, index));
            }
        }
    }

    fn add_image(&mut self, pending: Pending<ImageEntry>) {
        let Pending {
            file,
            number,
            entry,
        } = pending;
        let skipped = |problem: String| format!("{file}: image #{number} skipped: {problem}");
        let Some(&index) = self.by_id.get(&entry.tech) else {
            self.warnings
                .push(skipped(format!("unknown tech id {}", entry.tech)));
            return;
        };
        if entry.names.is_empty() {
            self.warnings.push(skipped("no names listed".into()));
            return;
        }
        let names: Vec<String> = entry
            .names
            .iter()
            .map(|name| normalize_image(name))
            .collect();
        if names.iter().any(String::is_empty) {
            self.warnings.push(skipped("an image name is empty".into()));
            return;
        }
        for name in names {
            match self.images.entry(name) {
                Entry::Occupied(taken) => self.warnings.push(format!(
                    "{file}: image #{number}: duplicate image `{}` dropped",
                    taken.key()
                )),
                Entry::Vacant(slot) => {
                    slot.insert(index);
                }
            }
        }
    }

    fn warn_about_unmatched(&mut self) {
        let mut matched = vec![false; self.techs.len()];
        for (_, index) in &self.configs {
            matched[*index] = true;
        }
        for index in self.images.values() {
            matched[*index] = true;
        }
        for (index, tech) in self.techs.iter().enumerate() {
            let has_key = Ecosystem::ALL
                .iter()
                .any(|&ecosystem| !tech.keys(ecosystem).is_empty());
            if !(matched[index] || has_key || !tech.languages.is_empty()) {
                self.warnings
                    .push(format!("tech {} has nothing that matches it", tech.id));
            }
        }
    }
}

fn parse_entry<T: DeserializeOwned>(entry: &toml::Value) -> Result<T, String> {
    entry
        .clone()
        .try_into::<T>()
        .map_err(|error| error.to_string().trim().to_string())
}

fn key_list(tech: &mut Tech, ecosystem: Ecosystem) -> &mut Vec<String> {
    match ecosystem {
        Ecosystem::Npm => &mut tech.npm,
        Ecosystem::Pypi => &mut tech.pypi,
        Ecosystem::Go => &mut tech.go,
        Ecosystem::Maven => &mut tech.maven,
        Ecosystem::Nuget => &mut tech.nuget,
        Ecosystem::Crates => &mut tech.crates,
    }
}

/// One sentence, one line, ends with a period, within the word limit.
fn check_line(label: &str, line: &str, max_words: usize) -> Result<(), String> {
    if line.trim().is_empty() {
        return Err(format!("{label} is empty"));
    }
    if line.contains(['\n', '\r']) {
        return Err(format!("{label} must be one line"));
    }
    if !line.ends_with('.') {
        return Err(format!("{label} must end with a period"));
    }
    let words = line.split_whitespace().count();
    if words > max_words {
        return Err(format!("{label} has {words} words (limit {max_words})"));
    }
    Ok(())
}

/// A key may use `*` only as its last character, after a non-empty prefix.
fn check_key(ecosystem: Ecosystem, key: &str) -> Result<(), String> {
    let name = ecosystem.as_str();
    if key.trim().is_empty() {
        return Err(format!("{name} key is empty"));
    }
    let stars = key.matches('*').count();
    if stars > 0 && !(stars == 1 && key.ends_with('*')) {
        return Err(format!(
            "{name} key `{key}` may use `*` only as its last character"
        ));
    }
    if key == "*" {
        return Err(format!("{name} key `*` needs a prefix"));
    }
    Ok(())
}

/// Matches `text` against a pattern where `*` stands for any run of characters.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    // Where the last `*` was, and the text position it currently covers up to.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((star_p, star_t)) = star {
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}
