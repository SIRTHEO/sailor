//! The window and the inventory name the same families, and now somebody
//! measures it: `FAMILIES` in `desktop/src/Installed.tsx` against `Kind::ALL`,
//! asked of the engine rather than copied, since two hand-written lists drift
//! together — see fault 10. It sits here because `desktop/src-tauri` declares
//! an empty `[workspace]`, so no `cargo test --workspace` compiles what is
//! written there. The price: it reads text, and insists on having read it.

use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

/// The words of `const FAMILIES = [...]`, in the order the window lists them.
/// Order is part of the answer: it is the order of the filter buttons.
fn families_the_window_names(source: &str) -> Vec<String> {
    let from = source
        .find("const FAMILIES")
        .expect("the window declares `const FAMILIES` in desktop/src/Installed.tsx");
    let body = &source[from..];
    let open = body.find('[').expect("`FAMILIES` opens an array");
    let close = body[open..].find(']').expect("`FAMILIES` closes its array");
    body[open + 1..open + close]
        .split(',')
        .map(|word| word.trim().trim_matches('"').to_owned())
        .filter(|word| !word.is_empty())
        .collect()
}

/// **ONE LIST OF FAMILIES, AND THE WINDOW ASKS FOR IT INSTEAD OF KEEPING ITS
/// OWN.** A kind the inventory has and the window does not name has no filter
/// button, and its entries can only be reached by scrolling the whole census;
/// a word the window names and the inventory does not filters to nothing, and
/// an empty list reads as «none installed» rather than as a mistake.
#[test]
fn the_window_names_the_kinds_the_inventory_has() {
    let path = repository_root().join("desktop/src/Installed.tsx");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));

    let named = families_the_window_names(&source);
    assert!(
        named.len() > 1,
        "the window's families were not read: {named:?}. A failed read must be \
         red, not a comparison that quietly succeeded"
    );

    let kinds: Vec<String> = inventory::Kind::ALL
        .into_iter()
        .map(|kind| kind.label().to_owned())
        .collect();

    let extra: Vec<&String> = named.iter().filter(|word| !kinds.contains(word)).collect();
    let missing: Vec<&String> = kinds.iter().filter(|word| !named.contains(word)).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "the window names {extra:?} that the inventory has no kind for, and \
         leaves out {missing:?} that it does have. One of the two lists moved \
         and the other did not: `Kind::ALL` in crates/inventory and `FAMILIES` \
         in desktop/src/Installed.tsx"
    );
    assert_eq!(
        named, kinds,
        "the window lists the same families in another order, and the order is \
         the order of the filter buttons"
    );
}
