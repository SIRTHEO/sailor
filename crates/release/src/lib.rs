//! Release decisions, separated from the gestures that carry them out.
//!
//! `cargo build --release` builds from the working tree, not from a commit, so
//! rebuilding puts everybody's uncommitted lines into service. This crate holds
//! what can be got wrong silently — which targets exist, which commit produced
//! the binary in service, and whether it can be replaced right now. Cloning,
//! building, copying and restarting live in `main.rs`.

/// A resident service: some need a restart for the replacement to take effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Service {
    /// Default launchd label for `launchctl kickstart -k gui/<uid>/<label>`.
    /// A property of the installation, not of the code: `RELEASE_SERVICE_LABEL`
    /// overrides it.
    pub label: &'static str,
    /// Where the service leaves the receipt of the task it is holding, relative
    /// to `~/.claude`. A restart in the middle leaves a task neither done nor
    /// queued.
    pub in_progress_rel: &'static str,
}

/// What gets released. Paths are relative to a root, never absolute: a table
/// carrying one machine's paths could not be tested anywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// How it is named on the command line.
    pub name: &'static str,
    /// The `cargo` target to build.
    pub bin: &'static str,
    /// The manifest that builds it, relative to the **sources**: a build
    /// launched at the root never enters a nested workspace.
    pub manifest_rel: &'static str,
    /// The directory of the page the binary embeds at compile time — the one
    /// holding `package.json` — or `None` for a binary with no page.
    pub page_rel: Option<&'static str>,
    /// The cargo features the build has to be given, beyond the defaults.
    ///
    /// **A DEFAULT BUILD OF A SHELL IS A DEVELOPMENT BUILD**: its own build
    /// script reads `dev = !custom-protocol`, so without that feature it embeds
    /// no page and looks for a development server that answers nobody once the
    /// binary is in service. Fault 77.
    pub features: &'static [&'static str],
    /// The copy inside the build tree, relative to the **sources** — the one a
    /// local `cargo build` overwrites.
    pub live_rel: &'static str,
    /// The copy in service, relative to the **configuration home**, outside
    /// `target/` where no build reaches it. Hooks name this one, so building
    /// puts nothing into service by construction rather than by vigilance.
    ///
    /// The two roots are not the same one, and getting them mixed up writes the
    /// binary where nobody runs it.
    pub safe_rel: &'static str,
    /// The file naming the commit the binary in service was built from,
    /// relative to the home. It is the only place that fact exists.
    pub stamp_rel: &'static str,
    /// The tree the suite last passed on, relative to the home.
    pub suite_memo_rel: &'static str,
    /// `None` for anything reborn on every call.
    pub service: Option<Service>,
}

/// The targets that exist. `every_release_target_names_a_real_binary` asks
/// `cargo metadata` whether each `bin` is real, because `bin` is a string and a
/// table naming a deleted crate still compiles.
///
/// Whether a one-row table should stay a table is Theo's call: `Service`,
/// `service_domain` and the readiness wait currently have no target using them.
pub const TARGETS: &[Target] = &[
    // Whoever puts the others into service must be able to do it for itself:
    // otherwise the only way to install `sailor` is copying a binary by hand,
    // which is the gesture this table exists to remove.
    //
    // No service to restart — but "no service" does not mean "safe to
    // overwrite". Rewriting a memory-mapped executable in place invalidates its
    // signature for every running process, which is why the release writes
    // beside the file and renames.
    Target {
        name: "sailor",
        bin: "sailor",
        manifest_rel: ROOT_MANIFEST,
        page_rel: None,
        features: &[],
        live_rel: "target/release/sailor",
        safe_rel: "bin/sailor",
        stamp_rel: "state/sailor-binary-commit",
        suite_memo_rel: "state/sailor-suite-tree",
        service: None,
    },
    // The window, until now only launchable from inside `target/`, which a
    // `cargo clean` empties. Three things differ from the row above, all
    // declared: a workspace of its own, a page to build first, and cargo
    // writing beside that shell instead of at the root.
    Target {
        name: "window",
        bin: "sailor-desktop",
        manifest_rel: "desktop/src-tauri/Cargo.toml",
        page_rel: Some("desktop"),
        // Named on the dependency and not on this package, which declares no
        // feature of its own: `--features tauri/custom-protocol` is what the
        // shell's own builder passes, and copying it here keeps the release and
        // that builder producing the same binary.
        features: &["tauri/custom-protocol"],
        live_rel: "desktop/src-tauri/target/release/sailor-desktop",
        safe_rel: "bin/sailor-desktop",
        stamp_rel: "state/window-binary-commit",
        suite_memo_rel: "state/window-suite-tree",
        service: None,
    },
];

/// The manifest at the root of the sources: the workspace of the engine.
pub const ROOT_MANIFEST: &str = "Cargo.toml";

/// The manifests a release of this target runs the suite of, in order: the
/// root's, then the target's own when it declares one. Without the second a
/// window would go into service with its shell never judged.
///
/// **THE PAGE'S TESTS ARE NOT HERE, AND THAT IS NOT AN OMISSION.** They run
/// inside its build, so a page that does not pass produces no `dist`.
pub fn manifests_to_judge(target: &Target) -> Vec<&'static str> {
    let mut all = vec![ROOT_MANIFEST];
    if target.manifest_rel != ROOT_MANIFEST {
        all.push(target.manifest_rel);
    }
    all
}

/// The directories of the sources a target is made of, for git: `crates/`
/// always, plus the top of whatever else it declares. A window whose parts
/// were read as `crates/` only would be stamped with a commit that changed
/// nothing in it.
pub fn parts_of(target: &Target) -> Vec<&'static str> {
    let mut parts = vec!["crates"];
    // The root's manifest names no directory of its own, and `crates/` is
    // already here; a nested one names the tree it sits in.
    let manifest_dir = target
        .manifest_rel
        .contains('/')
        .then(|| top_of(target.manifest_rel));
    for part in [manifest_dir, target.page_rel.map(top_of)]
        .into_iter()
        .flatten()
    {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts
}

/// The first directory of a relative path.
fn top_of(path: &'static str) -> &'static str {
    path.split('/').next().unwrap_or(path)
}

/// What a release has to do about the page's modules before it can build it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modules {
    /// The tree holds exactly these ones: the clone borrows them instead of
    /// downloading a quarter of a gigabyte again.
    Borrow,
    /// Install them inside the clone, from the lock the clone carries.
    Install,
}

/// Whether the clone may borrow the tree's modules, from the two locks.
///
/// **THE LOCK IS THE WHOLE ANSWER.** Borrowed against a different one they
/// build a page nobody wrote — a window in service carrying versions its
/// commit never named — so byte equality is asked of the files themselves.
pub fn how_to_get_the_modules(cloned_lock: &str, tree_lock: &str, tree_has_them: bool) -> Modules {
    if tree_has_them && cloned_lock == tree_lock && !cloned_lock.is_empty() {
        Modules::Borrow
    } else {
        Modules::Install
    }
}

/// What a shell would run when somebody types the target's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnPath {
    /// What the search path finds is what was just put in service.
    Same,
    /// Another copy comes first, and it is a different file: whoever looks the
    /// name up, the window included, goes on running that one.
    Shadowed { first: String },
    /// Nothing on the search path answers to that name.
    Absent,
}

/// Which of the three it is, from what the search path answered.
///
/// **THE RELEASE CANNOT SEE THIS AND SAY NOTHING.** A release that ends green
/// while an older binary keeps answering to the name is the fault this crate
/// exists to prevent, through the one door it left open.
pub fn on_path(found: Option<&str>, safe: &str, same_bytes: bool) -> OnPath {
    match found {
        None => OnPath::Absent,
        Some(first) if first == safe || same_bytes => OnPath::Same,
        Some(first) => OnPath::Shadowed {
            first: first.to_owned(),
        },
    }
}

/// The code a release ends with, once it knows what the name finds.
///
/// **A SHADOWED RELEASE IS NOT A FINISHED RELEASE**: everything that looks the
/// name up goes on running the older copy, and ending zero there says «done»
/// about the one thing the release exists to guarantee.
pub fn ends_with(found: &OnPath) -> i32 {
    match found {
        OnPath::Shadowed { .. } => SHADOWED,
        _ => 0,
    }
}

/// What a release exits with when the name still finds another copy. Not `1`:
/// the build and the suite went, and nothing about them needs redoing.
pub const SHADOWED: i32 = 4;

/// The target with this name, if it exists.
pub fn target(name: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|t| t.name == name)
}

/// The accepted names, for whoever typed one that does not exist.
pub fn target_names() -> String {
    TARGETS
        .iter()
        .map(|t| t.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The first word of the first line that is not a comment.
///
/// `#` lines are for whoever had to **reconstruct** a value rather than record
/// it. A reconstructed fact may be used, but it must say that it is one.
pub fn read_stamp(contents: &str) -> Option<String> {
    contents
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_string)
}

/// A task the service is holding right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    pub task: String,
    pub pid: u32,
}

/// Whether it can be replaced now, and what was seen to say so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Readiness {
    /// Tasks held by a live process. While there is one, the restart waits.
    pub busy: Vec<Busy>,
    /// Receipts that do not say whose they are. They are reported but **do not
    /// block**: a stray file stopping every release forever is a worse fault
    /// than the one it would avoid.
    pub unknown: Vec<String>,
}

impl Readiness {
    pub fn is_ready(&self) -> bool {
        self.busy.is_empty()
    }
}

/// Reads the receipts in `in-corso/` and says whether the service is mid-task.
///
/// It asks whether the pid is alive rather than whether the file exists: a
/// receipt left by a process killed halfway stays until the next startup
/// recovers it, so presence alone would block releases exactly when one is most
/// needed. A recycled pid can lie, and that is accepted — a false "busy" costs
/// a postponed release, a false "free" costs a truncated task.
pub fn readiness(receipt_names: &[String], alive: &dyn Fn(u32) -> bool) -> Readiness {
    let mut out = Readiness::default();
    for name in receipt_names {
        // `.DS_Store` appears by itself in every directory the Finder looks at.
        // Reporting it as unidentified on every release teaches people to skip
        // that line, which is where a real receipt will show up one day.
        if name.starts_with('.') {
            continue;
        }
        let (task, pid) = split_receipt_name(name);
        match pid {
            Some(pid) if alive(pid) => out.busy.push(Busy { task, pid }),
            Some(_) => {} // orphaned: the service recovers it at next startup
            None => out.unknown.push(name.clone()),
        }
    }
    out
}

/// The files of the built page that the binary does not carry.
///
/// **A BUILD THAT SUCCEEDS IS NOT A BINARY THAT WORKS**: given the wrong
/// features the shell embeds nothing and compiles green, and what comes out
/// opens a window, carries its title, answers every call and shows a blank
/// page. Nothing in the exit code says so, so the bytes are read. Fault 77.
pub fn page_files_missing_from(binary: &[u8], asset_names: &[String]) -> Vec<String> {
    // Every name, not just one: a page missing a font was not embedded whole
    // either, and it costs one pass over a file already in hand.
    asset_names
        .iter()
        .filter(|name| !contains(binary, name.as_bytes()))
        .cloned()
        .collect()
}

/// Whether `needle` appears anywhere in `haystack`. The embedded names sit
/// among compressed blocks, so this is a byte search and not a line one.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of the fault: the page built, the binary carries none of it.
    #[test]
    fn a_binary_without_the_page_names_every_file_it_is_missing() {
        let names = vec!["assets/index-AAA.js".to_owned(), "assets/index-BBB.css".to_owned()];
        let missing = page_files_missing_from(b"a shell with no page inside", &names);
        assert_eq!(missing, names, "neither file is in there, so both are named");
    }

    #[test]
    fn a_binary_carrying_the_page_is_missing_nothing() {
        let names = vec!["assets/index-AAA.js".to_owned(), "assets/index-BBB.css".to_owned()];
        let binary = b"\x00\x01assets/index-AAA.js\xff\xfeassets/index-BBB.css\x00";
        assert!(page_files_missing_from(binary, &names).is_empty());
    }

    /// Half a page is the harder case: the build went, one file did not make it,
    /// and the window would open showing text with no style on it.
    #[test]
    fn a_page_missing_one_file_names_that_one() {
        let names = vec!["assets/index-AAA.js".to_owned(), "assets/index-BBB.css".to_owned()];
        let binary = b"only assets/index-AAA.js is in here";
        assert_eq!(page_files_missing_from(binary, &names), vec!["assets/index-BBB.css"]);
    }

    /// A check with nothing to look for reads as agreement — fault 22. It is the
    /// caller's job to have found the files; this says what this function does
    /// when it has not, so the caller can be judged on it.
    #[test]
    fn nothing_to_look_for_finds_nothing_missing() {
        assert!(page_files_missing_from(b"anything", &[]).is_empty());
    }

    /// The window declares what takes it out of development mode, and the engine
    /// declares nothing: a page built and then not embedded is fault 77.
    #[test]
    fn a_target_with_a_page_declares_what_embeds_it() {
        for target in TARGETS {
            if target.page_rel.is_some() {
                assert!(
                    target.features.contains(&"tauri/custom-protocol"),
                    "{}: it builds a page and does not ask for the feature that \
                     embeds it, so the binary would go looking for the \
                     development server instead",
                    target.name
                );
            } else {
                assert!(
                    target.features.is_empty(),
                    "{}: features on a target with no page need a reason here",
                    target.name
                );
            }
        }
    }

    #[test]
    fn stamp_skips_comments_and_blank_lines() {
        let contents = "# reconstructed by hand\n\n  abc123 (do not trust)\n";
        assert_eq!(read_stamp(contents).as_deref(), Some("abc123"));
    }

    #[test]
    fn a_stamp_of_only_comments_names_nothing() {
        assert_eq!(read_stamp("# nothing\n#\n\n"), None);
    }

    /// The case a release exists not to cause: truncating a task.
    #[test]
    fn a_receipt_of_a_live_process_holds_back_the_restart() {
        let names = vec!["triage-voci.task.4242".to_string()];
        let ready = readiness(&names, &|pid| pid == 4242);
        assert!(!ready.is_ready());
        assert_eq!(
            ready.busy,
            vec![Busy {
                task: "triage-voci.task".to_string(),
                pid: 4242
            }]
        );
    }

    /// The opposite case, and the one where a release is needed most: after a
    /// process died halfway the receipt stays, but belongs to nobody.
    #[test]
    fn an_orphaned_receipt_holds_back_nothing() {
        let names = vec!["triage-voci.task.4242".to_string()];
        let ready = readiness(&names, &|_| false);
        assert!(ready.is_ready());
        assert!(ready.unknown.is_empty());
    }

    #[test]
    fn a_receipt_without_a_pid_is_reported_but_does_not_block() {
        let names = vec!["arrivato-per-altra-via.txt".to_string()];
        let ready = readiness(&names, &|_| true);
        assert!(ready.is_ready());
        assert_eq!(
            ready.unknown,
            vec!["arrivato-per-altra-via.txt".to_string()]
        );
    }

    /// **THE ROOT'S SUITE DOES NOT ENTER A NESTED WORKSPACE.** A release that
    /// ran only the root's would put a shell into service nothing had judged.
    #[test]
    fn a_target_with_a_workspace_of_its_own_is_judged_by_two_suites() {
        let engine = target("sailor").expect("the table names it");
        let window = target("window").expect("the table names it");

        assert_eq!(manifests_to_judge(engine), vec![ROOT_MANIFEST]);
        assert_eq!(
            manifests_to_judge(window),
            vec![ROOT_MANIFEST, "desktop/src-tauri/Cargo.toml"],
        );
    }

    /// A page is built before the shell that embeds it, so the target says
    /// where it is. Saying the engine had one would send the release looking
    /// for a `package.json` at the root of the sources.
    #[test]
    fn only_the_target_that_carries_a_page_declares_one() {
        assert_eq!(target("sailor").expect("named").page_rel, None);
        assert_eq!(target("window").expect("named").page_rel, Some("desktop"));
    }

    /// **THE MODULES ARE BORROWED ONLY AGAINST THE SAME LOCK**, or the window
    /// goes into service carrying versions its own commit never named.
    #[test]
    fn the_modules_are_borrowed_only_when_the_two_locks_are_the_same_bytes() {
        let lock = "{\"lockfileVersion\": 3}";
        assert_eq!(how_to_get_the_modules(lock, lock, true), Modules::Borrow);
        assert_eq!(
            how_to_get_the_modules(lock, "{\"lockfileVersion\": 2}", true),
            Modules::Install
        );
        // The tree may simply not have them — a fresh checkout, or a cleaned
        // one. Then there is nothing to borrow and the answer is not "yes".
        assert_eq!(how_to_get_the_modules(lock, lock, false), Modules::Install);
        // And two locks that are both unreadable are not "the same lock": an
        // empty string is what a missing file reads as, and borrowing on it
        // would be borrowing on no evidence at all.
        assert_eq!(how_to_get_the_modules("", "", true), Modules::Install);
    }

    /// What a release leaves out and what its stamp names are read over the
    /// parts the target is really made of, not over `crates/` alone.
    #[test]
    fn the_parts_of_a_target_are_the_directories_it_is_made_of() {
        assert_eq!(parts_of(target("sailor").expect("named")), vec!["crates"]);
        assert_eq!(
            parts_of(target("window").expect("named")),
            vec!["crates", "desktop"]
        );
    }

    /// The middle answer is the fault: green release, old binary answering.
    #[test]
    fn a_release_can_tell_whether_the_name_still_finds_an_older_copy() {
        let safe = "/home/bin/sailor";
        assert_eq!(on_path(Some(safe), safe, true), OnPath::Same);
        // A second copy elsewhere, byte for byte the same one: nothing to say.
        assert_eq!(on_path(Some("/altrove/bin/sailor"), safe, true), OnPath::Same);
        assert_eq!(
            on_path(Some("/altrove/bin/sailor"), safe, false),
            OnPath::Shadowed {
                first: "/altrove/bin/sailor".to_owned()
            }
        );
        assert_eq!(on_path(None, safe, false), OnPath::Absent);
    }

    /// The one outcome a release must not call finished.
    #[test]
    fn a_shadowed_release_does_not_end_at_zero() {
        assert_eq!(ends_with(&OnPath::Same), 0);
        assert_eq!(ends_with(&OnPath::Absent), 0);
        assert_eq!(
            ends_with(&OnPath::Shadowed {
                first: "/altrove/bin/sailor".to_owned()
            }),
            SHADOWED
        );
        assert_ne!(SHADOWED, 0);
    }

    #[test]
    fn targets_are_found_by_name() {
        assert_eq!(target("sailor").map(|t| t.bin), Some("sailor"));
        assert!(target("nessuno").is_none());
        // While these answered, the error listing available targets promised a
        // release that could not happen: both binaries are gone.
        assert!(target("notte").is_none());
        assert!(target("hooks").is_none());
    }

    /// Whoever puts the others into service must be able to do it for itself.
    #[test]
    fn the_releaser_can_release_itself() {
        let itself = target("sailor").expect("sailor must be a target");
        assert_eq!(itself.safe_rel, "bin/sailor");
        assert!(
            itself.service.is_none(),
            "sailor is not a resident service: declaring it one would call launchctl on a label that does not exist"
        );
    }

    /// Only a resident service declares a restart: saying it of one that is not
    /// would call `launchctl` on a label that does not exist. No target is
    /// resident today, and this demands it rather than assuming it — the day one
    /// comes back, whoever adds it has to name a real launchd label.
    #[test]
    fn only_a_resident_service_declares_a_restart() {
        for candidate in TARGETS {
            assert!(
                candidate.service.is_none(),
                "{}: declares a service to restart; check that the launchd label really exists",
                candidate.name
            );
        }
    }

    /// The invariant the whole defence rests on: the second copy lives where
    /// `cargo` does not write. Point it inside `target/` and the two paths
    /// become one, silently.
    ///
    /// It asks where it lives, not where it does not: "does not contain
    /// `target/`" is satisfied by the empty string, so the test would stay green
    /// with a `safe_rel` naming nowhere. A prohibition lets through everything
    /// it did not think of; an obligation does not.
    #[test]
    fn the_safe_copy_lives_under_bin() {
        for candidate in TARGETS {
            assert!(
                candidate.safe_rel.starts_with("bin/") && candidate.safe_rel.len() > 4,
                "{}: the safe copy is '{}', which is not under bin/ — under target/ cargo overwrites it",
                candidate.name,
                candidate.safe_rel
            );
        }
    }

    /// The three kinds together, which is what an `in-corso/` directory really
    /// looks like after a few days. The three separate tests above would pass
    /// even if the code handled the first receipt and ignored the rest.
    #[test]
    fn the_three_kinds_of_receipt_are_told_apart_together() {
        let names = vec![
            "vecchia.task.111".to_string(),
            ".DS_Store".to_string(),
            "viva.task.222".to_string(),
            "arrivato-per-altra-via".to_string(),
        ];
        let ready = readiness(&names, &|pid| pid == 222);
        assert!(!ready.is_ready());
        assert_eq!(ready.busy.len(), 1);
        assert_eq!(ready.busy[0].task, "viva.task");
        assert_eq!(ready.unknown, vec!["arrivato-per-altra-via".to_string()]);
    }

    /// A hand-reconstructed stamp carries more than one comment, and not always
    /// at the top: the first *useful* line must win anyway.
    #[test]
    fn the_stamp_reads_the_first_useful_line_whatever_surrounds_it() {
        let contents =
            "\n   \n# why it was reconstructed\n\t abc123 \n# and a note after it\ndef456\n";
        assert_eq!(read_stamp(contents).as_deref(), Some("abc123"));
    }
}

/// A receipt name split from its process-number suffix. The format belongs to
/// whoever writes the receipts, which is this crate.
pub fn split_receipt_name(name: &str) -> (String, Option<u32>) {
    match name.rsplit_once('.') {
        Some((base, suffix))
            if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) =>
        {
            (base.to_string(), suffix.parse().ok())
        }
        _ => (name.to_string(), None),
    }
}

/// Whether a process with that number still exists.
///
/// **An error that is not "no such process" counts as alive** — typically the
/// pid exists but is not ours. A false "dead" steps over somebody else's lock;
/// a false "alive" costs one more wait.
pub fn process_exists(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    const ESRCH: i32 = 3;
    let ret = unsafe { kill(pid as i32, 0) };
    if ret == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(ESRCH)
}
