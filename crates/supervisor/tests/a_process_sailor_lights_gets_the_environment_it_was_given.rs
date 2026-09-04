//! **Fault 44: whoever lights a process must be able to say which home it runs
//! under.** The child inherited the environment of whoever opened the terminal,
//! whole, and there was no road to give it another.
//!
//! **A real process is lit and the variable the child received is read**, never
//! the rule handing it over: that test stays green with the rule unplugged.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use supervisor::child::{Process, Spec};
use supervisor::Running;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-supervisor-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("the directory is made");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Lights `/bin/sh`, has it write what it holds in `A_DECLARED_HOME` into a file, and
/// waits for the file. **The child is the judge**: what it received, it says.
fn what_the_child_received(dir: &TestDirectory, environment: Vec<(String, String)>) -> String {
    let written = dir.0.join("received");
    let mut process = Process::start(
        Spec {
            process_id: "a-test".to_owned(),
            command: "/bin/sh".to_owned(),
            args: vec![
                "-c".to_owned(),
                format!("printf '%s' \"${{A_DECLARED_HOME:-niente}}\" > {}", written.display()),
            ],
            working_directory: dir.0.clone(),
            port: None,
            purpose: "a test".to_owned(),
            started_by: "a test".to_owned(),
            environment,
        },
        None,
    )
    .expect("the process lights");

    // The file is waited for and not a fixed time: a `sleep` long enough here
    // is a judge that turns red on somebody else's machine.
    for _ in 0..200 {
        if let Ok(found) = std::fs::read_to_string(&written) {
            let _ = process.stop();
            return found;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let _ = process.stop();
    panic!("the child wrote nothing in five seconds");
}

/// **THE DECLARED ENVIRONMENT REACHES THE CHILD**, and the child says so.
#[test]
fn a_child_receives_the_environment_its_spec_declares() {
    let dir = TestDirectory::new("environment");

    let received = what_the_child_received(
        &dir,
        vec![("A_DECLARED_HOME".to_owned(), "/a/declared/home".to_owned())],
    );

    assert_eq!(received, "/a/declared/home");
}

/// **AND ONE THAT DECLARES NOTHING KEEPS WHAT THE PARENT HAD**, which is what a
/// development server really wants. Without this arm the cure could empty
/// everybody's environment and the test above would stay green.
#[test]
fn a_child_with_nothing_declared_keeps_what_the_parent_had() {
    let dir = TestDirectory::new("inherited-environment");
    std::env::set_var("A_DECLARED_HOME", "/the/home/of/whoever/lights/it");

    let received = what_the_child_received(&dir, Vec::new());
    std::env::remove_var("A_DECLARED_HOME");

    assert_eq!(received, "/the/home/of/whoever/lights/it");
}
