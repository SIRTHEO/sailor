//! The commit this binary is built from, so `sailor version` can name the
//! build in service. Sources the repository does not track say they do not
//! know, even when they sit inside some other repository.

use std::path::Path;
use std::process::Command;

fn asked(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|said| !said.is_empty())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let tracked = asked(&["ls-files", "build.rs"]).is_some();
    let commit = tracked
        .then(|| asked(&["rev-parse", "--verify", "HEAD^{commit}"]))
        .flatten();
    println!(
        "cargo:rustc-env=SAILOR_BUILD_COMMIT={}",
        commit.as_deref().unwrap_or("")
    );
    let mut watched = vec![
        "HEAD".to_owned(),
        "logs/HEAD".to_owned(),
        "packed-refs".to_owned(),
    ];
    watched.extend(asked(&["symbolic-ref", "-q", "HEAD"]));
    // HEAD moves on a checkout, the branch it names on a commit even where no
    // reflog is kept. A watched file that is missing would make every build
    // dirty, so only those present count.
    for path in watched {
        let Some(file) = asked(&["rev-parse", "--path-format=absolute", "--git-path", &path])
        else {
            continue;
        };
        if Path::new(&file).exists() {
            println!("cargo:rerun-if-changed={file}");
        }
    }
}
