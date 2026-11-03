//! Reads one `package.json`: its name, `bin`, `main`, `scripts` and dependencies, each
//! with the line it is on. The file is read as JSON with comments and trailing commas,
//! as tools read it. A problem becomes a note and never an error.
//!
//! Fields: <https://docs.npmjs.com/cli/v11/configuring-npm/package-json> (`bin`, `main`).

use jsonc_parser::ast::{Object, Value};
use jsonc_parser::common::Ranged;

/// The sections of `package.json` that list dependencies.
const DEP_SECTIONS: &[&str] = &[
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

/// One declared dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    /// The `package.json` section it is in, such as `dependencies`.
    pub section: String,
    /// True for `devDependencies`.
    pub dev: bool,
    /// The range as written, such as `^15.0.0` or `workspace:*`.
    pub declared: String,
    /// The version the lockfile pins, when a lockfile is found and lists it.
    pub resolved: Option<String>,
    /// 1-based line of the dependency's name.
    pub line: u32,
}

/// What one `package.json` says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NpmManifest {
    /// Relative path of the file, such as `apps/web/package.json`.
    pub path: String,
    /// Its folder: `apps/web`, or `""` for the repository root.
    pub root: String,
    pub name: Option<String>,
    /// (command name, script path, line), sorted by name.
    pub bin: Vec<(String, String, u32)>,
    /// (path, line) of `main`.
    pub main: Option<(String, u32)>,
    /// (script name, command, line), sorted by name.
    pub scripts: Vec<(String, String, u32)>,
    /// Sorted by (section, name).
    pub deps: Vec<Dependency>,
    /// The lockfile that covers this manifest, set by `Manifests::load`.
    pub lockfile: Option<String>,
    /// What could not be read, one line each, each starting with the file's path.
    pub notes: Vec<String>,
}

/// The folder part of a relative path; `""` for a file at the root.
pub(crate) fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Byte offsets of line starts, to turn a byte offset into a 1-based line.
struct Lines(Vec<usize>);

impl Lines {
    fn new(text: &str) -> Lines {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Lines(starts)
    }

    fn line(&self, offset: usize) -> u32 {
        let n = self.0.partition_point(|&start| start <= offset);
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

struct Reader<'a> {
    rel: &'a str,
    lines: Lines,
    out: NpmManifest,
}

impl Reader<'_> {
    fn note(&mut self, what: String, offset: usize) {
        let line = self.lines.line(offset);
        self.out
            .notes
            .push(format!("{}: {what} (line {line})", self.rel));
    }

    /// A string property of `obj`, or a note when it has another type.
    fn string_of(&mut self, obj: &Object, key: &str, label: &str) -> Option<(String, u32)> {
        let prop = obj.get(key)?;
        if let Value::StringLit(s) = &prop.value {
            return Some((s.value.to_string(), self.lines.line(prop.name.start())));
        }
        self.note(format!("\"{label}\" is not a string"), prop.name.start());
        None
    }

    /// The `{name: string}` entries of the object property `key`, with the line of each
    /// name. A value of another type is a note.
    fn entries_of(&mut self, obj: &Object, key: &str) -> Vec<(String, String, u32)> {
        let Some(prop) = obj.get(key) else {
            return Vec::new();
        };
        let Value::Object(inner) = &prop.value else {
            self.note(format!("\"{key}\" is not an object"), prop.name.start());
            return Vec::new();
        };
        let mut entries = Vec::new();
        for item in &inner.properties {
            let name = item.name.as_str();
            match &item.value {
                Value::StringLit(s) => entries.push((
                    name.to_string(),
                    s.value.to_string(),
                    self.lines.line(item.name.start()),
                )),
                _ => self.note(
                    format!("\"{key}.{name}\" is not a string"),
                    item.name.start(),
                ),
            }
        }
        entries
    }

    fn read(&mut self, obj: &Object) {
        self.out.name = self.string_of(obj, "name", "name").map(|(n, _)| n);
        self.out.main = self.string_of(obj, "main", "main");
        self.read_bin(obj);
        self.out.scripts = self.entries_of(obj, "scripts");
        self.out.scripts.sort();
        for section in DEP_SECTIONS {
            for (name, declared, line) in self.entries_of(obj, section) {
                self.out.deps.push(Dependency {
                    name,
                    section: (*section).to_string(),
                    dev: *section == "devDependencies",
                    declared,
                    resolved: None,
                    line,
                });
            }
        }
        self.out
            .deps
            .sort_by(|a, b| (&a.section, &a.name, a.line).cmp(&(&b.section, &b.name, b.line)));
    }

    /// `bin` is a string (one command named after the package, without its scope) or a
    /// map of command to script path.
    fn read_bin(&mut self, obj: &Object) {
        let Some(prop) = obj.get("bin") else {
            return;
        };
        let at = prop.name.start();
        match &prop.value {
            Value::StringLit(s) => match self.out.name.as_deref() {
                Some(name) => {
                    let command = name.rsplit('/').next().unwrap_or(name).to_string();
                    let line = self.lines.line(at);
                    self.out.bin.push((command, s.value.to_string(), line));
                }
                None => self.note(
                    "\"bin\" is a string but the package has no \"name\"".into(),
                    at,
                ),
            },
            Value::Object(_) => self.out.bin = self.entries_of(obj, "bin"),
            _ => self.note("\"bin\" is not a string or an object".into(), at),
        }
        self.out.bin.sort();
    }
}

/// Reads `text` as the `package.json` at `rel`. It never fails: a file that is not valid
/// JSON, or whose top level is not an object, gives an empty manifest and one note.
pub fn parse_package_json(rel: &str, text: &str) -> NpmManifest {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = Reader {
        rel,
        lines: Lines::new(text),
        out: NpmManifest {
            path: rel.to_string(),
            root: dir_of(rel).to_string(),
            ..NpmManifest::default()
        },
    };
    match jsonc_parser::parse_to_ast(text, &Default::default(), &Default::default()) {
        Err(e) => {
            let line = e.line_display();
            reader
                .out
                .notes
                .push(format!("{rel}: not valid JSON (line {line})"));
        }
        Ok(parsed) => match &parsed.value {
            Some(Value::Object(obj)) => reader.read(obj),
            other => {
                let at = other.as_ref().map_or(0, |v| v.start());
                reader.note("not a JSON object".into(), at);
            }
        },
    }
    reader.out
}
