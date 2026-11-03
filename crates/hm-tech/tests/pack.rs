use hm_tech::{Catalog, Category, Ecosystem, PACK_FILES};

/// Each file is checked alone, so a data writer can run this on their own file. References
/// to a tech from another file and techs only matched from another file are expected.
#[test]
fn each_pack_file_is_valid_on_its_own() {
    for entry in PACK_FILES {
        let catalog = Catalog::load(&[*entry]);
        let problems: Vec<_> = catalog
            .warnings()
            .iter()
            .filter(|warning| {
                !warning.contains("unknown tech id")
                    && !warning.contains("has nothing that matches it")
            })
            .collect();
        assert!(problems.is_empty(), "{}: {problems:#?}", entry.0);
    }
}

#[test]
fn there_are_twelve_pack_files() {
    assert_eq!(PACK_FILES.len(), 12);
}

#[test]
fn embedded_pack_has_no_warnings() {
    let warnings = Catalog::embedded().warnings();
    assert!(warnings.is_empty(), "{warnings:#?}");
}

/// The "quality beats count" floors; a lower number needs a ruling.
#[test]
fn pack_size_floors() {
    let catalog = Catalog::embedded();
    let floors = [
        (Ecosystem::Npm, 270),
        (Ecosystem::Pypi, 100),
        (Ecosystem::Go, 65),
        (Ecosystem::Maven, 50),
        (Ecosystem::Nuget, 35),
        (Ecosystem::Crates, 55),
    ];
    for (ecosystem, floor) in floors {
        let count = catalog.key_count(ecosystem);
        assert!(
            count >= floor,
            "{} has {count} keys, floor is {floor}",
            ecosystem.as_str()
        );
    }
}

#[test]
fn real_lookups() {
    let catalog = Catalog::embedded();
    let package = |ecosystem, name| {
        catalog
            .package(ecosystem, name)
            .map(|tech| tech.id.as_str())
    };
    assert_eq!(
        package(Ecosystem::Npm, "@radix-ui/react-dialog"),
        Some("radix-ui")
    );
    assert_eq!(package(Ecosystem::Npm, "react-dom"), Some("react"));
    assert_eq!(
        package(Ecosystem::Npm, "@types/node"),
        Some("type-definitions")
    );
    assert_eq!(package(Ecosystem::Npm, "next"), Some("nextjs"));
    assert_eq!(package(Ecosystem::Pypi, "Django"), Some("django"));
    assert_eq!(
        catalog
            .package(Ecosystem::Pypi, "Django")
            .map(|tech| tech.name.as_str()),
        Some("Django")
    );
    assert_eq!(
        package(Ecosystem::Go, "github.com/jackc/pgx/v5"),
        Some("postgresql")
    );
    assert_eq!(
        package(Ecosystem::Go, "gorm.io/driver/postgres"),
        Some("gorm")
    );
    assert_eq!(
        package(
            Ecosystem::Maven,
            "org.springframework.boot:spring-boot-starter-web"
        ),
        Some("spring-boot")
    );
    assert_eq!(
        package(
            Ecosystem::Maven,
            "org.springframework.boot:spring-boot-starter-security"
        ),
        Some("spring-security")
    );
    assert_eq!(
        package(Ecosystem::Nuget, "microsoft.entityframeworkcore.sqlserver"),
        Some("sqlserver")
    );
    assert_eq!(package(Ecosystem::Crates, "serde_json"), Some("serde-json"));
    assert_eq!(
        catalog
            .config_file("apps/web/next.config.ts")
            .map(|tech| tech.id.as_str()),
        Some("nextjs")
    );
    assert_eq!(
        catalog
            .image("postgres:16-alpine")
            .map(|tech| tech.id.as_str()),
        Some("postgresql")
    );
    assert_eq!(
        catalog.language("tsx").map(|tech| tech.id.as_str()),
        Some("typescript")
    );
    assert_eq!(package(Ecosystem::Npm, "left-pad"), None);
}

#[test]
fn every_category_is_used() {
    let catalog = Catalog::embedded();
    for category in Category::ALL {
        assert!(
            catalog.iter().any(|tech| tech.category == category),
            "no tech in category {}",
            category.as_str()
        );
    }
}
