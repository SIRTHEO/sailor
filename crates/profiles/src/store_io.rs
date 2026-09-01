//! The impure gestures of `profiles`: environment, disk, symlinks. The pure
//! part stays in `lib.rs`; this is the only file that touches the world.

use crate::{parse_store, serialize_store, symlink_swap, ProfileStore};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `HOME`, or a readable error if it is not set.
pub fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_owned())
}

fn default_state_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".claude")
        .join("state")
        .join("profili.json")
}

/// `PROFILES_STATE_PATH` when set, otherwise `~/.claude/state/profili.json`.
///
/// The filename is data, not language: it names state already written on disk,
/// and renaming it would orphan the profiles somebody already has.
pub fn state_path() -> PathBuf {
    env::var_os("PROFILES_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_state_path)
}

/// `PROFILES_HOME_ROOT` when set, otherwise `profiles-homes/` beside the state.
pub fn profiles_root() -> PathBuf {
    env::var_os("PROFILES_HOME_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            state_path()
                .parent()
                .map(|p| p.join("profiles-homes"))
                .unwrap_or_else(|| PathBuf::from("profiles-homes"))
        })
}

pub fn load_store_from(path: &Path) -> Result<ProfileStore, String> {
    match fs::read_to_string(path) {
        Ok(content) => parse_store(&content)
            .map_err(|e| format!("unreadable state in {}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ProfileStore::default()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// The directory to create before writing, or nothing when there is none.
///
/// For a path with no directory in front, `parent()` answers `Some("")`, and
/// `create_dir_all("")` fails with «no such file» — the save would refuse while
/// holding permission on the current directory. A function of its own so the
/// test can judge the decision without `set_current_dir`, which is per
/// **process**: while it ran, parallel tests wrote elsewhere and fell.
fn parent_to_create(path: &Path) -> Option<&Path> {
    path.parent().filter(|p| !p.as_os_str().is_empty())
}

pub fn save_store_to(path: &Path, store: &ProfileStore) -> Result<(), String> {
    if let Some(parent) = parent_to_create(path) {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let json = serialize_store(store).map_err(|e| format!("serialisation failed: {e}"))?;
    fs::write(path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn load_store() -> Result<ProfileStore, String> {
    load_store_from(&state_path())
}

pub fn save_store(store: &ProfileStore) -> Result<(), String> {
    save_store_to(&state_path(), store)
}

/// Moves the link onto `profile_home` without ever touching a real file: it
/// refuses when `link_path` is not already a symlink, and requires the profile
/// to have its own credentials rather than fabricating empty ones a command line
/// would take for real. The profile being left keeps everything, because its
/// file is never opened for writing.
pub fn apply_symlink_swap(
    fixed_home: &Path,
    relative_path: &str,
    profile_home: &Path,
) -> Result<(), String> {
    let swap = symlink_swap(fixed_home, relative_path, profile_home);
    if !swap.target_path.exists() {
        return Err(format!(
            "{} does not exist yet: this profile has no credentials to link to",
            swap.target_path.display()
        ));
    }
    match fs::symlink_metadata(&swap.link_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::remove_file(&swap.link_path).map_err(|e| {
                format!(
                    "cannot remove the old link {}: {e}",
                    swap.link_path.display()
                )
            })?;
        }
        Ok(_) => {
            return Err(format!(
                "{} is not a symlink: the swap stops rather than lose real credentials",
                swap.link_path.display()
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot read {}: {e}", swap.link_path.display())),
    }
    if let Some(parent) = swap.link_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::os::unix::fs::symlink(&swap.target_path, &swap.link_path)
        .map_err(|e| format!("cannot link {}: {e}", swap.link_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Profile;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A throwaway directory under `$TMPDIR`, removed when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            // **THE COUNTER IS NOT SPARE: without it these tests stole each
            // other's directory.** `cargo test` runs a crate's tests on threads
            // of the **same** process, so the pid is identical for all of them,
            // and macOS's clock has no real nanosecond resolution — two tests
            // starting together got the same name, and the first to finish
            // deleted the other's directory on its way out.
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let unique = format!(
                "profiles-test-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let path = env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("test directory");
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn store_roundtrip_survives_disk() {
        let dir = TempDir::new();
        let path = dir.path().join("state").join("profili.json");
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "work".to_owned(),
            cli_id: "claude".to_owned(),
            home_dir: dir.path().join("claude").join("work"),
        });
        store.active.insert("claude".to_owned(), "work".to_owned());

        save_store_to(&path, &store).expect("saving");
        let reloaded = load_store_from(&path).expect("reloading");
        assert_eq!(reloaded, store);
    }

    #[test]
    fn load_store_from_missing_file_is_an_empty_store() {
        let dir = TempDir::new();
        let path = dir.path().join("does-not-exist.json");
        assert_eq!(
            load_store_from(&path).expect("no error on a missing file"),
            ProfileStore::default()
        );
    }

    /// The test that matters for a quick swap: the profile being left must come
    /// out of it with its credentials intact.
    #[test]
    fn switching_profiles_never_loses_the_one_left_behind() {
        let dir = TempDir::new();
        let fixed_home = dir.path().join("fixed-home");
        let profile_a = dir.path().join("profiles").join("acme").join("a");
        let profile_b = dir.path().join("profiles").join("acme").join("b");
        fs::create_dir_all(&profile_a).unwrap();
        fs::create_dir_all(&profile_b).unwrap();
        let relative = "credentials.json";
        fs::write(profile_a.join(relative), "credentials-a").unwrap();
        fs::write(profile_b.join(relative), "credentials-b").unwrap();

        apply_symlink_swap(&fixed_home, relative, &profile_a).expect("swap onto a");
        assert_eq!(
            fs::read_to_string(fixed_home.join(relative)).unwrap(),
            "credentials-a"
        );

        apply_symlink_swap(&fixed_home, relative, &profile_b).expect("swap onto b");
        assert_eq!(
            fs::read_to_string(fixed_home.join(relative)).unwrap(),
            "credentials-b"
        );

        assert_eq!(
            fs::read_to_string(profile_a.join(relative)).unwrap(),
            "credentials-a",
            "the profile left behind lost its credentials"
        );
    }

    #[test]
    fn apply_symlink_swap_refuses_to_clobber_a_real_file() {
        let dir = TempDir::new();
        let fixed_home = dir.path().join("fixed-home");
        fs::create_dir_all(&fixed_home).unwrap();
        let relative = "credentials.json";
        fs::write(fixed_home.join(relative), "real-credentials").unwrap();

        let profile_a = dir.path().join("profiles").join("acme").join("a");
        fs::create_dir_all(&profile_a).unwrap();
        fs::write(profile_a.join(relative), "credentials-a").unwrap();

        let result = apply_symlink_swap(&fixed_home, relative, &profile_a);
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(fixed_home.join(relative)).unwrap(),
            "real-credentials",
            "a real file was touched instead of being refused"
        );
    }

    #[test]
    fn apply_symlink_swap_refuses_a_profile_without_credentials_yet() {
        let dir = TempDir::new();
        let fixed_home = dir.path().join("fixed-home");
        let profile_a = dir.path().join("profiles").join("acme").join("a");
        fs::create_dir_all(&profile_a).unwrap();

        let result = apply_symlink_swap(&fixed_home, "credentials.json", &profile_a);
        assert!(result.is_err());
    }

    /// A path with no directory in front: `parent()` answers with the empty
    /// string, and `create_dir_all` refuses it despite the permission being
    /// there. The three arms are the proof — drop the empty filter in
    /// `parent_to_create` and the first goes red.
    #[test]
    fn a_bare_filename_has_no_directory_to_create() {
        assert_eq!(parent_to_create(Path::new("profili.json")), None);
        assert_eq!(
            parent_to_create(Path::new("state/profili.json")),
            Some(Path::new("state"))
        );
        assert_eq!(
            parent_to_create(Path::new("/tmp/state/profili.json")),
            Some(Path::new("/tmp/state"))
        );
    }

    /// And the save really works, checked where no other test is looking: a
    /// directory of its own, with an absolute path.
    #[test]
    fn the_store_is_written_where_it_is_asked_to_be() {
        let dir = TempDir::new();
        let path = dir.path().join("inside").join("profili.json");
        save_store_to(&path, &ProfileStore::default()).expect("the save must succeed");
        assert!(path.exists());
    }
}
