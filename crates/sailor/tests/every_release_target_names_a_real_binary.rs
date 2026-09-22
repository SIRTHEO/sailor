//! Every target of `sailor release` names a binary its manifest really builds.
//!
//! **WHY IT EXISTS.** The table once carried three rows and two were fossils:
//! they wanted binaries whose crates had been deleted with the world they
//! served. Neither could build anything, and both found out at the worst
//! moment — inside a clone of `HEAD`, after compiling.
//!
//! **THE LIST OF BINARIES IS ASKED OF CARGO, NOT COPIED HERE.** Two lists
//! written by hand confirm each other even when they are wrong together, which
//! is fault 19. `cargo metadata --no-deps` is the only place that knows what a
//! manifest builds, and it resolves no dependencies, so it needs no network.
//!
//! **WHY HERE AND NOT IN `desktop/src-tauri`.** That package declares an empty
//! `[workspace]`: `cargo test --workspace` does not compile it, and a test
//! written inside it never goes red for anybody. `crates/sailor` is in the
//! workspace and depends on `release`, so the gate runs this one — and from
//! here it asks the shell's manifest too.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate sits in <root>/crates/sailor")
        .to_path_buf()
}

/// The binaries that manifest produces, asked of whoever builds them.
///
/// `--no-deps` limits the answer to the workspace's own members and resolves no
/// dependencies: the narrowest question that answers ours, and no network.
/// **The manifest is the one the target declares**: asking the root for the
/// shell's binaries is asking one workspace about another.
fn binaries_of(manifest_rel: &str) -> Vec<String> {
    let cargo = option_env!("CARGO").unwrap_or("cargo");
    let manifest = repository_root().join(manifest_rel);
    assert!(
        manifest.is_file(),
        "the declared manifest does not exist: {}",
        manifest.display()
    );
    let output = Command::new(cargo)
        .current_dir(repository_root())
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .unwrap_or_else(|error| panic!("cannot ask cargo for its binaries: {error}"));
    assert!(
        output.status.success(),
        "`cargo metadata` failed, so this test looked at nothing: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Value =
        serde_json::from_slice(&output.stdout).expect("`cargo metadata` did not answer in JSON");
    let packages = metadata["packages"]
        .as_array()
        .expect("`cargo metadata` always answers with a list of packages");

    let mut names: Vec<String> = Vec::new();
    for package in packages {
        for target in package["targets"].as_array().into_iter().flatten() {
            let is_binary = target["kind"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|kind| kind == "bin");
            if is_binary {
                names.push(
                    target["name"]
                        .as_str()
                        .expect("a cargo target always has a name")
                        .to_string(),
                );
            }
        }
    }
    names.sort();

    // An empty answer would mean the question changed under our feet, not that
    // the workspace has no binaries: without this line the test would pass by
    // approval instead of by verification, which is fault 22.
    assert!(
        !names.is_empty(),
        "cargo named no binary: the question is no longer the right one, \
         and this test would be approving any table at all"
    );
    names
}

#[test]
fn every_release_target_names_a_binary_the_workspace_really_builds() {
    let mut named_by_cargo = 0;
    for candidate in release::TARGETS {
        // A target built by something else is checked below, against the
        // builder it declares: asking cargo about a Swift package would be
        // asking the wrong witness.
        if let release::Built::ByCommand { inside, .. } = candidate.built {
            let package = repository_root().join(inside);
            assert!(
                package.is_dir(),
                "target '{}' is built in '{inside}', which does not exist: {}",
                candidate.name,
                package.display()
            );
            assert!(
                candidate.live_rel.starts_with(&format!("{inside}/")),
                "target '{}' is built in '{inside}' and looks for the fresh copy \
                 outside it: '{}'",
                candidate.name,
                candidate.live_rel
            );
            continue;
        }
        let binaries = binaries_of(candidate.manifest_rel);
        named_by_cargo += binaries.len();
        assert!(
            binaries.iter().any(|name| name == candidate.bin),
            "target '{}' wants to build the binary '{}', which that manifest does not produce. \
             The real ones, asked of cargo: {}. `sailor release {}` would fail after cloning \
             HEAD and starting the build",
            candidate.name,
            candidate.bin,
            binaries.join(", "),
            candidate.name
        );

        // The fresh copy is looked for where cargo writes it, beside the
        // manifest and not at the root. A target that names the right binary and
        // looks in the wrong place is the same fossil one step further on, and
        // no compiler sees this one either.
        let beside_the_manifest = candidate
            .manifest_rel
            .rsplit_once('/')
            .map(|(directory, _)| format!("{directory}/"))
            .unwrap_or_default();
        let where_cargo_writes =
            format!("{beside_the_manifest}target/release/{}", candidate.bin);
        assert_eq!(
            candidate.live_rel, where_cargo_writes,
            "target '{}' looks for the fresh copy in '{}', but cargo writes it in '{}'",
            candidate.name, candidate.live_rel, where_cargo_writes
        );
    }
    workspace::measured_against(
        named_by_cargo,
        "binaries cargo names across the manifests",
        release::TARGETS.len(),
        "release targets",
    );
}

/// The binary of a bundled target lands inside its own bundle. Two paths that
/// drift apart install the binary beside the bundle and sign an empty one: both
/// gestures succeed, and nothing starts.
#[test]
fn every_bundle_holds_the_binary_it_signs() {
    for candidate in release::TARGETS {
        let Some(bundle) = candidate.bundle else {
            continue;
        };
        assert!(
            candidate
                .safe_rel
                .starts_with(&format!("{}/", bundle.root_rel)),
            "target '{}' installs its binary in '{}', outside the bundle '{}' it signs",
            candidate.name,
            candidate.safe_rel,
            bundle.root_rel
        );
        assert!(
            !bundle.signed_by.is_empty(),
            "target '{}' declares no signature: the system refuses to start an unsigned bundle",
            candidate.name
        );
        for (from, _) in bundle.carries {
            let carried = repository_root().join(from);
            assert!(
                carried.is_file(),
                "target '{}' carries '{from}' into its bundle, and the tree has no such file: {}",
                candidate.name,
                carried.display()
            );
        }
    }
}

/// **A BUNDLE STARTS THE FILE ITS PLIST NAMES.** `CFBundleExecutable` is what
/// macOS looks for inside `Contents/MacOS`; when it names anything else the
/// bundle is assembled, signed and installed, and opening it does nothing at
/// all — the same shape of silence as a dot without `LSUIElement`.
#[test]
fn every_bundle_names_the_binary_it_starts() {
    let mut checked = 0;
    for candidate in release::TARGETS {
        let Some(bundle) = candidate.bundle else {
            continue;
        };
        let Some((plist_rel, _)) = bundle
            .carries
            .iter()
            .find(|(_, into)| *into == "Contents/Info.plist")
        else {
            panic!(
                "target '{}' is a bundle carrying no Info.plist: without one the system does not \
                 know what to start",
                candidate.name
            );
        };
        let plist = std::fs::read_to_string(repository_root().join(plist_rel))
            .unwrap_or_else(|error| panic!("cannot read {plist_rel}: {error}"));
        let starts = after_the_key(&plist, "CFBundleExecutable").unwrap_or_else(|| {
            panic!("target '{}' carries a plist naming no executable", candidate.name)
        });
        let installed = candidate
            .safe_rel
            .rsplit('/')
            .next()
            .expect("a path has a last segment");
        assert_eq!(
            starts, installed,
            "target '{}' installs '{}' and its plist starts '{starts}': opening the bundle would \
             do nothing",
            candidate.name, candidate.safe_rel
        );
        let holds_the_binary = candidate
            .safe_rel
            .starts_with(&format!("{}/Contents/MacOS/", bundle.root_rel));
        assert!(
            holds_the_binary,
            "target '{}' installs its binary at '{}', where the plist's name is not looked for",
            candidate.name, candidate.safe_rel
        );
        checked += 1;
    }
    assert!(checked > 0, "no bundled target: this test guards nothing");
}

/// The string of the `<string>` following a `<key>`, which is all this file
/// needs of a property list.
fn after_the_key(plist: &str, key: &str) -> Option<String> {
    let at = plist.find(&format!("<key>{key}</key>"))? + key.len() + 11;
    let rest = &plist[at..];
    let opens = rest.find("<string>")? + "<string>".len();
    let closes = rest.find("</string>")?;
    Some(rest[opens..closes].trim().to_string())
}

/// The page a target declares exists, and carries its `package.json`. Without
/// this, `page_rel` is a string the way `bin` was one, and the release finds
/// out inside the clone of HEAD after it has already compiled.
#[test]
fn every_page_a_target_declares_is_a_real_one() {
    for candidate in release::TARGETS {
        let Some(page) = candidate.page_rel else {
            continue;
        };
        let manifest = repository_root().join(page).join("package.json");
        assert!(
            manifest.is_file(),
            "target '{}' declares the page '{page}', which carries no package.json: {}",
            candidate.name,
            manifest.display()
        );
    }
}
