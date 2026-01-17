use forgejo_cli::localization::bundles;

use forgejo_cli::ftl_format;

#[test]
fn localization_fallback() {
    let _ = bundles::LOCALE.set(bundles::init_to("tok"));

    let s = ftl_format!("test-fallback-only-english");
    assert_eq!(s, "This message is only in english");
}
