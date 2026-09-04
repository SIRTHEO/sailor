//! Downloads the catalogue by running `curl`, with no authentication and so no
//! key in here. **The address is data, not Rust**: another catalogue is a file.
//!
//! No test reaches the real network: one is red when the line drops, not when
//! the code is wrong. What is tested is what happens **around** it, through
//! the override — a command that answers nothing, and one that answers.

use std::process::Command;

/// The source shipped with the product, embedded like the price list.
pub const BUILTIN_SOURCE: &str = include_str!("../catalogue-source.default.json");

/// The variable naming a file read instead of the shipped one.
pub const CATALOGUE_SOURCE_PATH_VAR: &str = "MODELS_CATALOGUE_SOURCE";

/// Where the catalogue is asked, and which reader reads the answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct CatalogueSource {
    pub url: String,
    pub shape: String,
}

#[derive(serde::Deserialize)]
struct SourceFile {
    catalogue: CatalogueSource,
}

/// What a file declares.
pub fn parse_source(text: &str) -> Result<CatalogueSource, String> {
    serde_json::from_str::<SourceFile>(text)
        .map(|read| read.catalogue)
        .map_err(|error| format!("the catalogue source does not parse: {error}"))
}

/// The source in force. **A broken file falls back to the shipped one**:
/// whoever mistyped a comma wants their catalogue back, not a dead product.
pub fn catalogue_source() -> CatalogueSource {
    let declared = std::env::var_os(CATALOGUE_SOURCE_PATH_VAR)
        .and_then(|path| std::fs::read_to_string(path).ok());
    let text = declared.as_deref().unwrap_or(BUILTIN_SOURCE);
    parse_source(text)
        .or_else(|_| parse_source(BUILTIN_SOURCE))
        .expect("the catalogue source shipped with the product parses")
}

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
            curl.args(["-sS", "-m", "30", &catalogue_source().url]);
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

    /// **THE SHIPPED SOURCE IS READ, NOT ASSUMED**: one that stops parsing
    /// leaves the product falling back to itself, which reads like a default.
    #[test]
    fn the_shipped_source_names_where_the_catalogue_is_asked_for() {
        let source = catalogue_source();
        assert!(source.url.starts_with("https://"), "{source:?}");
        assert!(!source.shape.is_empty(), "{source:?}");
    }

    /// **AND A FILE REPLACES IT WHOLE**: that is the point of the move, and it
    /// is asked of `catalogue_source` and not of the parser — the precedence
    /// between the two is the part a reversed `or_else` would silently swap.
    #[test]
    fn a_declared_file_replaces_the_shipped_source() {
        let shipped = catalogue_source().url;
        let path = std::env::temp_dir().join(format!("models-source-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"catalogue": {"url": "https://un-altro/models", "shape": "una-forma"}}"#,
        )
        .expect("the declared file is written");

        std::env::set_var(CATALOGUE_SOURCE_PATH_VAR, &path);
        let declared = catalogue_source();
        // A FILE THAT DOES NOT PARSE FALLS BACK, and the same variable says it.
        std::fs::write(&path, "{ non è JSON").expect("the broken file is written");
        let broken = catalogue_source();
        std::env::remove_var(CATALOGUE_SOURCE_PATH_VAR);
        let _ = std::fs::remove_file(&path);

        assert_eq!(declared.url, "https://un-altro/models", "the file was not read");
        assert_eq!(declared.shape, "una-forma");
        assert_eq!(broken.url, shipped, "a broken file left no catalogue at all");
    }

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
