//! What Sailor keeps, where it actually lives, and how much room it takes.
//!
//! **A THING WHOSE PLACE YOU DO NOT KNOW IS A THING YOU DO NOT CONTROL.** Each
//! row names a store with its path on disk, how many things are in it and how
//! many bytes; a store that does not exist yet says so instead of showing a
//! plausible zero, because a missing store shown as empty tells a lie.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Store {
    pub what: String,
    pub r#where: String,
    /// How many things: flow files, rows, profiles. `None` when the store is
    /// not there yet.
    pub how_many: Option<u64>,
    pub bytes: Option<u64>,
    pub exists: bool,
}

/// The binary the hooks and the terminals call, and what it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InService {
    pub binary: Option<String>,
    pub built_at: Option<i64>,
    /// The commit the release stamped, read from the home's stamp file.
    pub commit: Option<String>,
    pub window_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Keeps {
    pub home: String,
    pub home_files: u64,
    pub home_bytes: u64,
    pub stores: Vec<Store>,
    pub in_service: InService,
    pub project_root: Option<String>,
}

/// Files and bytes under a directory, walked without following links.
pub(crate) fn measure(path: &Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                pending.push(entry.path());
            } else if let Ok(meta) = entry.metadata() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}

fn flow_files(directory: &Path) -> Option<(u64, u64)> {
    if !directory.is_dir() {
        return None;
    }
    let mut count = 0;
    let mut bytes = 0;
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        if path.to_string_lossy().ends_with(".flow.json") {
            count += 1;
            bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        }
    }
    Some((count, bytes))
}

fn store_of(what: &str, path: &Path, how_many: Option<u64>) -> Store {
    let exists = path.exists();
    let bytes = if !exists {
        None
    } else if path.is_dir() {
        Some(measure(path).1)
    } else {
        std::fs::metadata(path).ok().map(|meta| meta.len())
    };
    Store {
        what: what.to_owned(),
        r#where: path.display().to_string(),
        how_many: if exists { how_many } else { None },
        bytes,
        exists,
    }
}

/// The stores, given the home and the flow sources: pure over the disk, so a
/// test can hand it a scratch home and read the rows back.
pub(crate) fn stores_in(home: &Path, sources: &[ui::gather::FlowSource], ledger_dir: &Path) -> Vec<Store> {
    let mut stores = Vec::new();
    for source in sources {
        let counted = flow_files(&source.dir);
        stores.push(Store {
            what: format!("Flows, {}", source.origin),
            r#where: source.dir.display().to_string(),
            how_many: counted.map(|(count, _)| count),
            bytes: counted.map(|(_, bytes)| bytes),
            exists: counted.is_some(),
        });
    }
    let ledger_rows = ui::gather::ledger_present(ledger_dir)
        .then(|| ledger::Ledger::open(ledger_dir).ok())
        .flatten()
        .and_then(|ledger| ledger.tables().ok())
        .map(|tables| tables.iter().map(|table| table.rows.max(0) as u64).sum());
    stores.push(store_of("Runs, steps and events", ledger_dir, ledger_rows));
    let faults = ledger_dir.join("faults.db");
    stores.push(store_of("Faults", &faults, None));
    let profiles = profiles::store_io::state_path();
    let profile_count = profiles::store_io::load_store_from(&profiles)
        .ok()
        .map(|store| store.profiles.len() as u64);
    stores.push(store_of("Profiles", &profiles, profile_count));
    stores.push(store_of("Profile homes", &home.join("profiles-homes"), None));
    stores.push(store_of("Prices", &home.join("pricing.json"), None));
    stores.push(store_of("Terminals held", &ledger_dir.join("terminals"), None));
    stores
}

fn stamp_in(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join("state").join("sailor-binary-commit")).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

fn binary_in_service() -> (Option<PathBuf>, Option<i64>) {
    let path = match std::env::var_os("SAILOR_BIN").filter(|value| !value.is_empty()) {
        Some(declared) => Some(PathBuf::from(declared)),
        None => match toolbox::probe::look_up("sailor", &toolbox::Machine::current()) {
            toolbox::probe::Look::Found(path) => Some(path),
            _ => None,
        },
    };
    let built_at = path.as_ref().and_then(|path| {
        std::fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs() as i64)
    });
    (path, built_at)
}

#[tauri::command]
pub(crate) fn what_sailor_keeps() -> Result<Keeps, String> {
    let home = ledger::sailor_home().ok_or_else(|| "no home: HOME is not set".to_owned())?;
    let (home_files, home_bytes) = measure(&home);
    let ledger_dir = ui::gather::default_ledger_dir();
    let stores = stores_in(&home, &ui::gather::flow_sources(), &ledger_dir);
    let (binary, built_at) = binary_in_service();
    let project_root = std::env::current_dir()
        .ok()
        .and_then(|working| flow::workspace::find_root(&working))
        .map(|root| root.display().to_string());
    Ok(Keeps {
        home: home.display().to_string(),
        home_files,
        home_bytes,
        stores,
        in_service: InService {
            binary: binary.map(|path| path.display().to_string()),
            built_at,
            commit: stamp_in(&home),
            window_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        project_root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A STORE THAT IS NOT THERE SAYS SO**, with no count and no size, and a
    /// store that is there is counted from the disk: two flow files are two,
    /// and a home with three files measures three.
    #[test]
    fn what_is_kept_is_read_from_the_disk_and_a_missing_store_is_said() {
        let scratch = std::env::temp_dir().join(format!("sailor-keeps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let flows = scratch.join("flows");
        std::fs::create_dir_all(&flows).expect("scratch");
        std::fs::write(flows.join("one.flow.json"), "{}").expect("write");
        std::fs::write(flows.join("two.flow.json"), "{\"id\":1}").expect("write");
        std::fs::write(flows.join("notes.md"), "not a flow").expect("write");
        std::fs::write(scratch.join("pricing.json"), "[]").expect("write");

        assert_eq!(measure(&scratch), (4, 2 + 8 + 10 + 2));

        let sources = vec![
            ui::gather::FlowSource {
                origin: "yours",
                dir: flows.clone(),
            },
            ui::gather::FlowSource {
                origin: "the project's",
                dir: scratch.join("nowhere"),
            },
        ];
        let stores = stores_in(&scratch, &sources, &scratch.join("ledger"));
        let by_what = |what: &str| stores.iter().find(|store| store.what == what).expect(what).clone();

        let yours = by_what("Flows, yours");
        assert_eq!((yours.exists, yours.how_many, yours.bytes), (true, Some(2), Some(10)));
        let project = by_what("Flows, the project's");
        assert_eq!((project.exists, project.how_many, project.bytes), (false, None, None));
        let ledger = by_what("Runs, steps and events");
        assert!(!ledger.exists && ledger.how_many.is_none(), "{ledger:?}");
        let prices = by_what("Prices");
        assert_eq!((prices.exists, prices.bytes), (true, Some(2)));

        assert_eq!(stamp_in(&scratch), None);
        std::fs::create_dir_all(scratch.join("state")).expect("state");
        std::fs::write(scratch.join("state/sailor-binary-commit"), "abc123\n").expect("stamp");
        assert_eq!(stamp_in(&scratch).as_deref(), Some("abc123"));
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
