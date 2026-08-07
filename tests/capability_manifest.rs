use ruflo_config::CapabilityManifest;
use ruflo_types::{Capability, RufloError};

#[test]
fn wave_two_cannot_be_marked_supported_without_transport_and_auth_fixtures() {
    let manifest =
        CapabilityManifest::from_test_fixture("tests/fixtures/capabilities/wave-2-incomplete.json")
            .unwrap();

    let error = manifest.validate_release(2).unwrap_err();

    assert!(matches!(
        error,
        RufloError::InvalidInput { code, .. } if code == "release.validation.incomplete_evidence"
    ));
    let message = error.to_string();
    assert!(message.contains("workflow.run"));
    assert!(message.contains("auth.token.exchange"));
    assert!(message.contains("security_tests must not be empty"));
    assert!(message.contains("supply_chain_review.sboms must not be empty"));
    assert!(message.contains("adrs must include one ADR record per long-lived integration"));
}

#[test]
fn wave_three_requires_migration_rvf_and_adr_evidence() {
    let manifest =
        CapabilityManifest::from_test_fixture("tests/fixtures/capabilities/wave-3-incomplete.json")
            .unwrap();

    let error = manifest.validate_release(3).unwrap_err();

    assert!(matches!(
        error,
        RufloError::InvalidInput { code, .. } if code == "release.validation.incomplete_evidence"
    ));
    let message = error.to_string();
    assert!(message.contains("migration_tests must not be empty"));
    assert!(message.contains("rvf_tests must not be empty"));
    assert!(message.contains("missing ADR record for long-lived integration `federated-memory`"));
}

#[test]
fn wave_two_release_validation_passes_with_complete_evidence() {
    let manifest =
        CapabilityManifest::from_test_fixture("tests/fixtures/capabilities/wave-2-complete.json")
            .unwrap();

    manifest.validate_release(2).unwrap();
}

#[test]
fn unsupported_capabilities_do_not_require_release_promotion_evidence() {
    let manifest = CapabilityManifest::from_registry(&[ruflo_config::RegisteredCapability::new(
        "workflow_run",
        Capability::unsupported("workflow.run", 2, "enable Wave 2"),
    )]);

    manifest.validate_release(2).unwrap();
}
