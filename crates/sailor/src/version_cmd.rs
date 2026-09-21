//! `sailor version`: the build that is running, to compare it against a
//! release's stamp (`sailor release <target> --dry-run` reads the same stamp)
//! without having to open a debugger.

/// The shape of `sailor version`. See `flow_cmd::USAGE`.
pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor version",
    says_key: "",
}];

pub fn run(_args: &[String]) -> i32 {
    println!(
        "{}",
        said(env!("CARGO_PKG_VERSION"), env!("SAILOR_BUILD_COMMIT"))
    );
    0
}

fn said(version: &str, commit: &str) -> String {
    if commit.is_empty() {
        return catalogue::say("cli.version.commit_unknown", &[("version", version)]);
    }
    format!("sailor {version} ({commit})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_exits_clean() {
        assert_eq!(run(&[]), 0);
    }

    #[test]
    fn the_version_names_the_commit_it_was_built_from() {
        assert_eq!(said("0.1.0", "760deb69"), "sailor 0.1.0 (760deb69)");
        let unknown = said("0.1.0", "");
        assert!(unknown.starts_with("sailor 0.1.0 ("), "{unknown}");
        assert_ne!(unknown, "sailor 0.1.0 ()");
    }

    fn git(args: &[&str]) -> Option<String> {
        let out = std::process::Command::new("git")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .filter(|said| !said.is_empty())
    }

    /// Sources a repository tracks name one of its commits; sources nothing
    /// tracks, such as an archive, say they do not know. The head itself may
    /// have moved since the build, so it is not what the stamp is held to.
    #[test]
    fn a_build_from_a_tracked_tree_names_one_of_its_commits() {
        let stamped = env!("SAILOR_BUILD_COMMIT");
        if git(&["ls-files", "build.rs"]).is_none() {
            assert_eq!(stamped, "");
            return;
        }
        let known = git(&["rev-parse", "--verify", &format!("{stamped}^{{commit}}")]);
        assert_eq!(known.as_deref(), Some(stamped));
    }
}
