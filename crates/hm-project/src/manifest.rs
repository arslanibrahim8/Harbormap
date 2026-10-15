//! Reads project manifests near a working directory (read-only): the nearest
//! package.json and the nearest Python manifest give dependency names; the nearest
//! manifest of any kind gives the service name.

use std::collections::BTreeSet;
use std::path::Path;

use crate::paths::within;
use crate::world::World;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// Name from the nearest manifest, e.g. "web" for `{"name": "@shop/web"}`.
    pub name: Option<String>,
    /// Dependency names, lowercased; Python names use `-` instead of `_`.
    pub packages: BTreeSet<String>,
}

/// Looks in `dir` and its parents up to `stop` (inclusive). When `dir` is not inside
/// `stop`, only `dir` itself is read.
pub fn read_manifest(world: &dyn World, dir: &Path, stop: &Path) -> Manifest {
    let windows = world.windows();
    let mut manifest = Manifest::default();
    let mut found_node = false;
    let mut found_python = false;
    let mut current = Some(dir);
    while let Some(here) = current {
        let names = world.file_names(here);
        let has = |file: &str| names.iter().any(|name| name == file);

        if !found_node && has("package.json") {
            found_node = true;
            if let Some(text) = world.read_to_string(&here.join("package.json")) {
                read_package_json(&text, &mut manifest);
            }
        }
        if !found_python {
            if has("pyproject.toml")
                && let Some(text) = world.read_to_string(&here.join("pyproject.toml"))
            {
                found_python = true;
                read_pyproject(&text, &mut manifest);
            }
            for requirements in names
                .iter()
                .filter(|name| name.starts_with("requirements") && name.ends_with(".txt"))
            {
                found_python = true;
                if let Some(text) = world.read_to_string(&here.join(requirements)) {
                    read_requirements(&text, &mut manifest);
                }
            }
        }
        if manifest.name.is_none() {
            if let Some(project_file) = names
                .iter()
                .find(|name| name.ends_with(".csproj") || name.ends_with(".fsproj"))
            {
                manifest.name = project_file
                    .rsplit_once('.')
                    .map(|(stem, _)| stem.to_string());
            } else if has("go.mod")
                && let Some(text) = world.read_to_string(&here.join("go.mod"))
            {
                manifest.name = go_module_name(&text);
            }
        }

        let at_stop = within(stop, here, windows);
        if (found_node && found_python && manifest.name.is_some())
            || !within(here, stop, windows)
            || at_stop
        {
            break;
        }
        current = here.parent();
    }
    manifest
}

fn read_package_json(text: &str, manifest: &mut Manifest) {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    if manifest.name.is_none() {
        manifest.name = json
            .get("name")
            .and_then(|name| name.as_str())
            .map(|name| name.rsplit('/').next().unwrap_or(name).to_string())
            .filter(|name| !name.is_empty());
    }
    for section in ["dependencies", "devDependencies"] {
        if let Some(deps) = json.get(section).and_then(|deps| deps.as_object()) {
            manifest
                .packages
                .extend(deps.keys().map(|name| name.to_lowercase()));
        }
    }
}

fn read_pyproject(text: &str, manifest: &mut Manifest) {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return;
    };
    let project = table.get("project");
    let poetry = table.get("tool").and_then(|tool| tool.get("poetry"));
    if manifest.name.is_none() {
        manifest.name = project
            .or(poetry)
            .and_then(|section| section.get("name"))
            .and_then(|name| name.as_str())
            .map(str::to_string);
    }
    if let Some(deps) = project
        .and_then(|project| project.get("dependencies"))
        .and_then(|deps| deps.as_array())
    {
        manifest.packages.extend(
            deps.iter()
                .filter_map(|dep| dep.as_str())
                .filter_map(python_requirement_name),
        );
    }
    if let Some(deps) = poetry
        .and_then(|poetry| poetry.get("dependencies"))
        .and_then(|deps| deps.as_table())
    {
        manifest.packages.extend(
            deps.keys()
                .filter(|name| name.as_str() != "python")
                .map(|name| normalize_python(name)),
        );
    }
}

fn read_requirements(text: &str, manifest: &mut Manifest) {
    manifest.packages.extend(
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('-'))
            .filter_map(python_requirement_name),
    );
}

/// `"fastapi[standard]>=0.115"` → `fastapi`.
fn python_requirement_name(requirement: &str) -> Option<String> {
    let name: String = requirement
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    (!name.is_empty()).then(|| normalize_python(&name))
}

fn normalize_python(name: &str) -> String {
    name.to_lowercase().replace(['_', '.'], "-")
}

/// `module github.com/acme/api` → `api`.
fn go_module_name(text: &str) -> Option<String> {
    let module = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("module "))?
        .trim()
        .trim_matches('"');
    module.rsplit('/').next().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::FixtureWorld;
    use std::path::PathBuf;

    fn world(files: &[(&str, &str)]) -> FixtureWorld {
        FixtureWorld {
            files: files
                .iter()
                .map(|(path, text)| (PathBuf::from(path), text.to_string()))
                .collect(),
            ..FixtureWorld::default()
        }
    }

    #[test]
    fn nearest_package_json_gives_name_and_dependencies() {
        let world = world(&[
            (
                "/w/shop/package.json",
                r#"{"name":"shop","devDependencies":{"turbo":"2"}}"#,
            ),
            (
                "/w/shop/apps/web/package.json",
                r#"{"name":"@shop/web","dependencies":{"vite":"6","React":"19"}}"#,
            ),
        ]);
        let manifest = read_manifest(&world, Path::new("/w/shop/apps/web"), Path::new("/w/shop"));
        assert_eq!(manifest.name.as_deref(), Some("web"));
        assert_eq!(
            manifest.packages.into_iter().collect::<Vec<_>>(),
            vec!["react", "vite"]
        );
    }

    #[test]
    fn python_manifests_are_normalized() {
        let world = world(&[
            (
                "/w/api/pyproject.toml",
                "[project]\nname = \"shop-api\"\ndependencies = [\"fastapi[standard]>=0.115\", \"SQLAlchemy==2.0\"]\n",
            ),
            (
                "/w/api/requirements-dev.txt",
                "# tools\npytest_asyncio==0.24\n-r requirements.txt\n",
            ),
        ]);
        let manifest = read_manifest(&world, Path::new("/w/api"), Path::new("/w/api"));
        assert_eq!(manifest.name.as_deref(), Some("shop-api"));
        assert!(manifest.packages.contains("fastapi"));
        assert!(manifest.packages.contains("sqlalchemy"));
        assert!(manifest.packages.contains("pytest-asyncio"));
    }

    #[test]
    fn dotnet_and_go_projects_are_named_after_their_project_files() {
        let world = world(&[
            (
                "/w/api/Shop.Api.csproj",
                "<Project Sdk=\"Microsoft.NET.Sdk.Web\"/>",
            ),
            (
                "/w/svc/go.mod",
                "module github.com/acme/billing\n\ngo 1.23\n",
            ),
        ]);
        let api = read_manifest(&world, Path::new("/w/api"), Path::new("/w"));
        assert_eq!(api.name.as_deref(), Some("Shop.Api"));
        let svc = read_manifest(&world, Path::new("/w/svc"), Path::new("/w"));
        assert_eq!(svc.name.as_deref(), Some("billing"));
    }

    #[test]
    fn stops_at_the_boundary() {
        let world = world(&[("/w/package.json", r#"{"name":"outside"}"#)]);
        let manifest = read_manifest(&world, Path::new("/w/shop/api"), Path::new("/w/shop"));
        assert_eq!(manifest, Manifest::default());
    }
}
