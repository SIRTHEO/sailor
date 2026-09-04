//! **A POWER NOTHING CALLS IS A POWER NOBODY HAS.** The shell declares its
//! commands in one list and the page reaches them by name, and nothing held
//! the two together: `beat_report` stayed declared and unasked for weeks while
//! the beat behind it recorded, every minute, a schedule shown to nobody.

use std::path::{Path, PathBuf};

/// Re-measured exactly when it falls, never raised.
const UNREACHED_TODAY: usize = 0;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

/// The commands the shell hands to the page, from its one declaration.
fn declared(main: &str) -> Vec<String> {
    let opened = match main.find("generate_handler![") {
        Some(at) => at + "generate_handler![".len(),
        None => return Vec::new(),
    };
    let closed = match main[opened..].find(']') {
        Some(at) => opened + at,
        None => return Vec::new(),
    };
    main[opened..closed]
        .split(',')
        .map(|name| name.trim().rsplit("::").next().unwrap_or("").to_owned())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Everything the page ships, tests apart: a command reached only by a test is
/// reached by nobody who uses the product.
fn page(under: &Path, out: &mut String) {
    let Ok(entries) = std::fs::read_dir(under) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "node_modules" {
                page(&path, out);
            }
        } else if (name.ends_with(".ts") || name.ends_with(".tsx")) && !name.contains(".test.") {
            out.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
            out.push('\n');
        }
    }
}

#[test]
fn the_control_a_declaration_is_read_and_a_name_is_found_in_the_page() {
    let list = declared("tauri::generate_handler![flows, live::live_status, run::stop_run]");
    assert_eq!(list, vec!["flows", "live_status", "stop_run"]);
    assert!(declared("nothing declares anything here").is_empty());
}

#[test]
fn no_power_is_declared_and_left_unreachable() {
    let root = root();
    let main = std::fs::read_to_string(root.join("desktop/src-tauri/src/main.rs"))
        .expect("the shell declares its commands here");
    let list = declared(&main);
    assert!(list.len() > 10, "the declaration was not read: {list:?}");

    let mut written = String::new();
    page(&root.join("desktop/src"), &mut written);
    assert!(!written.is_empty(), "the page was not read");

    let unreached: Vec<&String> = list
        .iter()
        .filter(|name| !written.contains(&format!("\"{name}\"")))
        .collect();
    assert_eq!(
        unreached.len(),
        UNREACHED_TODAY,
        "{} of {} commands are declared and never asked for: {unreached:?}",
        unreached.len(),
        list.len()
    );
}
