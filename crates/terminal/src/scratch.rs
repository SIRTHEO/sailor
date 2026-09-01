//! A short scratch directory for tests, and a refusal that says who refused.
//!
//! Lives in the library rather than in the tests for the reason [`crate::Buffer`]
//! does: five test files and two modules need it, and copies drift on the one
//! detail that matters — the sentence printed when it cannot be had.

use std::path::PathBuf;

/// The variable that names a short writable root, for whoever runs inside a
/// perimeter that does not grant the usual one.
pub const ROOT_VARIABLE: &str = "SAILOR_TEST_TMP";

/// Where scratch directories go when nobody says otherwise.
///
/// Not the system temporary directory, and not by preference: a letterbox
/// address may not reach [`crate::inbox::LONGEST_ADDRESS`] bytes, and the
/// per-session scratch paths a sandbox usually grants spend most of that budget
/// before the socket is named.
const FALLBACK_ROOT: &str = "/tmp";

/// The root scratch directories are made in.
pub fn root() -> PathBuf {
    chosen_root(std::env::var_os(ROOT_VARIABLE))
}

/// The choice itself, apart from where the answer comes from: a test that had
/// to set the variable would race every other test making a directory.
fn chosen_root(declared: Option<std::ffi::OsString>) -> PathBuf {
    declared
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(FALLBACK_ROOT))
}

/// A scratch directory short enough to hold a letterbox, made fresh.
///
/// **BEING DENIED AND BEING WRONG LOOK ALIKE.** A perimeter that refuses the
/// root turns every test here red under names like `relay` and `terminal`, so
/// the honest reading becomes "the new branch is broken". The refusal names the
/// perimeter, the cap that forces a short path, and the way out.
pub fn directory(name: &str) -> PathBuf {
    let root = root();
    let directory = root.join(format!("sr-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    if let Err(error) = std::fs::create_dir_all(&directory) {
        panic!("{}", refusal(&directory, &error));
    }
    directory
}

/// The sentence a failed scratch directory prints, kept apart so a test can
/// read it without needing a perimeter to deny anything.
pub fn refusal(directory: &std::path::Path, error: &std::io::Error) -> String {
    let mut said = format!(
        "could not make the scratch directory {}",
        directory.display()
    );
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        said.push_str(&format!(
            ": the perimeter refused it, which is not a failure of what is being \
             tested. These tests open unix sockets, an address may not reach {} \
             bytes, and the usual scratch path spends that budget before the \
             socket is named — so they need a SHORT writable root. Set {} to one.",
            crate::inbox::LONGEST_ADDRESS,
            ROOT_VARIABLE
        ));
    } else {
        said.push_str(&format!(": {error}"));
    }
    said
}

/// The same refusal for a call a perimeter denied rather than a directory.
///
/// Opening a pseudo-terminal or binding a socket is refused as a *call*, which
/// no short path cures — so the sentence must say so, or whoever declared
/// [`ROOT_VARIABLE`] counts the reds still there and concludes it does nothing.
pub fn blamed(error: std::io::Error) -> std::io::Error {
    if error.kind() != std::io::ErrorKind::PermissionDenied {
        return error;
    }
    std::io::Error::new(
        error.kind(),
        format!(
            "the perimeter refused the call itself, which is not a failure of \
             what is being tested, and which no short path cures — {ROOT_VARIABLE} \
             will not help here: {error}"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The variable is what makes this work anywhere, so it has to be read —
    /// and an empty one is nobody declaring anything, not a root at the top of
    /// the disk.
    #[test]
    fn a_declared_root_is_used_and_an_empty_one_is_not() {
        let declared = chosen_root(Some(std::ffi::OsString::from("/short/here")));
        assert_eq!(declared, PathBuf::from("/short/here"));

        assert_eq!(chosen_root(None), PathBuf::from(FALLBACK_ROOT));
        assert_eq!(
            chosen_root(Some(std::ffi::OsString::new())),
            PathBuf::from(FALLBACK_ROOT),
            "an empty variable must not turn every scratch path into a bare name"
        );
    }

    /// **THE POINT OF THE WHOLE MODULE.** A denial must accuse the perimeter and
    /// name the way out; anything else must not claim a perimeter that is fine.
    #[test]
    fn a_denial_names_the_perimeter_and_an_ordinary_error_does_not() {
        let path = std::path::Path::new("/somewhere/short");

        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let said = refusal(path, &denied);
        assert!(said.contains("perimeter refused"), "{said}");
        assert!(
            said.contains(ROOT_VARIABLE),
            "it must name the way out: {said}"
        );
        assert!(
            said.contains("not a failure of what is being tested"),
            "it must say the tested thing is not the accused: {said}"
        );

        let denied_call = blamed(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let said = denied_call.to_string();
        assert!(said.contains("refused the call itself"), "{said}");
        assert!(
            said.contains("will not help here"),
            "declaring the variable does not cure a denied call, and the \
             sentence has to say so or it invites the wrong conclusion: {said}"
        );
        let ordinary = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(
            !blamed(ordinary).to_string().contains("perimeter"),
            "only a denial names one"
        );

        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        let said = refusal(path, &missing);
        assert!(
            !said.contains("perimeter"),
            "a missing parent is not a sandbox, and saying so would send the \
             reader to the wrong place: {said}"
        );
    }
}
