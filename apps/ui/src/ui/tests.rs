//! Logic tests for the ui layer (no engine needed).

use super::parts::initials;

#[test]
fn initials_take_two_words() {
    assert_eq!(initials("Ana Popescu"), "AP");
    assert_eq!(initials("Mama"), "M");
    assert_eq!(initials("  "), "?");
    assert_eq!(initials("ioana ștefănescu"), "IȘ");
    assert_eq!(initials("Familia 🏡"), "F");
}

#[test]
fn a_handle_is_never_a_bare_at() {
    assert_eq!(super::parts::handle("ana"), "@ana");
    assert_eq!(super::parts::handle(""), "");
}
