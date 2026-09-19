//! The work an account really did, gathered from the home it was done in.
//!
//! **THE RECORDS ARE READ WHERE THE ENGINE WRITES THEM, AND NOWHERE ELSE.**
//! One home holds one account's transcripts; the same engine under another
//! home is another account, and the two are never added together.

use crate::descriptor::{Descriptor, Work};
use models::work::{Kept, Tallying, WorkWords, Worked};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// What one home did since `since_unix`; `None` where the descriptor declares
/// no record, which is **unknown and not nothing**.
pub fn read_in_home(descriptor: &Descriptor, home: &Path, since_unix: i64) -> Option<Worked> {
    let declared = descriptor.work.as_ref()?;
    let since = models::fuel::rfc3339_of_unix_secs(since_unix);
    let words = words_of(declared);
    let mut tally = Tallying::default();
    for file in records_under(&home.join(&declared.under), &declared.suffix, since_unix) {
        let Ok(opened) = std::fs::File::open(&file) else {
            continue;
        };
        for line in std::io::BufReader::new(opened).lines().map_while(Result::ok) {
            tally.read_line(&line, &words, &since);
        }
    }
    Some(tally.finish())
}

fn words_of(declared: &Work) -> WorkWords {
    WorkWords {
        when: declared.when.clone(),
        only_when: declared
            .only_when
            .iter()
            .map(|kept| Kept {
                at: kept.at.clone(),
                is: kept.is.clone(),
            })
            .collect(),
        model: declared.model.clone(),
        session: declared.session.clone(),
        input: declared.tokens.input.clone(),
        output: declared.tokens.output.clone(),
        cache_read: declared.tokens.cache_read.clone(),
        cache_write: declared.tokens.cache_write.clone(),
        cache_write_long: declared.tokens.cache_write_long.clone(),
    }
}

/// **A FILE UNTOUCHED SINCE THE WINDOW OPENED HOLDS NOTHING INSIDE IT.** These
/// directories grow to gigabytes; reading them all to find the seven files of
/// an afternoon is a panel nobody would wait for.
fn records_under(root: &Path, suffix: &str, since_unix: i64) -> Vec<PathBuf> {
    let opened_at = UNIX_EPOCH + Duration::from_secs(since_unix.max(0).unsigned_abs());
    let mut found = Vec::new();
    let mut left = vec![root.to_path_buf()];
    while let Some(here) = left.pop() {
        let Ok(entries) = std::fs::read_dir(&here) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(about) = entry.metadata() else {
                continue;
            };
            if about.is_dir() {
                left.push(entry.path());
            } else if entry.file_name().to_string_lossy().ends_with(suffix)
                && about.modified().is_ok_and(|touched| touched >= opened_at)
            {
                found.push(entry.path());
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_descriptor(work: Option<Work>) -> Descriptor {
        Descriptor {
            work,
            ..serde_json::from_str::<Descriptor>(r#"{"id":"an-engine","family":"engine"}"#).expect("a bare descriptor")
        }
    }

    fn declared() -> Work {
        serde_json::from_str(
            r#"{"under":"projects","suffix":".jsonl","when":["timestamp"],
                "only_when":[{"at":["type"],"is":"assistant"}],
                "model":["message","model"],"session":["sessionId"],
                "tokens":{"input":["message","usage","input_tokens"],
                          "output":["message","usage","output_tokens"]}}"#,
        )
        .expect("the declaration parses")
    }

    fn a_home_with_a_record(name: &str, line: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("sailor-work-{name}-{}", std::process::id()));
        let under = home.join("projects").join("a-tree");
        std::fs::remove_dir_all(&home).ok();
        std::fs::create_dir_all(&under).expect("the record directory");
        std::fs::write(under.join("a-session.jsonl"), line).expect("the record");
        home
    }

    #[test]
    fn the_calls_of_a_home_are_read_from_its_records() {
        let home = a_home_with_a_record(
            "read",
            r#"{"type":"assistant","timestamp":"2099-01-01T00:00:00Z","sessionId":"one","message":{"model":"big","usage":{"input_tokens":10,"output_tokens":2}}}"#,
        );
        let worked = read_in_home(&a_descriptor(Some(declared())), &home, 0)
            .expect("the descriptor declares a record");
        assert_eq!(worked.calls, 1);
        assert_eq!(worked.sessions, 1);
        assert_eq!(worked.by_model["big"].input, 10);
    }

    /// **UNKNOWN IS NOT ZERO.** An engine nobody measured must not show a row
    /// of noughts beside one that really did nothing.
    #[test]
    fn an_engine_that_declares_no_record_answers_nothing_at_all() {
        let home = a_home_with_a_record("undeclared", "{}");
        assert_eq!(read_in_home(&a_descriptor(None), &home, 0), None);
    }
}
