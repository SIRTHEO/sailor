//! The page cuts a long `reference` note and points at `sailor memory show`:
//! only the real binary can prove the pointer leads to the whole note.

use std::path::PathBuf;
use std::process::{Command, Output};

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sailor-show-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch home");
    dir
}

fn sailor(home: &PathBuf, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sailor"))
        .args(args)
        .env("SAILOR_HOME", home)
        .env("SAILOR_LEDGER", home.join("ledger"))
        .current_dir(home)
        .output()
        .expect("run the real command")
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_pointer_on_the_page_leads_to_the_whole_note() {
    let home = home("whole");
    let tail = "the tail that only the note holds";
    let body = format!("The lab answers on one port. {} {tail}", "Filler sentence. ".repeat(30));
    let kept = sailor(&home, &["remember", "--global", "reference", "lab-port", &body]);
    assert!(kept.status.success(), "{}", String::from_utf8_lossy(&kept.stderr));

    let page = said(&sailor(&home, &["memory", "page", "--print"]));
    assert!(page.contains("sailor memory show lab-port"), "{page}");
    assert!(!page.contains(tail), "{page}");

    let shown = sailor(&home, &["memory", "show", "lab-port"]);
    assert!(shown.status.success(), "{}", String::from_utf8_lossy(&shown.stderr));
    assert!(said(&shown).contains(tail), "{}", said(&shown));
}

#[test]
fn a_label_nobody_holds_is_refused_by_name() {
    let home = home("none");
    let kept = sailor(&home, &["remember", "--global", "reference", "another-note", "Short."]);
    assert!(kept.status.success(), "{}", String::from_utf8_lossy(&kept.stderr));
    let shown = sailor(&home, &["memory", "show", "nobody-holds-this"]);
    assert_eq!(shown.status.code(), Some(1));
    let refusal = String::from_utf8_lossy(&shown.stderr).into_owned();
    assert!(refusal.contains("nobody-holds-this"), "{refusal}");
}
