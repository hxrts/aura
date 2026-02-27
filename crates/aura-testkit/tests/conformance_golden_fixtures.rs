use aura_testkit::{load_conformance_artifact_file, replay_conformance_artifact};

#[test]
fn golden_conformance_fixtures_load_and_validate() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("conformance");
    let fixture_names = ["consensus", "sync", "recovery", "invitation"];

    for fixture in fixture_names {
        let path = fixture_dir.join(format!("{fixture}.json"));
        assert!(path.exists(), "fixture must exist: {}", path.display());

        let artifact = load_conformance_artifact_file(&path)
            .unwrap_or_else(|err| panic!("failed to load {}: {err}", path.display()));

        let report = replay_conformance_artifact(&artifact)
            .unwrap_or_else(|err| panic!("failed to replay {}: {err}", path.display()));

        // Golden fixtures currently pin structure/surfaces and let digest recompute
        // happen during replay checks.
        assert_eq!(report.step_hash_sets_verified, 0);
        assert!(!report.run_digest_verified);
    }
}
