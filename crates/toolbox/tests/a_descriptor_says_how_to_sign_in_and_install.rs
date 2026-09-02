//! A descriptor can say how a person signs a command line in and how it is
//! installed; one that says nothing is read as «nobody measured», never as a
//! default gesture.

use toolbox::descriptor::Descriptor;

fn parsed(text: &str) -> Descriptor {
    serde_json::from_str(text).expect("a descriptor parses")
}

#[test]
fn a_descriptor_without_the_two_blocks_declares_neither() {
    // The control first: silence is not a gesture.
    let bare = parsed(r#"{"id": "x", "family": "ai_cli"}"#);
    assert!(bare.login.is_none(), "{:?}", bare.login);
    assert!(bare.install.is_none(), "{:?}", bare.install);
}

#[test]
fn the_sign_in_and_the_install_line_are_read_back_as_written() {
    let said = parsed(
        r#"{"id": "x", "family": "ai_cli",
            "login": {"args": ["auth", "login"], "interactive": true, "note": "measured"},
            "install": {"line": "brew install x"}}"#,
    );
    let login = said.login.clone().expect("the sign-in");
    assert_eq!(login.args, vec!["auth", "login"]);
    assert!(login.interactive);
    assert_eq!(login.note, "measured");
    let install = said.install.clone().expect("the install line");
    assert_eq!(install.line, "brew install x");
    assert_eq!(install.note, "");
    // And it survives a round trip: a rewrite keeps what was measured.
    let again: Descriptor =
        serde_json::from_str(&serde_json::to_string(&said).expect("serialises")).expect("parses");
    assert_eq!(again, said);
}

#[test]
fn the_shipped_engines_declare_how_they_sign_in() {
    let shipped: serde_json::Value =
        serde_json::from_str(include_str!("../descriptors/default.json")).expect("the shipped list");
    let entries = shipped["tools"]
        .as_array()
        .or_else(|| shipped.as_array())
        .expect("a list of descriptors");
    let mut with_login = Vec::new();
    for entry in entries {
        let descriptor: Descriptor = serde_json::from_value(entry.clone()).expect("each entry parses");
        if descriptor.login.is_some() {
            with_login.push(descriptor.id);
        }
    }
    for expected in ["claude-code", "codex", "gemini-cli", "agy"] {
        assert!(with_login.contains(&expected.to_owned()), "{expected} declares no sign-in: {with_login:?}");
    }
}
