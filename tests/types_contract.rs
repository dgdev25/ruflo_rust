use ruflo_types::{Capability, CapabilityStatus, RufloError};

#[test]
fn unsupported_capability_has_stable_machine_fields() {
    let capability = Capability::unsupported("workflow.run", 2, "enable Wave 2");
    assert_eq!(capability.status, CapabilityStatus::Unsupported);
    assert_eq!(capability.wave, 2);
    assert!(matches!(
        RufloError::unsupported(capability),
        RufloError::UnsupportedInWave { .. }
    ));
}

#[test]
fn supported_capability_has_no_migration_note() {
    let capability = Capability::supported("memory.search", 1);
    assert_eq!(capability.status, CapabilityStatus::Supported);
    assert_eq!(capability.wave, 1);
    assert_eq!(capability.name, "memory.search");
    assert!(capability.migration.is_none());
}

#[test]
fn capability_roundtrips_through_json() {
    let capability = Capability::unsupported("workflow.run", 2, "enable Wave 2");
    let json = serde_json::to_string(&capability).unwrap();
    let back: Capability = serde_json::from_str(&json).unwrap();
    assert_eq!(capability, back);
}
