use forgejo_cli::localization::bundles;

use forgejo_cli::{ftl_eprintln, ftl_format, ftl_println, ftl_write};

#[test]
fn localization_macros() {
    let _ = bundles::LOCALE.set(bundles::init_to("en-US"));

    let s = ftl_format!("test-hello");
    assert_eq!(s, "Hello");

    let mut s = String::new();
    ftl_write!(&mut s, "test-hello");
    assert_eq!(s, "Hello");

    ftl_println!("test-hello");
    ftl_eprintln!("test-hello");
}
