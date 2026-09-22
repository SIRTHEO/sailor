//! The last gate a candidate passes before integration or a release lets it
//! leave. Every case is decided from readings handed in, so a refusal is proved
//! without a forge, a remote that moves on cue, or a binary in service.

use actions::candidate_gate::{
    what_the_gate_says, Heads, PolicyWords, Readings, Service, TheScan, Verdict,
};

const TRUNK: &str = "7a7a7a";
const REVIEWED: &str = "c0ffee";
const CANDIDATE: &str = "cafe01";

fn policy(merge: &str) -> PolicyWords {
    PolicyWords {
        merge: merge.to_owned(),
        push: "auto".to_owned(),
        release: "ask".to_owned(),
        read_from: "d1d1d1".to_owned(),
    }
}

fn at(commit: &str) -> Option<String> {
    Some(commit.to_owned())
}

/// What `integrate-on-the-trunk` reads when nothing moved.
fn integrating() -> Readings {
    Readings {
        candidate: CANDIDATE.to_owned(),
        scan: TheScan::TheTrunks,
        policy_read: policy("auto"),
        policy_now: Ok(policy("auto")),
        trunk_was: TRUNK.to_owned(),
        trunk_now: at(TRUNK),
        already: false,
        heads: Some(Heads {
            reviewed: REVIEWED.to_owned(),
            local: at(REVIEWED),
            remote: at(REVIEWED),
        }),
        service: None,
    }
}

/// What `cut-a-release` reads when nothing moved.
fn releasing() -> Readings {
    Readings {
        heads: None,
        service: Some(Service {
            commit: CANDIDATE.to_owned(),
            sha256: "5a5a".to_owned(),
            walked_on: "5a5a".to_owned(),
        }),
        ..integrating()
    }
}

#[test]
fn an_integration_nothing_moved_under_goes_to_the_scan() {
    assert_eq!(what_the_gate_says(&integrating()), Verdict::LetTheScanJudge);
}

#[test]
fn a_release_nothing_moved_under_goes_to_the_scan() {
    assert_eq!(what_the_gate_says(&releasing()), Verdict::LetTheScanJudge);
}

/// A branch may not bring the judge that will judge it.
#[test]
fn a_scan_the_branch_changed_judges_nothing() {
    let readings = Readings {
        scan: TheScan::Differs,
        ..integrating()
    };
    assert_eq!(what_the_gate_says(&readings), Verdict::ScanDiffers);
}

#[test]
fn a_trunk_that_commits_no_scan_lets_nothing_through() {
    let readings = Readings {
        scan: TheScan::NotOnTheTrunk,
        ..releasing()
    };
    assert_eq!(what_the_gate_says(&readings), Verdict::ScanNotOnTheTrunk);
}

#[test]
fn a_policy_changed_since_it_was_read_withdraws_the_authorization() {
    let readings = Readings {
        policy_now: Ok(policy("ask")),
        ..integrating()
    };
    assert_eq!(what_the_gate_says(&readings), Verdict::PolicyChanged);
}

/// The same words read from another commit are another reading: whoever
/// authorized, authorized what that commit said.
#[test]
fn the_same_policy_read_from_another_commit_is_a_change() {
    let readings = Readings {
        policy_now: Ok(PolicyWords {
            read_from: "e2e2e2".to_owned(),
            ..policy("auto")
        }),
        ..integrating()
    };
    assert_eq!(what_the_gate_says(&readings), Verdict::PolicyChanged);
}

#[test]
fn a_policy_that_cannot_be_read_now_is_no_policy() {
    let readings = Readings {
        policy_now: Err("the trunk carries no readable policy".to_owned()),
        ..integrating()
    };
    assert_eq!(
        what_the_gate_says(&readings),
        Verdict::NoTrustedPolicy("the trunk carries no readable policy".to_owned())
    );
}

#[test]
fn a_trunk_that_moved_sends_the_flow_back() {
    let readings = Readings {
        trunk_now: at("9b9b9b"),
        ..integrating()
    };
    assert_eq!(
        what_the_gate_says(&readings),
        Verdict::TrunkMoved(at("9b9b9b"))
    );
}

/// A remote trunk that cannot be read has not been shown to stand still.
#[test]
fn a_trunk_that_cannot_be_read_has_moved() {
    let readings = Readings {
        trunk_now: None,
        ..releasing()
    };
    assert_eq!(what_the_gate_says(&readings), Verdict::TrunkMoved(None));
}

#[test]
fn a_local_branch_past_the_reviewed_commit_is_not_what_was_reviewed() {
    let mut readings = integrating();
    readings.heads = Some(Heads {
        local: at("0d0d0d"),
        ..readings.heads.unwrap()
    });
    assert_eq!(
        what_the_gate_says(&readings),
        Verdict::BranchMoved(at("0d0d0d"))
    );
}

#[test]
fn a_remote_branch_past_the_reviewed_commit_no_longer_shows_the_review() {
    let mut readings = integrating();
    readings.heads = Some(Heads {
        remote: None,
        ..readings.heads.unwrap()
    });
    assert_eq!(
        what_the_gate_says(&readings),
        Verdict::RemoteBranchMoved(None)
    );
}

/// A pull request that already merged has moved the trunk and may have moved
/// its branch on the forge; the local branch is still what the review covered.
#[test]
fn one_that_merged_already_is_not_held_by_the_heads_that_moved_with_it() {
    let mut readings = Readings {
        already: true,
        trunk_now: at("9b9b9b"),
        ..integrating()
    };
    readings.heads = Some(Heads {
        remote: None,
        ..readings.heads.unwrap()
    });
    assert_eq!(what_the_gate_says(&readings), Verdict::LetTheScanJudge);
    readings.heads = Some(Heads {
        local: at("0d0d0d"),
        ..readings.heads.unwrap()
    });
    assert_eq!(
        what_the_gate_says(&readings),
        Verdict::BranchMoved(at("0d0d0d"))
    );
}

/// Nothing shows a request merged on a trunk nobody could read.
#[test]
fn one_that_merged_already_is_held_by_a_trunk_nobody_could_read() {
    let readings = Readings {
        already: true,
        trunk_now: None,
        ..integrating()
    };
    assert_eq!(what_the_gate_says(&readings), Verdict::TrunkMoved(None));
}

#[test]
fn a_binary_built_from_another_commit_is_not_the_candidate() {
    let mut readings = releasing();
    readings.service = Some(Service {
        commit: "0ld0ld".to_owned(),
        ..readings.service.unwrap()
    });
    assert_eq!(
        what_the_gate_says(&readings),
        Verdict::BuiltFromSomethingElse("0ld0ld".to_owned())
    );
}

#[test]
fn a_binary_changed_since_the_journey_was_walked_on_it_is_held() {
    let mut readings = releasing();
    readings.service = Some(Service {
        sha256: "6b6b".to_owned(),
        ..readings.service.unwrap()
    });
    assert_eq!(
        what_the_gate_says(&readings),
        Verdict::ServiceChanged("6b6b".to_owned())
    );
}

/// The half around the decision, run for real: a repository with a remote, a
/// committed scan, and a candidate the scan either lets through or does not.
mod on_a_real_repository {
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const SCAN: &str = "scripts/scan.sh";

    fn scratch(name: &str) -> PathBuf {
        let at = std::env::temp_dir().join(format!("gate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).expect("the scratch directory");
        at
    }

    fn git(repo: &Path, args: &[&str]) -> String {
        let ran = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            ran.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        String::from_utf8_lossy(&ran.stdout).trim().to_owned()
    }

    fn commit(repo: &Path, file: &str, text: &str) -> String {
        let path = repo.join(file);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
        std::fs::write(&path, text).expect("the file");
        git(repo, &["add", file]);
        git(repo, &["commit", "-q", "-m", file]);
        git(repo, &["rev-parse", "HEAD"])
    }

    /// A trunk named `line`, a policy and a scan on it, pushed to a bare remote,
    /// and a branch `work` one commit past it: what integration is handed.
    fn a_delivery(name: &str, work_file: &str) -> (PathBuf, String, String) {
        let at = scratch(name);
        let (remote, repo) = (at.join("remote.git"), at.join("repo"));
        std::fs::create_dir_all(&repo).expect("the repository");
        git(&at, &["init", "-q", "--bare", "remote.git"]);
        git(&repo, &["init", "-q", "-b", "line"]);
        for (key, value) in [
            ("sailor.trunk", "line"),
            ("user.email", "gate@example"),
            ("user.name", "gate"),
        ] {
            git(&repo, &["config", key, value]);
        }
        git(
            &repo,
            &["remote", "add", "origin", &remote.to_string_lossy()],
        );
        commit(
            &repo,
            ".sailor/delivery-policy.json",
            r#"{"merge": "auto", "push": "auto", "release": "ask"}"#,
        );
        commit(
            &repo,
            SCAN,
            "#!/bin/sh\nif git ls-tree -r --name-only \"$1\" | grep -q forbidden; then echo \"a forbidden file\"; exit 3; fi\n",
        );
        git(&repo, &["update-index", "--chmod=+x", SCAN]);
        std::fs::set_permissions(
            repo.join(SCAN),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .expect("the scan runs");
        let trunk = commit(&repo, "readme", "the trunk\n");
        git(&repo, &["push", "-q", "origin", "line"]);
        git(&repo, &["checkout", "-q", "-b", "work"]);
        let reviewed = commit(&repo, work_file, "the change\n");
        git(&repo, &["push", "-q", "origin", "work"]);
        git(&repo, &["checkout", "-q", "line"]);
        (repo, trunk, reviewed)
    }

    fn integrating(repo: &Path, trunk: &str, reviewed: &str) -> Value {
        let policy = workspace::delivery::policy_on_the_trunk(repo).expect("the policy reads");
        json!({
            "repo": repo,
            "remote": "origin",
            "base": "line",
            "candidate": reviewed,
            "scan": SCAN,
            "scan_base": trunk,
            "policy_read": serde_json::to_value(policy).expect("the policy serializes"),
            "trunk_was": trunk,
            "already": false,
            "branch": "work",
            "reviewed": reviewed,
        })
    }

    fn the_gate(input: Value) -> Result<Value, (String, String)> {
        let mut registry = flow::ActionRegistry::default();
        actions::candidate_gate::register_candidate_gate(&mut registry);
        let action = registry
            .get("candidate_gate")
            .expect("the action is registered");
        match action.execute(&input, &flow::SharedState::default()) {
            Ok(flow::ActionOutcome::Went(said)) => Ok(said),
            Ok(other) => Err(("not_went".to_owned(), format!("{other:?}"))),
            Err(refusal) => Err((refusal.class, refusal.said)),
        }
    }

    #[test]
    fn a_candidate_the_scan_lets_through_leaves() {
        let (repo, trunk, reviewed) = a_delivery("clean", "src/change");
        assert_eq!(
            the_gate(integrating(&repo, &trunk, &reviewed)),
            Ok(json!({ "ref": reviewed, "privacy_exit": 0 }))
        );
    }

    /// The gate reads the trunk and nothing else: a tag the remote carries is
    /// not written into the repository the delivery runs from.
    #[test]
    fn the_gate_brings_home_no_tag_of_the_remote() {
        let (repo, trunk, reviewed) = a_delivery("tag", "src/change");
        git(&repo, &["push", "-q", "origin", &format!("{reviewed}:refs/tags/archive")]);
        assert_eq!(
            the_gate(integrating(&repo, &trunk, &reviewed)),
            Ok(json!({ "ref": reviewed, "privacy_exit": 0 }))
        );
        assert_eq!(git(&repo, &["tag", "--list"]), "");
    }

    #[test]
    fn a_candidate_the_scan_refuses_stays_with_what_the_scan_said() {
        let (repo, trunk, reviewed) = a_delivery("refused", "forbidden");
        let (class, said) = the_gate(integrating(&repo, &trunk, &reviewed)).unwrap_err();
        assert_eq!(class, "publication_refused");
        assert!(
            said.contains("exit 3") && said.contains("a forbidden file"),
            "{said}"
        );
    }

    #[test]
    fn a_scan_edited_in_the_tree_judges_nothing() {
        let (repo, trunk, reviewed) = a_delivery("edited", "src/change");
        std::fs::write(repo.join(SCAN), "#!/bin/sh\nexit 0\n").expect("the edit");
        let (class, said) = the_gate(integrating(&repo, &trunk, &reviewed)).unwrap_err();
        assert_eq!(class, "the_candidate_is_held");
        assert!(
            said.contains("differs from the one the trusted trunk commits"),
            "{said}"
        );
    }

    /// A lost transport is not an empty ref: nothing is decided on it.
    #[test]
    fn a_remote_that_cannot_be_reached_holds_the_candidate() {
        let (repo, trunk, reviewed) = a_delivery("unreached", "src/change");
        let input = integrating(&repo, &trunk, &reviewed);
        std::fs::remove_dir_all(repo.parent().expect("the scratch").join("remote.git"))
            .expect("the remote goes");
        let (class, said) = the_gate(input).unwrap_err();
        assert_eq!(class, "the_candidate_is_held");
        assert!(said.contains("cannot read origin"), "{said}");
    }

    /// A refusal this machine can make alone is made before the remote is
    /// asked, so a lost transport cannot hide it.
    #[test]
    fn a_refusal_read_here_is_not_hidden_by_a_lost_remote() {
        let (repo, trunk, reviewed) = a_delivery("both", "src/change");
        let input = integrating(&repo, &trunk, &reviewed);
        std::fs::write(repo.join(SCAN), "#!/bin/sh\nexit 0\n").expect("the edit");
        std::fs::remove_dir_all(repo.parent().expect("the scratch").join("remote.git"))
            .expect("the remote goes");
        let (_, said) = the_gate(input).unwrap_err();
        assert!(
            said.contains("differs from the one the trusted trunk commits"),
            "{said}"
        );
    }

    #[test]
    fn a_trunk_that_moved_on_the_remote_is_read_after_a_fetch() {
        let (repo, trunk, reviewed) = a_delivery("moved", "src/change");
        let input = integrating(&repo, &trunk, &reviewed);
        git(&repo, &["checkout", "-q", "--detach"]);
        let moved = commit(&repo, "later", "somebody else landed first\n");
        git(&repo, &["push", "-q", "origin", "HEAD:line"]);
        git(&repo, &["checkout", "-q", "line"]);
        git(&repo, &["update-ref", "refs/remotes/origin/line", &trunk]);
        let (class, said) = the_gate(input).unwrap_err();
        assert_eq!(class, "the_candidate_is_held");
        assert!(said.contains(&moved), "{said}");
    }

    #[test]
    fn a_remote_branch_pushed_past_the_review_is_held() {
        let (repo, trunk, reviewed) = a_delivery("pushed", "src/change");
        let input = integrating(&repo, &trunk, &reviewed);
        git(&repo, &["checkout", "-q", "work"]);
        let later = commit(&repo, "src/more", "after the review\n");
        git(&repo, &["push", "-q", "origin", "work"]);
        git(&repo, &["reset", "-q", "--hard", &reviewed]);
        git(&repo, &["checkout", "-q", "line"]);
        let (class, said) = the_gate(input).unwrap_err();
        assert_eq!(class, "the_candidate_is_held");
        assert!(
            said.contains(&later) && said.contains("no longer shows"),
            "{said}"
        );
    }
}
