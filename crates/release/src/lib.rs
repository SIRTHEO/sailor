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
        live_rel: "target/release/sailor",
        safe_rel: "bin/sailor",
        stamp_rel: "state/sailor-binary-commit",
        service: None,
    },
];

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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(ready.unknown, vec!["arrivato-per-altra-via.txt".to_string()]);
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
        let contents = "\n   \n# why it was reconstructed\n\t abc123 \n# and a note after it\ndef456\n";
        assert_eq!(read_stamp(contents).as_deref(), Some("abc123"));
    }
}

/// A receipt name split from its process-number suffix. The format belongs to
/// whoever writes the receipts, which is this crate.
pub fn split_receipt_name(name: &str) -> (String, Option<u32>) {
    match name.rsplit_once('.') {
        Some((base, suffix)) if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) => {
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
