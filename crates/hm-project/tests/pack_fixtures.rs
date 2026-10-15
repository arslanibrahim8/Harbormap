//! Every signature in the embedded packs ships with fixtures; each must match it.

use hm_doctor::packs::load_embedded;
use hm_project::matcher::{ProcFacts, match_image, match_process};

#[test]
fn every_signature_fixture_matches_its_own_signature() {
    let packs = load_embedded();
    let mut failures = Vec::new();
    for signature in packs.signatures.iter() {
        for (index, fixture) in signature.fixture.iter().enumerate() {
            let os = fixture
                .os
                .as_deref()
                .or(signature.os.as_deref())
                .unwrap_or("linux");
            let found = match &fixture.image {
                Some(image) => match_image(&packs.signatures, image, os),
                None => match_process(&packs.signatures, &ProcFacts::from_fixture(fixture, os)),
            };
            let got = found.map(|found| found.signature.id.as_str());
            if got != Some(signature.id.as_str()) {
                failures.push(format!(
                    "{} fixture #{} matched {got:?}",
                    signature.id,
                    index + 1
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn os_specific_signatures_stay_on_their_os() {
    let packs = load_embedded();
    let facts = ProcFacts {
        exe: "controlcenter".into(),
        name: "controlcenter".into(),
        os: "linux".into(),
        ..ProcFacts::default()
    };
    assert!(match_process(&packs.signatures, &facts).is_none());
}
