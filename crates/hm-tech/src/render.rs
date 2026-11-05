//! Plain-text rendering of the technologies list, in English and Turkish.
//!
//! The output is what the dev demo and `harbormap tech` print; it ends with a newline.

use std::fmt::Write;

use crate::catalog::Catalog;
use crate::list::{Evidence, RootList, TechItem, TechList};
use crate::model::{Category, Level, Locale};

/// Renders the list: one block per root, separated by a blank line. Roots get their path in
/// the title only when there is more than one.
pub fn render(list: &TechList, catalog: &Catalog, level: Level, locale: Locale) -> String {
    let title = match locale {
        Locale::En => "Technologies",
        Locale::Tr => "Teknolojiler",
    };
    if list.roots.is_empty() {
        return format!("{title}\n");
    }
    let with_roots = list.roots.len() > 1;
    let blocks: Vec<String> = list
        .roots
        .iter()
        .map(|root| render_root(root, catalog, level, locale, title, with_roots))
        .collect();
    blocks.join("\n")
}

/// The label of a category group.
pub fn category_label(category: Category, locale: Locale) -> &'static str {
    match (category, locale) {
        (Category::Language, Locale::En) => "Language",
        (Category::Language, Locale::Tr) => "Dil",
        (Category::Framework, Locale::En) => "Framework",
        (Category::Framework, Locale::Tr) => "Çatı (framework)",
        (Category::Ui, Locale::En) => "UI",
        (Category::Ui, Locale::Tr) => "Arayüz",
        (Category::Data, Locale::En) => "Data and ORM",
        (Category::Data, Locale::Tr) => "Veri ve ORM",
        (Category::Auth, Locale::En) => "Auth and security",
        (Category::Auth, Locale::Tr) => "Kimlik ve güvenlik",
        (Category::Validation, Locale::En) => "Validation",
        (Category::Validation, Locale::Tr) => "Doğrulama",
        (Category::Testing, Locale::En) => "Testing",
        (Category::Testing, Locale::Tr) => "Test",
        (Category::Build, Locale::En) => "Build and tooling",
        (Category::Build, Locale::Tr) => "Derleme ve araçlar",
        (Category::Deploy, Locale::En) => "Deploy and infra",
        (Category::Deploy, Locale::Tr) => "Dağıtım ve altyapı",
        (Category::Other, Locale::En) => "Other",
        (Category::Other, Locale::Tr) => "Diğer",
    }
}

/// One evidence line, without indentation.
pub fn evidence_line(evidence: &Evidence, locale: Locale) -> String {
    let en = locale == Locale::En;
    match evidence {
        Evidence::Language { files } if en => format!("used in {}", files_en(*files)),
        Evidence::Language { files } => format!("{files} dosyada kullanılıyor"),
        Evidence::Imported { files } if en => format!("imported in {}", files_en(*files)),
        Evidence::Imported { files } => format!("{files} dosyada içe aktarılıyor"),
        Evidence::NotDeclared { packages } if en => {
            format!("used but not declared: {}", packages.join(", "))
        }
        Evidence::NotDeclared { packages } => {
            format!("kullanılıyor ama tanımlı değil: {}", packages.join(", "))
        }
        Evidence::NotImported if en => {
            "declared, not imported (may be a command-line tool)".to_string()
        }
        Evidence::NotImported => {
            "tanımlı ama içe aktarılmıyor (bir komut satırı aracı olabilir)".to_string()
        }
        Evidence::Declared {
            file,
            line,
            name,
            declared,
            resolved,
            dev,
        } => {
            let (head, resolved_word, dev_word) = if en {
                ("declared", "resolved", "dev")
            } else {
                ("tanım", "kurulu", "geliştirme")
            };
            let mut text = format!("{head}: {file}:{line} · {name}");
            if let Some(declared) = declared {
                text.push(' ');
                text.push_str(declared);
            }
            if let Some(resolved) = resolved {
                let _ = write!(text, " ({resolved_word} {resolved})");
            }
            if *dev {
                let _ = write!(text, " · {dev_word}");
            }
            text
        }
        Evidence::Config { path } if en => format!("config file {path}"),
        Evidence::Config { path } => format!("ayar dosyası {path}"),
        Evidence::Image { image, file, line } if en => {
            format!("Docker image {image} ({file}:{line})")
        }
        Evidence::Image { image, file, line } => format!("Docker imajı {image} ({file}:{line})"),
    }
}

fn files_en(files: u32) -> String {
    if files == 1 {
        "1 file".to_string()
    } else {
        format!("{files} files")
    }
}

/// Whether the beginner level prints this evidence.
fn beginner_evidence(evidence: &Evidence) -> bool {
    matches!(
        evidence,
        Evidence::Language { .. }
            | Evidence::Imported { .. }
            | Evidence::NotDeclared { .. }
            | Evidence::NotImported
    )
}

fn render_root(
    root: &RootList,
    catalog: &Catalog,
    level: Level,
    locale: Locale,
    title: &str,
    with_roots: bool,
) -> String {
    let mut out = String::new();
    if with_roots {
        let path = if root.root.is_empty() {
            "."
        } else {
            &root.root
        };
        let _ = writeln!(out, "{title} · {path}");
    } else {
        let _ = writeln!(out, "{title}");
    }
    let shown = root.shown(level);
    for (category, items) in &shown.groups {
        let _ = writeln!(out, "{}", category_label(*category, locale));
        for item in items {
            render_item(&mut out, item, catalog, level, locale);
        }
    }
    if shown.more > 0 {
        match locale {
            Locale::En => {
                let _ = writeln!(out, "+{} more", shown.more);
            }
            Locale::Tr => {
                let _ = writeln!(out, "+{} tane daha", shown.more);
            }
        }
    }
    out
}

fn render_item(out: &mut String, item: &TechItem, catalog: &Catalog, level: Level, locale: Locale) {
    let text = item
        .tech
        .as_deref()
        .and_then(|id| catalog.tech(id))
        .map(|tech| tech.text(locale, level))
        .unwrap_or(match locale {
            Locale::En => "no description yet",
            Locale::Tr => "henüz açıklama yok",
        });
    let _ = writeln!(out, "  {}: {text}", item.name);
    for evidence in &item.evidence {
        if level == Level::Builder || beginner_evidence(evidence) {
            let _ = writeln!(out, "    {}", evidence_line(evidence, locale));
        }
    }
}
