//! What an account really did, read off the records its own home keeps.
//!
//! **THE SPEND OF A FLOW IS NOT THE WORK OF A PERSON.** Sailor bills what its
//! own steps called; a terminal opened by hand passes through none of that and
//! reads zero for a day that moved millions of tokens. The engine writes every
//! call down in the home it ran in, and that file is where the figure is.

use std::collections::{BTreeMap, BTreeSet};

/// Where a home writes the calls made in it, and what the fields are called.
/// **DECLARED, NEVER GUESSED**: a reader named after one engine is the road
/// model independence forbids.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkWords {
    /// The keys down to the instant of the call, written so that two of them
    /// sort as they happened.
    pub when: Vec<String>,
    /// Records to count; anything else on the line is passed over.
    pub only_when: Vec<Kept>,
    pub model: Vec<String>,
    pub session: Vec<String>,
    pub input: Vec<String>,
    pub output: Vec<String>,
    pub cache_read: Vec<String>,
    pub cache_write: Vec<String>,
    pub cache_write_long: Vec<String>,
}

/// One field a record must carry, with that value, to be counted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Kept {
    pub at: Vec<String>,
    pub is: String,
}

/// The tokens a call moved, each under its own name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cache_write_long: u64,
}

impl Tokens {
    pub fn all(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write + self.cache_write_long
    }
}

/// What one account did inside one window, per model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Worked {
    pub calls: u64,
    pub sessions: usize,
    pub by_model: BTreeMap<String, Tokens>,
    /// The instant of the last call counted; empty where there was none.
    pub latest: String,
}

impl Worked {
    pub fn tokens(&self) -> Tokens {
        self.by_model
            .values()
            .fold(Tokens::default(), |mut all, one| {
                all.input += one.input;
                all.output += one.output;
                all.cache_read += one.cache_read;
                all.cache_write += one.cache_write;
                all.cache_write_long += one.cache_write_long;
                all
            })
    }

    /// Adds another home's reading of the same account: a session lives in
    /// one home only, so the sessions add up like the calls do.
    pub fn add(&mut self, other: Worked) {
        self.calls += other.calls;
        self.sessions += other.sessions;
        for (model, tokens) in other.by_model {
            let held = self.by_model.entry(model).or_default();
            held.input += tokens.input;
            held.output += tokens.output;
            held.cache_read += tokens.cache_read;
            held.cache_write += tokens.cache_write;
            held.cache_write_long += tokens.cache_write_long;
        }
        if other.latest > self.latest {
            self.latest = other.latest;
        }
    }
}

/// The work of every account, from the readings of every home. **AN ACCOUNT
/// SIGNED IN IN TWO HOMES WORKED IN BOTH**: keeping one reading per name drops
/// the other home's work without a word.
pub fn per_account(readings: impl IntoIterator<Item = (String, Worked)>) -> BTreeMap<String, Worked> {
    let mut found: BTreeMap<String, Worked> = BTreeMap::new();
    for (account, worked) in readings {
        found.entry(account).or_default().add(worked);
    }
    found
}

/// What this work would cost at list price, in micro-units; `None` where a
/// model in it has no price, since a sum missing one model is an underestimate
/// wearing the face of a measure. **A SUBSCRIPTION PAYS NONE OF IT**: this is
/// the weight of the work, in the one unit that compares two accounts.
pub fn cost_micros(worked: &Worked, prices: &crate::pricing::PriceList) -> Option<i64> {
    let mut total: i64 = 0;
    for (model, tokens) in &worked.by_model {
        let counts = crate::pricing::TokenCounts {
            input: Some(tokens.input),
            output: Some(tokens.output),
            cached: Some(tokens.cache_read),
            cache_write: Some(tokens.cache_write),
            cache_write_long: Some(tokens.cache_write_long),
        };
        total += crate::pricing::cost_micros(counts, prices.find(model)?.micros())?;
    }
    Some(total)
}

/// A tally being made, so that many files add into one reading.
#[derive(Debug, Default)]
pub struct Tallying {
    calls: u64,
    by_model: BTreeMap<String, Tokens>,
    sessions: BTreeSet<String>,
    latest: String,
}

impl Tallying {
    /// Counts one record, if it is one and if it falls inside the window.
    /// **A LINE THAT DOES NOT PARSE IS PASSED OVER**: these files are written
    /// while they are read, and the last line is often half there.
    pub fn read_line(&mut self, line: &str, words: &WorkWords, since: &str) {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        if !words
            .only_when
            .iter()
            .all(|kept| text_at(&record, &kept.at) == Some(kept.is.as_str()))
        {
            return;
        }
        let Some(when) = text_at(&record, &words.when) else {
            return;
        };
        if when < since {
            return;
        }
        let counted = Tokens {
            input: number_at(&record, &words.input),
            output: number_at(&record, &words.output),
            cache_read: number_at(&record, &words.cache_read),
            cache_write: number_at(&record, &words.cache_write),
            cache_write_long: number_at(&record, &words.cache_write_long),
        };
        if counted.all() == 0 {
            return;
        }
        self.calls += 1;
        if when > self.latest.as_str() {
            when.clone_into(&mut self.latest);
        }
        if let Some(session) = text_at(&record, &words.session) {
            self.sessions.insert(session.to_owned());
        }
        let model = text_at(&record, &words.model).unwrap_or("").to_owned();
        let held = self.by_model.entry(model).or_default();
        held.input += counted.input;
        held.output += counted.output;
        held.cache_read += counted.cache_read;
        held.cache_write += counted.cache_write;
        held.cache_write_long += counted.cache_write_long;
    }

    pub fn finish(self) -> Worked {
        Worked {
            calls: self.calls,
            sessions: self.sessions.len(),
            by_model: self.by_model,
            latest: self.latest,
        }
    }
}

fn at<'a>(record: &'a serde_json::Value, path: &[String]) -> Option<&'a serde_json::Value> {
    if path.is_empty() {
        return None;
    }
    let mut here = record;
    for key in path {
        here = here.get(key)?;
    }
    Some(here)
}

fn text_at<'a>(record: &'a serde_json::Value, path: &[String]) -> Option<&'a str> {
    at(record, path).and_then(serde_json::Value::as_str)
}

/// **WHAT IS NOT THERE IS NOT COUNTED, AND ZERO IS WHAT THAT LOOKS LIKE.** A
/// sum has no room for «unknown», so the sum of nothing is nothing.
fn number_at(record: &serde_json::Value, path: &[String]) -> u64 {
    at(record, path).and_then(serde_json::Value::as_u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> WorkWords {
        WorkWords {
            when: vec!["timestamp".to_owned()],
            only_when: vec![Kept {
                at: vec!["type".to_owned()],
                is: "assistant".to_owned(),
            }],
            model: vec!["message".to_owned(), "model".to_owned()],
            session: vec!["sessionId".to_owned()],
            input: vec!["message".to_owned(), "usage".to_owned(), "input_tokens".to_owned()],
            output: vec!["message".to_owned(), "usage".to_owned(), "output_tokens".to_owned()],
            cache_read: vec![
                "message".to_owned(),
                "usage".to_owned(),
                "cache_read_input_tokens".to_owned(),
            ],
            cache_write: vec![
                "message".to_owned(),
                "usage".to_owned(),
                "cache_creation_input_tokens".to_owned(),
            ],
            cache_write_long: Vec::new(),
        }
    }

    fn a_call(when: &str, session: &str, model: &str, input: u64, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{when}","sessionId":"{session}",
               "message":{{"model":"{model}","usage":{{"input_tokens":{input},
               "output_tokens":{output},"cache_read_input_tokens":7,
               "cache_creation_input_tokens":3}}}}}}"#
        )
    }

    fn tallied(lines: &[String]) -> Worked {
        let mut tally = Tallying::default();
        for line in lines {
            tally.read_line(line, &words(), "2026-09-19T09:00:00Z");
        }
        tally.finish()
    }

    #[test]
    fn an_account_signed_in_in_two_homes_is_credited_with_both() {
        let dedicated = tallied(&[a_call("2026-09-19T10:00:00Z", "one", "big", 10, 1)]);
        let default = tallied(&[
            a_call("2026-09-19T11:00:00Z", "two", "big", 20, 2),
            a_call("2026-09-19T12:00:00Z", "three", "small", 30, 3),
        ]);
        let other = tallied(&[a_call("2026-09-19T13:00:00Z", "four", "big", 40, 4)]);
        let found = per_account([
            ("someone@example.test".to_owned(), dedicated),
            ("somebody-else@example.test".to_owned(), other),
            ("someone@example.test".to_owned(), default),
        ]);
        let someone = &found["someone@example.test"];
        assert_eq!((someone.calls, someone.sessions), (3, 3));
        assert_eq!(someone.by_model["big"].input, 30);
        assert_eq!(someone.by_model["small"].input, 30);
        assert_eq!(someone.latest, "2026-09-19T12:00:00Z");
        assert_eq!(found["somebody-else@example.test"].calls, 1);
    }

    #[test]
    fn the_calls_of_a_window_are_counted_per_model_and_per_session() {
        let mut tally = Tallying::default();
        for line in [
            a_call("2026-09-19T10:00:00Z", "one", "big", 10, 1),
            a_call("2026-09-19T11:00:00Z", "one", "big", 20, 2),
            a_call("2026-09-19T12:00:00Z", "two", "small", 30, 3),
        ] {
            tally.read_line(&line, &words(), "2026-09-19T09:00:00Z");
        }
        let worked = tally.finish();
        assert_eq!(worked.calls, 3);
        assert_eq!(worked.sessions, 2, "two sessions, not three calls");
        assert_eq!(worked.by_model["big"].input, 30);
        assert_eq!(worked.by_model["small"].output, 3);
        assert_eq!(worked.tokens().cache_read, 21);
        assert_eq!(worked.latest, "2026-09-19T12:00:00Z");
    }

    /// **THE WINDOW IS THE RECORD'S OWN INSTANT, NOT THE FILE'S.** A transcript
    /// written to all day carries the whole day; counting the file would hand
    /// five hours everything since the session opened.
    #[test]
    fn a_call_older_than_the_window_is_not_counted() {
        let mut tally = Tallying::default();
        tally.read_line(
            &a_call("2026-09-18T23:00:00Z", "one", "big", 10, 1),
            &words(),
            "2026-09-19T09:00:00Z",
        );
        assert_eq!(tally.finish(), Worked::default());
    }

    /// The person's own turns carry no usage, and the file is written while it
    /// is read, so its last line is often half there.
    #[test]
    fn what_is_not_a_call_is_passed_over_and_a_broken_line_never_panics() {
        let mut tally = Tallying::default();
        for line in [
            r#"{"type":"user","timestamp":"2026-09-19T10:00:00Z","message":{"content":"hi"}}"#,
            r#"{"type":"assistant","timestamp":"2026-09-19T10:0"#,
            "",
        ] {
            tally.read_line(line, &words(), "2026-09-19T09:00:00Z");
        }
        assert_eq!(tally.finish().calls, 0);
    }

    /// **A DECLARATION THAT NAMES NOTHING COUNTS NOTHING.** An engine nobody
    /// measured must read as unknown, and unknown is not a row of zeroes.
    #[test]
    fn words_that_name_no_field_count_nothing() {
        let mut tally = Tallying::default();
        tally.read_line(
            &a_call("2026-09-19T10:00:00Z", "one", "big", 10, 1),
            &WorkWords::default(),
            "2026-09-19T09:00:00Z",
        );
        assert_eq!(tally.finish().calls, 0);
    }
}
