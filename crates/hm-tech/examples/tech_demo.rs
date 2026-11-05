//! DEV DEMO ONLY. Reads package.json files directly to show the technologies list before L3 wires it; product manifest reading belongs to L2a.
//!
//! Usage: `cargo run -p hm-tech --example tech_demo -- [--builder] [--en] <package.json>...`

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use hm_tech::{
    Catalog, DeclaredDep, Ecosystem, Level, Locale, PackageRef, TechInput, build, render,
};

fn main() -> ExitCode {
    let mut level = Level::Beginner;
    let mut locale = Locale::Tr;
    let mut paths = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--builder" => level = Level::Builder,
            "--en" => locale = Locale::En,
            _ => paths.push(arg),
        }
    }
    if paths.is_empty() {
        eprintln!("usage: tech_demo [--builder] [--en] <package.json>...");
        return ExitCode::from(2);
    }

    let mut input = TechInput::default();
    for path in &paths {
        if let Err(message) = read_manifest(path, &mut input) {
            eprintln!("{path}: {message}");
            return ExitCode::from(1);
        }
    }

    let catalog = Catalog::embedded();
    for warning in catalog.warnings() {
        eprintln!("catalog warning: {warning}");
    }
    print!(
        "{}",
        render(&build(catalog, &input), catalog, level, locale)
    );
    ExitCode::SUCCESS
}

/// Adds what one package.json says (and the file names beside it) to `input`.
fn read_manifest(path: &str, input: &mut TechInput) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;

    let normalized = path.replace('\\', "/");
    let manifest = Path::new(&normalized);
    let root = match manifest.parent().and_then(Path::to_str) {
        None | Some("" | ".") => String::new(),
        Some(parent) => parent.to_owned(),
    };

    if !input.roots.contains(&root) {
        input.roots.push(root.clone());
    }
    if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
        input.own_packages.push(PackageRef {
            ecosystem: Ecosystem::Npm,
            name: name.to_owned(),
        });
    }

    for (key, dev) in [("dependencies", false), ("devDependencies", true)] {
        let Some(deps) = json.get(key).and_then(|v| v.as_object()) else {
            continue;
        };
        for (name, range) in deps {
            input.declared.push(DeclaredDep {
                ecosystem: Ecosystem::Npm,
                name: name.clone(),
                declared: range.as_str().map(str::to_owned),
                resolved: None,
                dev,
                file: normalized.clone(),
                line: line_of(&text, name),
                root: root.clone(),
            });
        }
    }

    let folder = if root.is_empty() {
        Path::new(".")
    } else {
        Path::new(&root)
    };
    let entries = fs::read_dir(folder).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        input.configs.push(if root.is_empty() {
            file_name
        } else {
            format!("{root}/{file_name}")
        });
    }
    Ok(())
}

/// The 1-based line of the first line mentioning `"<name>":`, else 1.
fn line_of(text: &str, name: &str) -> u32 {
    let needle = format!("\"{name}\":");
    text.lines()
        .position(|line| line.contains(&needle))
        .and_then(|i| u32::try_from(i + 1).ok())
        .unwrap_or(1)
}
