//! Downloads the real catalog by running `curl` — the road `notte` took to
//! OpenRouter. No authentication, so no key in here.
//!
//! No test reaches the real network: one is red when the line drops, not when
//! the code is wrong. What is tested is what happens **around** it, through
//! the override — a command that answers nothing, and one that answers.

use std::process::Command;

const CATALOG_URL: &str = "https://openrouter.ai/api/v1/models";

/// Downloads the catalog's JSON body. `MODELS_CATALOG_FETCH_OVERRIDE` replaces
/// `curl` with any command, to feed in a fixed catalog without a network.
///
/// **A LINE THAT IS DOWN AND A CATALOG THAT IS BROKEN ARE DIFFERENT FACTS.** It
/// used to answer an empty string for both, and the caller then said «invalid
/// JSON: EOF» — sending the reader to hunt a bug in a body never received.
pub fn catalog_body() -> Result<String, String> {
    let (what, mut command) = match std::env::var("MODELS_CATALOG_FETCH_OVERRIDE") {
        Ok(cmd) => (
            "MODELS_CATALOG_FETCH_OVERRIDE".to_owned(),
            Command::new(cmd),
        ),
        Err(_) => {
            let mut curl = Command::new("curl");
            curl.args(["-sS", "-m", "30", CATALOG_URL]);
            ("curl".to_owned(), curl)
        }
    };
    let output = command
        .output()
        .map_err(|error| format!("{what} did not start: {error}"))?;
    let body = String::from_utf8(output.stdout)
        .map_err(|_| format!("{what} answered something that is not text"))?;
    if body.trim().is_empty() {
        // curl's own words go to stderr, and they name the reason: no network,
        // a name that will not resolve, a timeout. Dropping them here would
        // leave the reader with «empty» and nothing to act on.
        let said = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if said.is_empty() {
            format!("{what} answered nothing at all")
        } else {
            format!("{what} answered nothing: {said}")
        });
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The override is the only seam the network leaves: through it, both
    /// outcomes are reachable without a line.
    fn with_override<T>(command: &str, work: impl FnOnce() -> T) -> T {
        // The variable is process-wide, so these two tests must not run at the
        // same time. One test, two arms.
        std::env::set_var("MODELS_CATALOG_FETCH_OVERRIDE", command);
        let out = work();
        std::env::remove_var("MODELS_CATALOG_FETCH_OVERRIDE");
        out
    }

    #[test]
    fn a_source_that_answers_nothing_is_not_a_broken_catalogue() {
        // THE ARM THAT MUST WORK, FIRST: with a real answer nothing is wrong,
        // and without this the check below would pass on a function that
        // always fails. `pwd` and not `echo` — the override runs with no
        // arguments, and a bare `echo` prints one empty line, which is exactly
        // the case being told apart here. It cost a red test to notice.
        let said = with_override("pwd", || catalog_body());
        assert!(
            said.is_ok(),
            "a source that answers was called broken: {said:?}"
        );

        let empty = with_override("true", || catalog_body());
        let why = empty.expect_err("a source that says nothing must not read as a catalogue");
        assert!(
            why.contains("nothing"),
            "the reason has to say the answer was empty, not describe JSON: {why}",
        );

        let missing = with_override("/nowhere/at/all/definitely-not-a-command", || {
            catalog_body()
        });
        let why = missing.expect_err("a command that will not start is not a catalogue either");
        assert!(
            why.contains("did not start"),
            "a command that never ran must not read as one that answered: {why}",
        );
    }
}
