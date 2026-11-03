use hm_tech::names::{is_builtin, is_package_specifier, normalize, normalize_image};
use hm_tech::{Catalog, Category, Ecosystem, Level, Locale};

const PACK: &str = r#"
[[tech]]
id = "react"
name = "React"
category = "ui"
npm = ["react", "react-dom"]
en.beginner = "a library for building screens from small reusable pieces."
en.builder = "a component library for building user interfaces with JSX and hooks."
tr.beginner = "ekranları küçük, tekrar kullanılabilir parçalardan kuran bir kütüphane."
tr.builder = "JSX ve hook'larla kullanıcı arayüzü kurmaya yarayan bir bileşen kütüphanesi."

[[tech]]
id = "radix-ui"
name = "Radix UI"
category = "ui"
npm = ["@radix-ui/*"]
en.beginner = "ready-made building blocks for menus, dialogs and other controls."
en.builder = "unstyled, accessible UI primitives for React, published as one package per component."
tr.beginner = "menü, pencere ve benzeri kontroller için hazır yapı taşları."
tr.builder = "React için stili olmayan, erişilebilir arayüz parçaları; her bileşen ayrı pakettir."

[[tech]]
id = "django"
name = "Django"
category = "framework"
pypi = ["Django"]
en.beginner = "a toolkit for building websites with Python."
en.builder = "a batteries-included Python web framework with an ORM, admin and templating."
tr.beginner = "Python ile web siteleri kurmaya yarayan bir araç takımı."
tr.builder = "ORM, yönetim paneli ve şablon sistemiyle gelen, her şeyi içeren bir Python web çatısı."

[[tech]]
id = "drf"
name = "Django REST framework"
category = "framework"
pypi = ["djangorestframework"]
en.beginner = "adds a data API to a Django website."
en.builder = "a toolkit on top of Django for building web APIs with serializers and viewsets."
tr.beginner = "Django sitesine veri sunan bir API ekler."
tr.builder = "Django üzerinde serileştiriciler ve görünüm kümeleriyle web API'leri kurmaya yarayan bir araç takımı."

[[tech]]
id = "newtonsoft"
name = "Newtonsoft.Json"
category = "data"
nuget = ["Newtonsoft.Json"]
en.beginner = "reads and writes JSON data in .NET code."
en.builder = "a popular JSON serializer and parser for .NET."
tr.beginner = ".NET kodunda JSON verisini okur ve yazar."
tr.builder = ".NET için yaygın kullanılan bir JSON serileştirici ve ayrıştırıcı."

[[tech]]
id = "serde-json"
name = "serde_json"
category = "data"
crates = ["serde_json"]
en.beginner = "reads and writes JSON data in Rust code."
en.builder = "the JSON serialization and deserialization crate built on serde."
tr.beginner = "Rust kodunda JSON verisini okur ve yazar."
tr.builder = "serde üzerine kurulu JSON serileştirme ve çözme kütüphanesi."

[[tech]]
id = "postgresql"
name = "PostgreSQL"
category = "data"
go = ["github.com/jackc/pgx"]
npm = ["pg"]
en.beginner = "a database that stores the app's data in tables."
en.builder = "an open-source relational database; these packages are its Go and Node.js drivers."
tr.beginner = "uygulamanın verisini tablolarda tutan bir veritabanı."
tr.builder = "açık kaynaklı bir ilişkisel veritabanı; bu paketler Go ve Node.js sürücüleridir."

[[tech]]
id = "typescript"
name = "TypeScript"
category = "language"
languages = ["typescript", "tsx"]
en.beginner = "JavaScript with type checks that catch mistakes early."
en.builder = "a typed superset of JavaScript that compiles to plain JavaScript."
tr.beginner = "hataları erken yakalayan tür denetimli JavaScript."
tr.builder = "düz JavaScript'e derlenen, türlü bir JavaScript üst kümesi."

[[tech]]
id = "nextjs"
name = "Next.js"
category = "framework"
npm = ["next"]
en.beginner = "a toolkit that turns React code into a full website."
en.builder = "a React framework with file-based routing, server rendering and a build pipeline."
tr.beginner = "React kodunu tam bir web sitesine çeviren bir araç takımı."
tr.builder = "dosya tabanlı yönlendirme, sunucuda sayfa üretimi ve derleme hattı sunan bir React çatısı."

[[tech]]
id = "docker"
name = "Docker"
category = "deploy"
en.beginner = "packs the app so it runs the same on any machine."
en.builder = "a container platform; Dockerfiles describe the images the app is built into."
tr.beginner = "uygulamayı her makinede aynı çalışacak şekilde paketler."
tr.builder = "bir kapsayıcı platformu; Dockerfile'lar uygulamanın içine kurulduğu imajları tanımlar."

[[config]]
tech = "nextjs"
files = ["next.config.*"]

[[config]]
tech = "typescript"
files = ["tsconfig.json", "tsconfig.*.json"]

[[config]]
tech = "docker"
files = ["Dockerfile", "Dockerfile.*", "*.Dockerfile"]

[[image]]
tech = "postgresql"
names = ["postgres"]
"#;

fn catalog() -> Catalog {
    Catalog::load(&[("t.toml", PACK)])
}

fn id(tech: Option<&hm_tech::Tech>) -> Option<&str> {
    tech.map(|tech| tech.id.as_str())
}

#[test]
fn a_valid_pack_loads_without_warnings() {
    let catalog = catalog();
    assert!(catalog.warnings().is_empty(), "{:#?}", catalog.warnings());
    assert_eq!(catalog.iter().count(), 10);
    assert_eq!(catalog.key_count(Ecosystem::Npm), 5);
    assert_eq!(catalog.key_count(Ecosystem::Go), 1);
}

#[test]
fn texts_and_keys_are_reachable_from_a_tech() {
    let catalog = catalog();
    let react = catalog.tech("react").unwrap();
    assert_eq!(react.name, "React");
    assert_eq!(react.category, Category::Ui);
    assert_eq!(react.keys(Ecosystem::Npm), ["react", "react-dom"]);
    assert!(react.keys(Ecosystem::Pypi).is_empty());
    assert!(
        react
            .text(Locale::En, Level::Beginner)
            .starts_with("a library")
    );
    assert!(react.text(Locale::Tr, Level::Builder).starts_with("JSX ve"));
    assert!(catalog.tech("missing").is_none());
    assert_eq!(catalog.iter().next().unwrap().id, "react");
}

#[test]
fn enums_serialise_in_snake_case() {
    assert_eq!(
        serde_json::to_string(&Category::Language).unwrap(),
        "\"language\""
    );
    assert_eq!(serde_json::to_string(&Ecosystem::Pypi).unwrap(), "\"pypi\"");
    assert_eq!(Category::ALL.len(), 10);
    assert_eq!(Category::Ui.as_str(), "ui");
    assert_eq!(Ecosystem::Crates.as_str(), "crates");
    assert!(Category::Language < Category::Other);
}

#[test]
fn scoped_patterns_and_aliases_resolve() {
    let catalog = catalog();
    assert_eq!(
        id(catalog.package(Ecosystem::Npm, "@radix-ui/react-dialog")),
        Some("radix-ui")
    );
    assert_eq!(
        id(catalog.package(Ecosystem::Npm, "react-dom")),
        Some("react")
    );
    assert_eq!(id(catalog.package(Ecosystem::Npm, "left-pad")), None);
    assert_eq!(id(catalog.package(Ecosystem::Npm, "@radix-ui")), None);
}

#[test]
fn names_are_normalised_per_ecosystem() {
    let catalog = catalog();
    assert_eq!(
        id(catalog.package(Ecosystem::Pypi, "Django")),
        Some("django")
    );
    assert_eq!(
        id(catalog.package(Ecosystem::Pypi, "DjangoRestFramework")),
        Some("drf")
    );
    assert_eq!(
        normalize(Ecosystem::Pypi, "Flask_SQLAlchemy"),
        normalize(Ecosystem::Pypi, "flask.sqlalchemy")
    );
    assert_eq!(
        normalize(Ecosystem::Pypi, "Flask_SQLAlchemy"),
        "flask-sqlalchemy"
    );
    assert_eq!(normalize(Ecosystem::Pypi, "a-_.b"), "a-b");
    assert_eq!(
        id(catalog.package(Ecosystem::Nuget, "newtonsoft.json")),
        Some("newtonsoft")
    );
    assert_eq!(
        id(catalog.package(Ecosystem::Crates, "serde-json")),
        Some("serde-json")
    );
    assert_eq!(
        id(catalog.package(Ecosystem::Go, "github.com/jackc/pgx/v5/pgxpool")),
        Some("postgresql")
    );
    assert_eq!(
        id(catalog.package(Ecosystem::Go, "github.com/jackc/pgx")),
        Some("postgresql")
    );
    assert_eq!(
        id(catalog.package(Ecosystem::Go, "github.com/jackc/pgxx")),
        None
    );
    assert_eq!(normalize(Ecosystem::Go, "Github.com/X"), "Github.com/X");
    assert_eq!(normalize(Ecosystem::Maven, "Org.Foo:Bar"), "Org.Foo:Bar");
    assert_eq!(normalize(Ecosystem::Npm, "@Scope/Pkg"), "@scope/pkg");
}

#[test]
fn the_longest_match_wins() {
    let pack = r#"
[[tech]]
id = "scope"
name = "Scope"
category = "other"
npm = ["@scope/*"]
go = ["example.com/*"]
en.beginner = "everything under one scope."
en.builder = "every package published under the scope."
tr.beginner = "tek bir kapsamdaki her şey."
tr.builder = "kapsam altında yayımlanan her paket."

[[tech]]
id = "scope-core"
name = "Scope Core"
category = "other"
npm = ["@scope/core", "@scope/core-*"]
go = ["example.com/lib", "example.com/lib/deep/*"]
en.beginner = "the core of the scope."
en.builder = "the core package and its plugins."
tr.beginner = "kapsamın çekirdeği."
tr.builder = "çekirdek paket ve eklentileri."
"#;
    let catalog = Catalog::load(&[("t.toml", pack)]);
    assert!(catalog.warnings().is_empty(), "{:#?}", catalog.warnings());
    let npm = |name| id(catalog.package(Ecosystem::Npm, name));
    assert_eq!(npm("@scope/core"), Some("scope-core"));
    assert_eq!(npm("@scope/core-extra"), Some("scope-core"));
    assert_eq!(npm("@scope/other"), Some("scope"));
    let go = |name| id(catalog.package(Ecosystem::Go, name));
    // A segment-prefix exact key beats a shorter pattern.
    assert_eq!(go("example.com/lib/sub/pkg"), Some("scope-core"));
    // A longer pattern beats a shorter segment-prefix exact key.
    assert_eq!(go("example.com/lib/deep/pkg"), Some("scope-core"));
    assert_eq!(go("example.com/other"), Some("scope"));
    assert_eq!(go("example.org/lib"), None);
}

#[test]
fn config_files_match_by_basename() {
    let catalog = catalog();
    let config = |path| id(catalog.config_file(path));
    assert_eq!(config("apps/web/next.config.mjs"), Some("nextjs"));
    assert_eq!(config("tsconfig.build.json"), Some("typescript"));
    assert_eq!(config("tsconfig.json"), Some("typescript"));
    assert_eq!(config("deploy/api.Dockerfile"), Some("docker"));
    assert_eq!(config("Dockerfile"), Some("docker"));
    // `*` spans dots.
    assert_eq!(config("src/next.config.ts.bak"), Some("nextjs"));
    assert_eq!(config("README.md"), None);
    assert_eq!(config("next.config"), None);
}

#[test]
fn image_names_are_normalised() {
    let catalog = catalog();
    let image = |name| id(catalog.image(name));
    assert_eq!(
        image("docker.io/library/postgres:16-alpine"),
        Some("postgresql")
    );
    assert_eq!(image("postgres@sha256:abc"), Some("postgresql"));
    assert_eq!(image("POSTGRES"), Some("postgresql"));
    assert_eq!(image("redis"), None);
    assert_eq!(
        normalize_image("localhost:5000/shop/api:1"),
        "localhost:5000/shop/api"
    );
    assert_eq!(
        normalize_image("index.docker.io/library/node:20-alpine"),
        "node"
    );
    assert_eq!(
        normalize_image("localhost:5000/shop"),
        "localhost:5000/shop"
    );
}

#[test]
fn languages_resolve() {
    let catalog = catalog();
    assert_eq!(id(catalog.language("tsx")), Some("typescript"));
    assert_eq!(id(catalog.language("TypeScript")), Some("typescript"));
    assert_eq!(id(catalog.language("kotlin")), None);
}

#[test]
fn builtins_and_specifiers() {
    assert!(is_builtin(Ecosystem::Npm, "fs"));
    assert!(is_builtin(Ecosystem::Npm, "node:test"));
    assert!(is_builtin(Ecosystem::Npm, "bun:sqlite"));
    assert!(!is_builtin(Ecosystem::Npm, "react"));
    assert!(is_builtin(Ecosystem::Go, "net/http"));
    assert!(!is_builtin(Ecosystem::Go, "github.com/x/y"));
    assert!(!is_builtin(Ecosystem::Pypi, "os"));
    for name in [
        "@/lib",
        "~/x",
        "#db",
        "astro:content",
        "./a",
        "../a",
        "/abs",
        "@scope",
        "",
    ] {
        assert!(
            !is_package_specifier(Ecosystem::Npm, name),
            "{name} is not a package"
        );
    }
    for name in ["react", "@scope/pkg", "left-pad/sub"] {
        assert!(is_package_specifier(Ecosystem::Npm, name), "{name}");
    }
    assert!(is_package_specifier(Ecosystem::Go, "github.com/x/y"));
    assert!(!is_package_specifier(Ecosystem::Pypi, ""));
}

/// A tech block with valid texts; `extra` adds key lines.
fn tech(id: &str, extra: &str) -> String {
    format!(
        r#"
[[tech]]
id = "{id}"
name = "Name of {id}"
category = "other"
{extra}
en.beginner = "does something useful."
en.builder = "a package that does something useful."
tr.beginner = "işe yarar bir şey yapar."
tr.builder = "işe yarar bir şey yapan bir paket."
"#
    )
}

#[test]
fn broken_entries_are_skipped_with_a_reason() {
    let nineteen = "one two three four five six seven eight nine ten eleven twelve \
                    thirteen fourteen fifteen sixteen seventeen eighteen nineteen.";
    let mut pack = String::new();
    pack += &tech("good-one", r#"npm = ["good-one"]"#);
    pack += &tech("react", r#"npm = ["react"]"#);
    pack += &tech("Bad Id", r#"npm = ["bad-id"]"#);
    pack += &tech("long-en", r#"npm = ["long-en"]"#).replace("does something useful.", nineteen);
    pack += &tech("same-tr", r#"npm = ["same-tr"]"#)
        .replace("işe yarar bir şey yapar.", "does something useful.");
    // Two keys, one of them taken by `react`: the key is dropped, the tech stays.
    pack += &tech("dup-key", r#"npm = ["dup-key", "react"]"#);
    pack += &tech("star-mid", r#"npm = ["star*mid"]"#);
    pack += &tech("empty-one", "");
    pack += &tech("typo", r#"npmm = ["typo"]"#);
    pack += "\n[[config]]\ntech = \"nope\"\nfiles = [\"nope.config.js\"]\n";
    let catalog = Catalog::load(&[("broken.toml", &pack)]);
    let warnings = catalog.warnings();
    assert_eq!(warnings.len(), 8, "{warnings:#?}");
    let has = |needles: &[&str]| {
        let found: Vec<_> = warnings
            .iter()
            .filter(|warning| needles.iter().all(|needle| warning.contains(needle)))
            .collect();
        assert_eq!(found.len(), 1, "{needles:?} in {warnings:#?}");
    };
    has(&["broken.toml", "Bad Id", "skipped"]);
    has(&["long-en", "19 words", "skipped"]);
    has(&["same-tr", "same as", "skipped"]);
    has(&["dup-key", "react", "dropped"]);
    has(&["star-mid", "`*`", "skipped"]);
    has(&["nope", "unknown tech id"]);
    has(&["empty-one", "has nothing that matches it"]);
    has(&["typo", "npmm", "skipped"]);
    // The valid entries still load.
    for id in ["good-one", "react", "dup-key", "empty-one"] {
        assert!(catalog.tech(id).is_some(), "{id} is missing");
    }
    for id in ["Bad Id", "long-en", "same-tr", "star-mid", "typo"] {
        assert!(catalog.tech(id).is_none(), "{id} should be skipped");
    }
    assert_eq!(id(catalog.package(Ecosystem::Npm, "react")), Some("react"));
    assert_eq!(
        catalog.tech("dup-key").unwrap().keys(Ecosystem::Npm),
        ["dup-key"]
    );
}

#[test]
fn duplicate_ids_and_stray_tables_are_reported() {
    let first = tech("same-id", r#"npm = ["one"]"#);
    let second = tech("same-id", r#"npm = ["two"]"#);
    let catalog = Catalog::load(&[
        ("a.toml", &first),
        ("b.toml", &format!("{second}\n[stray]\nx = 1\n")),
        ("c.toml", "this is = not [toml"),
    ]);
    let warnings = catalog.warnings();
    assert_eq!(warnings.len(), 3, "{warnings:#?}");
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("b.toml: tech #1 (same-id) skipped"))
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("b.toml") && w.contains("stray"))
    );
    assert!(warnings.iter().any(|w| w.starts_with("c.toml")));
    assert_eq!(catalog.iter().count(), 1);
    assert!(catalog.package(Ecosystem::Npm, "two").is_none());
}

#[test]
fn config_and_image_entries_can_name_a_tech_from_another_file() {
    let techs = tech("tool", r#"npm = ["tool"]"#);
    let extras = "[[config]]\ntech = \"tool\"\nfiles = [\"tool.rc\", \"a/b\"]\n\
                  [[image]]\ntech = \"tool\"\nnames = [\"Tool\", \"docker.io/library/tool:1\"]\n";
    let catalog = Catalog::load(&[("a.toml", &techs), ("b.toml", extras)]);
    let warnings = catalog.warnings();
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("`a/b`") && w.contains("skipped"))
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("duplicate image") && w.contains("dropped"))
    );
    assert_eq!(id(catalog.image("tool")), Some("tool"));
    assert!(
        catalog.config_file("tool.rc").is_none(),
        "the entry with a bad pattern is skipped"
    );
}

#[test]
fn embedded_pack_loads() {
    let catalog = Catalog::embedded();
    // Warnings are checked in the whole-pack tests; loading must not panic.
    let _ = catalog.warnings();
    assert!(std::ptr::eq(catalog, Catalog::embedded()));
}

#[test]
fn an_empty_pack_file_is_not_a_warning() {
    let catalog = Catalog::load(&[("empty.toml", "# just a header\n"), ("blank.toml", "")]);
    assert!(catalog.warnings().is_empty(), "{:#?}", catalog.warnings());
    assert_eq!(catalog.iter().count(), 0);
}
