use hm_tech::{
    Catalog, Category, DeclaredDep, Ecosystem, Evidence, ExternalImport, Level, Locale, RootList,
    TechInput, TechItem, TechList, build, category_label, evidence_line, render,
};

fn tech(id: &str, name: &str, category: &str, keys: &str) -> String {
    format!(
        r#"
[[tech]]
id = "{id}"
name = "{name}"
category = "{category}"
{keys}
en.beginner = "{name} short text."
en.builder = "{name} builder text for builders."
tr.beginner = "{name} için kısa bir deneme açıklaması."
tr.builder = "{name} için yapı kuranlara daha uzun bir açıklama."
"#
    )
}

fn catalog() -> Catalog {
    let mut pack = tech("nextjs", "Next.js", "framework", r#"npm = ["next"]"#);
    for n in 1..=12 {
        pack += &tech(
            &format!("t{n:02}"),
            &format!("Tool {n:02}"),
            "other",
            &format!(r#"npm = ["pkg-{n:02}"]"#),
        );
    }
    let catalog = Catalog::load(&[("test.toml", &pack)]);
    assert!(catalog.warnings().is_empty(), "{:#?}", catalog.warnings());
    catalog
}

fn npm(name: &str, declared: &str, resolved: Option<&str>, dev: bool, line: u32) -> DeclaredDep {
    DeclaredDep {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        declared: Some(declared.to_string()),
        resolved: resolved.map(str::to_string),
        dev,
        file: "package.json".to_string(),
        line,
        root: String::new(),
    }
}

fn import(package: &str, importer: &str) -> ExternalImport {
    ExternalImport {
        ecosystem: Ecosystem::Npm,
        package: package.to_string(),
        importer: importer.to_string(),
    }
}

/// Next.js imported in 2 files, 12 fillers imported in 1 file each, and `left-pad` declared
/// as a dev dependency that nothing imports.
fn project() -> (Catalog, TechList) {
    let catalog = catalog();
    let mut input = TechInput {
        roots: vec![String::new()],
        import_coverage: vec![Ecosystem::Npm],
        ..TechInput::default()
    };
    input
        .declared
        .push(npm("next", "^15.0.0", Some("15.0.3"), false, 12));
    input.imports.push(import("next", "src/a.ts"));
    input.imports.push(import("next", "src/b.ts"));
    for n in 1..=12 {
        let name = format!("pkg-{n:02}");
        input
            .declared
            .push(npm(&name, "^1.0.0", None, false, 20 + n));
        input
            .imports
            .push(import(&name, &format!("src/f{n:02}.ts")));
    }
    input
        .declared
        .push(npm("left-pad", "^1.3.0", None, true, 30));
    let list = build(&catalog, &input);
    (catalog, list)
}

#[test]
fn beginner_turkish_golden() {
    let (catalog, list) = project();
    let expected = "\
Teknolojiler
Çatı (framework)
  Next.js: Next.js için kısa bir deneme açıklaması.
    2 dosyada içe aktarılıyor
Diğer
  Tool 01: Tool 01 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 02: Tool 02 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 03: Tool 03 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 04: Tool 04 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 05: Tool 05 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 06: Tool 06 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 07: Tool 07 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 08: Tool 08 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 09: Tool 09 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 10: Tool 10 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
  Tool 11: Tool 11 için kısa bir deneme açıklaması.
    1 dosyada içe aktarılıyor
+2 tane daha
";
    assert_eq!(
        render(&list, &catalog, Level::Beginner, Locale::Tr),
        expected
    );
}

#[test]
fn builder_english_golden() {
    let (catalog, list) = project();
    let expected = "\
Technologies
Framework
  Next.js: Next.js builder text for builders.
    imported in 2 files
    declared: package.json:12 · next ^15.0.0 (resolved 15.0.3)
Other
  Tool 01: Tool 01 builder text for builders.
    imported in 1 file
    declared: package.json:21 · pkg-01 ^1.0.0
  Tool 02: Tool 02 builder text for builders.
    imported in 1 file
    declared: package.json:22 · pkg-02 ^1.0.0
  Tool 03: Tool 03 builder text for builders.
    imported in 1 file
    declared: package.json:23 · pkg-03 ^1.0.0
  Tool 04: Tool 04 builder text for builders.
    imported in 1 file
    declared: package.json:24 · pkg-04 ^1.0.0
  Tool 05: Tool 05 builder text for builders.
    imported in 1 file
    declared: package.json:25 · pkg-05 ^1.0.0
  Tool 06: Tool 06 builder text for builders.
    imported in 1 file
    declared: package.json:26 · pkg-06 ^1.0.0
  Tool 07: Tool 07 builder text for builders.
    imported in 1 file
    declared: package.json:27 · pkg-07 ^1.0.0
  Tool 08: Tool 08 builder text for builders.
    imported in 1 file
    declared: package.json:28 · pkg-08 ^1.0.0
  Tool 09: Tool 09 builder text for builders.
    imported in 1 file
    declared: package.json:29 · pkg-09 ^1.0.0
  Tool 10: Tool 10 builder text for builders.
    imported in 1 file
    declared: package.json:30 · pkg-10 ^1.0.0
  Tool 11: Tool 11 builder text for builders.
    imported in 1 file
    declared: package.json:31 · pkg-11 ^1.0.0
  Tool 12: Tool 12 builder text for builders.
    imported in 1 file
    declared: package.json:32 · pkg-12 ^1.0.0
  left-pad: no description yet
    declared, not imported (may be a command-line tool)
    declared: package.json:30 · left-pad ^1.3.0 · dev
";
    assert_eq!(
        render(&list, &catalog, Level::Builder, Locale::En),
        expected
    );
}

#[test]
fn singular_file_counts() {
    assert_eq!(
        evidence_line(&Evidence::Imported { files: 1 }, Locale::En),
        "imported in 1 file"
    );
    assert_eq!(
        evidence_line(&Evidence::Language { files: 1 }, Locale::En),
        "used in 1 file"
    );
    assert_eq!(
        evidence_line(&Evidence::Language { files: 3 }, Locale::En),
        "used in 3 files"
    );
    assert_eq!(
        evidence_line(&Evidence::Language { files: 1 }, Locale::Tr),
        "1 dosyada kullanılıyor"
    );
}

#[test]
fn every_evidence_line_in_both_languages() {
    let declared = Evidence::Declared {
        file: "Cargo.toml".to_string(),
        line: 4,
        name: "serde".to_string(),
        declared: Some("1".to_string()),
        resolved: Some("1.0.2".to_string()),
        dev: true,
    };
    let bare = Evidence::Declared {
        file: "go.mod".to_string(),
        line: 9,
        name: "gin".to_string(),
        declared: None,
        resolved: None,
        dev: false,
    };
    let not_declared = Evidence::NotDeclared {
        packages: vec!["a".to_string(), "b".to_string()],
    };
    let config = Evidence::Config {
        path: "tsconfig.json".to_string(),
    };
    let image = Evidence::Image {
        image: "postgres:16".to_string(),
        file: "Dockerfile".to_string(),
        line: 1,
    };
    let cases: [(&Evidence, &str, &str); 7] = [
        (
            &Evidence::Imported { files: 2 },
            "imported in 2 files",
            "2 dosyada içe aktarılıyor",
        ),
        (
            &not_declared,
            "used but not declared: a, b",
            "kullanılıyor ama tanımlı değil: a, b",
        ),
        (
            &Evidence::NotImported,
            "declared, not imported (may be a command-line tool)",
            "tanımlı ama içe aktarılmıyor (bir komut satırı aracı olabilir)",
        ),
        (
            &declared,
            "declared: Cargo.toml:4 · serde 1 (resolved 1.0.2) · dev",
            "tanım: Cargo.toml:4 · serde 1 (kurulu 1.0.2) · geliştirme",
        ),
        (&bare, "declared: go.mod:9 · gin", "tanım: go.mod:9 · gin"),
        (
            &config,
            "config file tsconfig.json",
            "ayar dosyası tsconfig.json",
        ),
        (
            &image,
            "Docker image postgres:16 (Dockerfile:1)",
            "Docker imajı postgres:16 (Dockerfile:1)",
        ),
    ];
    for (evidence, en, tr) in cases {
        assert_eq!(evidence_line(evidence, Locale::En), en);
        assert_eq!(evidence_line(evidence, Locale::Tr), tr);
    }
}

fn one_item_root(root: &str) -> RootList {
    RootList {
        root: root.to_string(),
        items: vec![TechItem {
            tech: Some("nextjs".to_string()),
            name: "Next.js".to_string(),
            category: Category::Framework,
            score: 1,
            evidence: vec![Evidence::Imported { files: 1 }],
        }],
    }
}

#[test]
fn monorepo_titles() {
    let catalog = catalog();
    let two = TechList {
        roots: vec![one_item_root(""), one_item_root("apps/web")],
    };
    let expected = "\
Technologies · .
Framework
  Next.js: Next.js short text.
    imported in 1 file

Technologies · apps/web
Framework
  Next.js: Next.js short text.
    imported in 1 file
";
    assert_eq!(
        render(&two, &catalog, Level::Beginner, Locale::En),
        expected
    );
    let turkish = render(&two, &catalog, Level::Beginner, Locale::Tr);
    assert!(turkish.starts_with("Teknolojiler · .\n"), "{turkish}");
    assert!(turkish.contains("\nTeknolojiler · apps/web\n"), "{turkish}");

    let one = TechList {
        roots: vec![one_item_root("apps/web")],
    };
    let text = render(&one, &catalog, Level::Beginner, Locale::En);
    assert!(text.starts_with("Technologies\nFramework\n"), "{text}");
    assert!(!text.contains('·'), "{text}");
}

#[test]
fn unknown_items_have_no_description_and_beginner_hides_detail_lines() {
    let catalog = catalog();
    let list = TechList {
        roots: vec![RootList {
            root: String::new(),
            items: vec![TechItem {
                tech: None,
                name: "left-pad".to_string(),
                category: Category::Other,
                score: 0,
                evidence: vec![
                    Evidence::NotImported,
                    Evidence::Config {
                        path: "x.json".to_string(),
                    },
                ],
            }],
        }],
    };
    assert_eq!(
        render(&list, &catalog, Level::Beginner, Locale::Tr),
        "Teknolojiler\nDiğer\n  left-pad: henüz açıklama yok\n    tanımlı ama içe aktarılmıyor (bir komut satırı aracı olabilir)\n"
    );
}

#[test]
fn empty_list_is_just_the_title() {
    let list = TechList { roots: Vec::new() };
    assert_eq!(
        render(&list, &catalog(), Level::Builder, Locale::En),
        "Technologies\n"
    );
}

#[test]
fn every_category_has_both_labels() {
    for category in Category::ALL {
        let en = category_label(category, Locale::En);
        let tr = category_label(category, Locale::Tr);
        assert!(!en.is_empty() && !tr.is_empty(), "{category:?}");
        assert_ne!(en, tr, "{category:?}");
    }
}

#[test]
fn category_labels_are_the_agreed_ones() {
    let en: Vec<_> = Category::ALL
        .iter()
        .map(|c| category_label(*c, Locale::En))
        .collect();
    let tr: Vec<_> = Category::ALL
        .iter()
        .map(|c| category_label(*c, Locale::Tr))
        .collect();
    assert_eq!(
        en,
        [
            "Language",
            "Framework",
            "UI",
            "Data and ORM",
            "Auth and security",
            "Validation",
            "Testing",
            "Build and tooling",
            "Deploy and infra",
            "Other",
        ]
    );
    assert_eq!(
        tr,
        [
            "Dil",
            "Çatı (framework)",
            "Arayüz",
            "Veri ve ORM",
            "Kimlik ve güvenlik",
            "Doğrulama",
            "Test",
            "Derleme ve araçlar",
            "Dağıtım ve altyapı",
            "Diğer",
        ]
    );
}
