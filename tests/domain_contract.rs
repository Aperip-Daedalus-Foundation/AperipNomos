use aperip_nomos::domain::{LicenseDraft, LicenseValidationError, MAX_LICENSE_BYTES};

#[test]
fn derives_identity_and_digest_from_filename() {
    let draft = LicenseDraft::from_upload(
        "Apache License 2.0.txt",
        None,
        None,
        b"Apache License\nVersion 2.0\n",
        1_753_680_000_000,
    )
    .expect("valid license");

    assert_eq!(draft.slug(), "apache-license-2-0");
    assert_eq!(draft.title(), "Apache License 2.0");
    assert_eq!(draft.source_filename(), "Apache License 2.0.txt");
    assert_eq!(draft.uploaded_at_ms(), 1_753_680_000_000);
    assert_eq!(draft.sha256().len(), 64);
}

#[test]
fn accepts_explicit_title_and_normalized_slug() {
    let draft = LicenseDraft::from_upload(
        "COPYING",
        Some("Aperip Community License"),
        Some("Aperip Community License"),
        b"permission is hereby granted",
        7,
    )
    .expect("valid license");

    assert_eq!(draft.title(), "Aperip Community License");
    assert_eq!(draft.slug(), "aperip-community-license");
}

#[test]
fn uses_digest_slug_when_filename_has_no_ascii_words() {
    let draft = LicenseDraft::from_upload("许可证.txt", None, None, "许可正文".as_bytes(), 9)
        .expect("valid license");

    assert!(draft.slug().starts_with("license-"));
    assert_eq!(draft.slug().len(), 20);
    assert_eq!(draft.title(), "许可证");
}

#[test]
fn rejects_empty_invalid_and_oversized_files() {
    assert_eq!(
        LicenseDraft::from_upload("MIT.txt", None, None, b" \r\n\t", 1),
        Err(LicenseValidationError::EmptyBody)
    );
    assert_eq!(
        LicenseDraft::from_upload("MIT.txt", None, None, &[0xff], 1),
        Err(LicenseValidationError::InvalidUtf8)
    );
    assert_eq!(
        LicenseDraft::from_upload("MIT.txt", None, None, &vec![b'a'; MAX_LICENSE_BYTES + 1], 1),
        Err(LicenseValidationError::BodyTooLarge)
    );
}

#[test]
fn rejects_unsafe_explicit_slug() {
    assert_eq!(
        LicenseDraft::from_upload("MIT.txt", None, Some("../../MIT"), b"MIT", 1),
        Err(LicenseValidationError::InvalidSlug)
    );
}
