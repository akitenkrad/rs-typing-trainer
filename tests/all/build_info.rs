use rs_typing_trainer::build_info;

#[test]
fn short_version_starts_with_the_package_version() {
    assert!(build_info::short_version().starts_with(concat!("v", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn short_version_omits_an_unknown_hash() {
    assert_eq!(build_info::short_version_for("unknown"), concat!("v", env!("CARGO_PKG_VERSION")));
}

#[test]
fn build_date_is_an_iso_calendar_date() {
    let date = build_info::build_date();
    let bytes = date.as_bytes();
    assert_eq!(bytes.len(), 10, "{date}");
    assert_eq!(bytes[4], b'-', "{date}");
    assert_eq!(bytes[7], b'-', "{date}");
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit()),
        "{date}"
    );
}
