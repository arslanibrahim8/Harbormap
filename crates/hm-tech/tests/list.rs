use hm_tech::{
    Catalog, Category, DeclaredDep, DockerImage, Ecosystem, Evidence, ExternalImport,
    LanguageCount, Level, PackageRef, RootList, TechInput, TechItem, TechList, build,
};

fn tech(id: &str, name: &str, category: &str, keys: &str) -> String {
    format!(
        r#"
[[tech]]
id = "{id}"
name = "{name}"
category = "{category}"
{keys}
en.beginner = "a short test description for {name}."
en.builder = "a longer test description of {name} for builders."
tr.beginner = "{name} için kısa bir deneme açıklaması."
tr.builder = "{name} için yapı kuranlara daha uzun bir deneme açıklaması."
"#
    )
}

fn catalog() -> Catalog {
    let mut pack = String::new();
    pack += &tech("react", "React", "ui", r#"npm = ["react", "react-dom"]"#);
    pack += &tech("radix-ui", "Radix UI", "ui", r#"npm = ["@radix-ui/*"]"#);
    pack += &tech("nextjs", "Next.js", "framework", r#"npm = ["next"]"#);
    pack += &tech("zod", "Zod", "validation", r#"npm = ["zod"]"#);
    pack += &tech("vitest", "Vitest", "testing", r#"npm = ["vitest"]"#);
    pack += &tech(
        "typescript",
        "TypeScript",
        "language",
        r#"npm = ["typescript"]
languages = ["typescript", "tsx"]"#,
    );
    pack += &tech(
        "postgresql",
        "PostgreSQL",
        "data",
        r#"npm = ["pg"]
pypi = ["psycopg"]"#,
    );
    pack += &tech("django", "Django", "framework", r#"pypi = ["Django"]"#);
    for n in 1..=13 {
        pack += &tech(
            &format!("t{n:02}"),
            &format!("Tool {n:02}"),
            "other",
            &format!(r#"npm = ["pkg-{n:02}"]"#),
        );
    }
    pack += "\n[[config]]\ntech = \"typescript\"\nfiles = [\"tsconfig.json\"]\n";
    pack += "\n[[image]]\ntech = \"postgresql\"\nnames = [\"postgres\"]\n";
    let catalog = Catalog::load(&[("test.toml", &pack)]);
    assert!(catalog.warnings().is_empty(), "{:#?}", catalog.warnings());
    catalog
}

fn dep(ecosystem: Ecosystem, name: &str, dev: bool, root: &str) -> DeclaredDep {
    DeclaredDep {
        ecosystem,
        name: name.to_string(),
        declared: Some("^1.0.0".to_string()),
        resolved: None,
        dev,
        file: format!("{root}/package.json")
            .trim_start_matches('/')
            .to_string(),
        line: 7,
        root: root.to_string(),
    }
}

fn npm(name: &str, root: &str) -> DeclaredDep {
    dep(Ecosystem::Npm, name, false, root)
}

fn import(ecosystem: Ecosystem, package: &str, importer: &str) -> ExternalImport {
    ExternalImport {
        ecosystem,
        package: package.to_string(),
        importer: importer.to_string(),
    }
}

fn npm_import(package: &str, importer: &str) -> ExternalImport {
    import(Ecosystem::Npm, package, importer)
}

fn imports_by(package: &str, count: usize) -> Vec<ExternalImport> {
    (0..count)
        .map(|n| npm_import(package, &format!("src/{package}-{n}.ts")))
        .collect()
}

fn input() -> TechInput {
    TechInput {
        roots: vec![String::new()],
        import_coverage: vec![Ecosystem::Npm],
        ..TechInput::default()
    }
}

fn names(items: &[&TechItem]) -> Vec<String> {
    items.iter().map(|item| item.name.clone()).collect()
}

fn root<'a>(list: &'a TechList, path: &str) -> &'a RootList {
    list.roots
        .iter()
        .find(|root| root.root == path)
        .unwrap_or_else(|| panic!("no root {path:?} in {list:#?}"))
}

fn item<'a>(root: &'a RootList, name: &str) -> &'a TechItem {
    root.items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no item {name:?} in {root:#?}"))
}

#[test]
fn ranks_by_import_count_inside_categories() {
    let mut input = input();
    input.declared = vec![
        npm("react", ""),
        npm("@radix-ui/react-dialog", ""),
        npm("zod", ""),
        npm("next", ""),
    ];
    input.imports.extend(imports_by("react", 3));
    input
        .imports
        .extend(imports_by("@radix-ui/react-dialog", 5));
    input.imports.extend(imports_by("zod", 1));
    input.imports.extend(imports_by("next", 2));
    let list = build(&catalog(), &input);
    assert_eq!(list.roots.len(), 1);
    let shown = list.roots[0].shown(Level::Builder);
    let categories: Vec<_> = shown.groups.iter().map(|(category, _)| *category).collect();
    assert_eq!(
        categories,
        [Category::Framework, Category::Ui, Category::Validation]
    );
    assert_eq!(names(&shown.groups[1].1), ["Radix UI", "React"]);
    assert_eq!(shown.more, 0);
}

#[test]
fn the_three_evidence_lines() {
    let mut input = input();
    input.declared = vec![npm("next", ""), dep(Ecosystem::Npm, "vitest", true, "")];
    input.imports = imports_by("next", 2);
    input.imports.push(npm_import("zod", "src/schema.ts"));
    let list = build(&catalog(), &input);
    let root = &list.roots[0];
    let declared = |name: &str, dev: bool| Evidence::Declared {
        file: "package.json".to_string(),
        line: 7,
        name: name.to_string(),
        declared: Some("^1.0.0".to_string()),
        resolved: None,
        dev,
    };
    assert_eq!(
        item(root, "Next.js").evidence,
        [Evidence::Imported { files: 2 }, declared("next", false)]
    );
    assert_eq!(
        item(root, "Vitest").evidence,
        [Evidence::NotImported, declared("vitest", true)]
    );
    assert_eq!(
        item(root, "Zod").evidence,
        [
            Evidence::Imported { files: 1 },
            Evidence::NotDeclared {
                packages: vec!["zod".to_string()]
            }
        ]
    );
    assert_eq!(item(root, "Next.js").score, 2);
    assert_eq!(item(root, "Vitest").score, 0);
    assert_eq!(item(root, "Vitest").tech.as_deref(), Some("vitest"));
}

#[test]
fn unmeasured_ecosystem_makes_no_import_claim() {
    let mut input = input();
    input.declared = vec![dep(Ecosystem::Pypi, "django", false, "")];
    let list = build(&catalog(), &input);
    let django = item(&list.roots[0], "Django");
    assert_eq!(
        django.evidence,
        [Evidence::Declared {
            file: "package.json".to_string(),
            line: 7,
            name: "django".to_string(),
            declared: Some("^1.0.0".to_string()),
            resolved: None,
            dev: false,
        }]
    );
}

#[test]
fn beginner_shows_twelve_with_more() {
    let mut input = input();
    for n in 1..=13 {
        input.declared.push(npm(&format!("pkg-{n:02}"), ""));
    }
    input.declared.push(npm("left-pad", ""));
    input.declared.push(npm("react", ""));
    let list = build(&catalog(), &input);
    let root = &list.roots[0];
    assert_eq!(root.items.len(), 15);

    let beginner = root.shown(Level::Beginner);
    let shown: Vec<_> = beginner
        .groups
        .iter()
        .flat_map(|(_, items)| items)
        .collect();
    assert_eq!(shown.len(), 12);
    assert!(shown.iter().all(|item| item.tech.is_some()));
    assert_eq!(beginner.more, 3);

    let builder = root.shown(Level::Builder);
    let all: usize = builder.groups.iter().map(|(_, items)| items.len()).sum();
    assert_eq!(all, 15);
    assert_eq!(builder.more, 0);

    // With room to spare, unknown items fill the remaining slots.
    let mut input = self::input();
    input.declared = vec![
        npm("pkg-01", ""),
        npm("pkg-02", ""),
        npm("react", ""),
        npm("left-pad", ""),
        npm("is-odd", ""),
    ];
    let list = build(&catalog(), &input);
    let beginner = list.roots[0].shown(Level::Beginner);
    let shown: usize = beginner.groups.iter().map(|(_, items)| items.len()).sum();
    assert_eq!(shown, 5);
    assert_eq!(beginner.more, 0);
}

#[test]
fn aliases_merge_into_one_item() {
    let mut input = input();
    input.declared = vec![npm("react", ""), npm("react-dom", "")];
    input.imports = vec![
        npm_import("react", "a.tsx"),
        npm_import("react", "b.tsx"),
        npm_import("react-dom", "b.tsx"),
        npm_import("react-dom", "c.tsx"),
        // A subpath import counts for its package.
        npm_import("react-dom/client", "c.tsx"),
    ];
    let list = build(&catalog(), &input);
    let root = &list.roots[0];
    assert_eq!(root.items.len(), 1);
    let react = &root.items[0];
    assert_eq!(react.name, "React");
    assert_eq!(react.score, 3);
    assert_eq!(react.evidence[0], Evidence::Imported { files: 3 });
    let declared = react
        .evidence
        .iter()
        .filter(|evidence| matches!(evidence, Evidence::Declared { .. }))
        .count();
    assert_eq!(declared, 2);
    assert_eq!(react.evidence.len(), 3);
}

#[test]
fn monorepo_roots_are_listed_separately() {
    let mut input = input();
    input.roots = vec![
        String::new(),
        "apps/web".to_string(),
        "apps/api".to_string(),
    ];
    input.declared = vec![npm("next", "apps/web"), npm("pg", "apps/api")];
    input.languages = vec![LanguageCount {
        path: "apps/web/src".to_string(),
        language: "typescript".to_string(),
        files: 10,
    }];
    let list = build(&catalog(), &input);
    let roots: Vec<_> = list.roots.iter().map(|root| root.root.as_str()).collect();
    assert_eq!(roots, ["apps/api", "apps/web"]);
    let web = root(&list, "apps/web");
    let web_names: Vec<_> = web.items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(web_names, ["TypeScript", "Next.js"]);
    assert_eq!(item(web, "TypeScript").score, 10);
    assert_eq!(root(&list, "apps/api").items[0].name, "PostgreSQL");
}

#[test]
fn a_hoisted_dependency_counts_as_declared() {
    let mut input = input();
    input.roots = vec![String::new(), "apps/web".to_string()];
    input.declared = vec![npm("react", ""), npm("next", "apps/web")];
    input.imports = vec![npm_import("react", "apps/web/src/a.tsx")];
    let list = build(&catalog(), &input);
    let react = item(root(&list, ""), "React");
    assert_eq!(react.evidence[0], Evidence::Imported { files: 1 });
    assert!(!react.evidence.contains(&Evidence::NotImported));
    let web = root(&list, "apps/web");
    assert!(web.items.iter().all(|item| {
        item.evidence
            .iter()
            .all(|evidence| !matches!(evidence, Evidence::NotDeclared { .. }))
    }));
    assert_eq!(web.items.len(), 1);
}

#[test]
fn builtins_and_aliases_are_not_listed() {
    let mut input = input();
    input.imports = [
        "fs",
        "fs/promises",
        "node:path",
        "@/components/x",
        "~/lib",
        "#db",
        "astro:content",
        "./local",
        "../up",
        "@scope",
        "$app/stores",
        "$lib/x",
        "_private",
        "bun",
    ]
    .iter()
    .map(|package| npm_import(package, "src/a.ts"))
    .collect();
    input.imports.push(import(Ecosystem::Go, "fmt", "main.go"));
    input
        .imports
        .push(import(Ecosystem::Go, "net/http", "main.go"));
    let list = build(&catalog(), &input);
    assert!(list.roots.is_empty(), "{list:#?}");
}

#[test]
fn subpath_imports_use_the_package_name() {
    let mut input = input();
    input.declared = vec![npm("react-dom", ""), npm("@radix-ui/react-dialog", "")];
    input.imports = vec![
        npm_import("react-dom/client", "a.tsx"),
        npm_import("@radix-ui/react-dialog/dist/x", "b.tsx"),
        npm_import("lodash/fp", "c.ts"),
    ];
    let list = build(&catalog(), &input);
    let root = &list.roots[0];
    assert_eq!(
        item(root, "React").evidence[0],
        Evidence::Imported { files: 1 }
    );
    assert_eq!(
        item(root, "Radix UI").evidence[0],
        Evidence::Imported { files: 1 }
    );
    let lodash = item(root, "lodash");
    assert_eq!(lodash.tech, None);
    assert_eq!(lodash.category, Category::Other);
    assert_eq!(
        lodash.evidence,
        [
            Evidence::Imported { files: 1 },
            Evidence::NotDeclared {
                packages: vec!["lodash".to_string()]
            }
        ]
    );
}

#[test]
fn own_packages_are_not_technologies() {
    let mut input = input();
    input.own_packages = vec![PackageRef {
        ecosystem: Ecosystem::Npm,
        name: "@shop/ui".to_string(),
    }];
    let mut shop = npm("@shop/ui", "");
    shop.declared = Some("workspace:*".to_string());
    input.declared = vec![shop];
    input.imports = vec![
        npm_import("@shop/ui", "src/a.ts"),
        npm_import("@shop/ui/button", "src/b.ts"),
    ];
    let list = build(&catalog(), &input);
    assert!(list.roots.is_empty(), "{list:#?}");
}

#[test]
fn go_modules_match_by_segment_prefix() {
    let mut input = input();
    input.own_packages = vec![PackageRef {
        ecosystem: Ecosystem::Go,
        name: "example.com/shop".to_string(),
    }];
    input.declared = vec![dep(Ecosystem::Go, "github.com/acme/lib", false, "")];
    input.import_coverage = vec![Ecosystem::Go];
    input.imports = vec![
        import(Ecosystem::Go, "github.com/acme/lib/sub", "a.go"),
        import(Ecosystem::Go, "github.com/acme/lib", "b.go"),
        import(Ecosystem::Go, "example.com/shop/internal/x", "c.go"),
        import(Ecosystem::Go, "github.com/acme/library", "d.go"),
    ];
    let list = build(&catalog(), &input);
    let root = &list.roots[0];
    assert_eq!(root.items.len(), 2, "{root:#?}");
    let lib = item(root, "github.com/acme/lib");
    assert_eq!(lib.evidence[0], Evidence::Imported { files: 2 });
    let library = item(root, "github.com/acme/library");
    assert_eq!(
        library.evidence[1],
        Evidence::NotDeclared {
            packages: vec!["github.com/acme/library".to_string()]
        }
    );
}

#[test]
fn variable_and_scratch_images_are_skipped() {
    let mut input = input();
    let image = |image: &str, line: u32| DockerImage {
        image: image.to_string(),
        file: "Dockerfile".to_string(),
        line,
    };
    input.images = vec![
        image("scratch", 1),
        image("${BASE_IMAGE}", 2),
        image("postgres:16", 1),
        image("acme/worker:2", 3),
    ];
    let list = build(&catalog(), &input);
    let root = &list.roots[0];
    assert_eq!(root.items.len(), 2, "{root:#?}");
    let postgres = item(root, "PostgreSQL");
    assert_eq!(
        postgres.evidence,
        [Evidence::Image {
            image: "postgres:16".to_string(),
            file: "Dockerfile".to_string(),
            line: 1
        }]
    );
    let worker = item(root, "acme/worker");
    assert_eq!(worker.tech, None);
    assert_eq!(worker.category, Category::Other);
}

#[test]
fn languages_and_configs_join_their_tech() {
    let mut input = input();
    let language = |language: &str, files: u32| LanguageCount {
        path: "src".to_string(),
        language: language.to_string(),
        files,
    };
    input.languages = vec![
        language("typescript", 40),
        language("tsx", 2),
        language("python", 0),
    ];
    input.declared = vec![dep(Ecosystem::Npm, "typescript", true, "")];
    input.configs = vec![
        "tsconfig.json".to_string(),
        "tsconfig.json".to_string(),
        "README.md".to_string(),
    ];
    let list = build(&catalog(), &input);
    let root = &list.roots[0];
    // The zero-file language is not listed.
    assert_eq!(root.items.len(), 1, "{root:#?}");
    let typescript = &root.items[0];
    assert_eq!(typescript.name, "TypeScript");
    assert_eq!(typescript.score, 42);
    assert_eq!(
        typescript.evidence,
        [
            Evidence::Language { files: 42 },
            Evidence::NotImported,
            Evidence::Declared {
                file: "package.json".to_string(),
                line: 7,
                name: "typescript".to_string(),
                declared: Some("^1.0.0".to_string()),
                resolved: None,
                dev: true,
            },
            Evidence::Config {
                path: "tsconfig.json".to_string()
            },
        ]
    );
}

#[test]
fn unknown_languages_are_listed_by_name() {
    let mut input = input();
    input.languages = vec![LanguageCount {
        path: "src".to_string(),
        language: "Zig".to_string(),
        files: 4,
    }];
    let list = build(&catalog(), &input);
    let zig = &list.roots[0].items[0];
    assert_eq!(zig.name, "Zig");
    assert_eq!(zig.tech, None);
    assert_eq!(zig.category, Category::Language);
    assert_eq!(zig.evidence, [Evidence::Language { files: 4 }]);
}

#[test]
fn dev_only_items_rank_after_equal_scores() {
    let mut input = input();
    // Same score and category; the dev-only item sorts first by name, so only the dev flag
    // puts it last.
    input.declared = vec![dep(Ecosystem::Npm, "pkg-01", true, ""), npm("pkg-02", "")];
    let list = build(&catalog(), &input);
    let other: Vec<_> = list.roots[0]
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(other, ["Tool 02", "Tool 01"]);
}

#[test]
fn ties_rank_by_category_before_dev_flag() {
    let mut input = input();
    input.declared = vec![
        npm("pkg-01", ""),
        npm("react", ""),
        dep(Ecosystem::Npm, "typescript", true, ""),
    ];
    let list = build(&catalog(), &input);
    let ranked: Vec<_> = list.roots[0]
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(ranked, ["TypeScript", "React", "Tool 01"]);
}

fn shuffle<T>(items: &mut [T], seed: &mut u64) {
    for i in (1..items.len()).rev() {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        items.swap(i, (*seed >> 33) as usize % (i + 1));
    }
}

#[test]
fn build_is_deterministic() {
    let mut input = input();
    input.roots = vec![
        String::new(),
        "apps/web".to_string(),
        "apps/api".to_string(),
    ];
    input.own_packages = vec![PackageRef {
        ecosystem: Ecosystem::Npm,
        name: "@shop/ui".to_string(),
    }];
    input.declared = vec![
        npm("react", ""),
        npm("react-dom", ""),
        npm("next", "apps/web"),
        dep(Ecosystem::Npm, "vitest", true, "apps/web"),
        npm("pg", "apps/api"),
        npm("left-pad", "apps/api"),
        npm("@shop/ui", "apps/web"),
        dep(Ecosystem::Pypi, "Django", false, "apps/api"),
    ];
    input.imports = vec![
        npm_import("react", "apps/web/a.tsx"),
        npm_import("react-dom/client", "apps/web/b.tsx"),
        npm_import("zod", "apps/web/b.tsx"),
        npm_import("zod", "apps/api/c.ts"),
        npm_import("lodash", "apps/api/c.ts"),
        npm_import("pg", "apps/api/d.ts"),
        npm_import("fs", "apps/api/d.ts"),
    ];
    input.configs = vec![
        "tsconfig.json".to_string(),
        "apps/web/tsconfig.json".to_string(),
    ];
    input.images = vec![
        DockerImage {
            image: "postgres:16".to_string(),
            file: "docker/Dockerfile".to_string(),
            line: 1,
        },
        DockerImage {
            image: "postgres:15".to_string(),
            file: "apps/api/Dockerfile".to_string(),
            line: 2,
        },
    ];
    input.languages = vec![
        LanguageCount {
            path: "apps/web".to_string(),
            language: "typescript".to_string(),
            files: 3,
        },
        LanguageCount {
            path: "apps/web/src".to_string(),
            language: "tsx".to_string(),
            files: 4,
        },
        LanguageCount {
            path: "apps/api".to_string(),
            language: "python".to_string(),
            files: 9,
        },
    ];
    let catalog = catalog();
    let expected = serde_json::to_string(&build(&catalog, &input)).unwrap();
    assert!(expected.contains("apps/web"));
    let mut seed = 42;
    for _ in 0..20 {
        let mut shuffled = input.clone();
        shuffle(&mut shuffled.roots, &mut seed);
        shuffle(&mut shuffled.own_packages, &mut seed);
        shuffle(&mut shuffled.declared, &mut seed);
        shuffle(&mut shuffled.imports, &mut seed);
        shuffle(&mut shuffled.import_coverage, &mut seed);
        shuffle(&mut shuffled.configs, &mut seed);
        shuffle(&mut shuffled.images, &mut seed);
        shuffle(&mut shuffled.languages, &mut seed);
        assert_eq!(
            serde_json::to_string(&build(&catalog, &shuffled)).unwrap(),
            expected
        );
    }
}
