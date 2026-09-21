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
        return format!("sailor {version} (built outside a repository: commit unknown)");
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
        assert!(said("0.1.0", "").contains("commit unknown"));
        assert!(!env!("SAILOR_BUILD_COMMIT").is_empty(), "a build inside this repository knows its commit");
    }
}
