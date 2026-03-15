use tnav::util::redact::redact_secret;

#[test]
fn short_secrets_are_fully_masked() {
    assert_eq!(redact_secret("secret"), "***");
}

#[test]
fn long_secrets_keep_only_prefix_and_suffix() {
    assert_eq!(redact_secret("  abcdefghijkl  "), "abc...jkl");
}
