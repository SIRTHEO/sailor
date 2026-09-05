//! Full-text ranking over texts handed in. SQLite's FTS5 is already compiled
//! into this crate, and an index rebuilt in memory from a few dozen documents
//! costs less than keeping one fresh on disk.

use crate::{Ledger, LedgerError};
use rusqlite::{params, Connection};

/// One document that matched: its id, its rank (lower is better, as `bm25`
/// gives it) and a snippet around the words that matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: String,
    pub rank: f64,
    pub excerpt: String,
}

/// The documents that mention every word of `query`, best first.
pub fn rank_texts(documents: &[(String, String)], query: &str) -> Result<Vec<Hit>, LedgerError> {
    let words = fts_query(query);
    if words.is_empty() {
        return Err(LedgerError::Refused("a search needs a word".to_owned()));
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch("CREATE VIRTUAL TABLE docs USING fts5(id UNINDEXED, body);")?;
    {
        let mut insert = connection.prepare("INSERT INTO docs(id, body) VALUES (?1, ?2)")?;
        for (id, body) in documents {
            insert.execute(params![id, body])?;
        }
    }
    let mut select = connection.prepare(
        "SELECT id, bm25(docs), snippet(docs, 1, '«', '»', '…', 12) \
         FROM docs WHERE docs MATCH ?1 ORDER BY bm25(docs)",
    )?;
    let hits = select
        .query_map(params![words], |row| {
            Ok(Hit {
                id: row.get(0)?,
                rank: row.get(1)?,
                excerpt: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(hits)
}

impl Ledger {
    /// What the ledger holds as text, one document per run, step, event and
    /// store entry — the most recent first, bounded, because a search is a
    /// question about what happened lately and not a scan of years.
    pub fn documents_to_search(
        &self,
        recent_runs: usize,
        recent_steps: usize,
        recent_events: usize,
    ) -> Result<Vec<(String, String)>, LedgerError> {
        let connection = self.lock()?;
        let mut documents = Vec::new();
        let mut runs = connection.prepare(
            "SELECT run_id, kind, entity, started_by, status, COALESCE(error, ''), COALESCE(worktree, '')              FROM runs ORDER BY started_at DESC LIMIT ?1",
        )?;
        for row in runs.query_map(params![recent_runs as i64], |row| {
            let id: String = row.get(0)?;
            let text = [1, 2, 3, 4, 5, 6]
                .iter()
                .map(|column| row.get::<_, String>(*column))
                .collect::<Result<Vec<_>, _>>()?
                .join(" ");
            Ok((format!("run:{id}"), text))
        })? {
            documents.push(row?);
        }
        let mut steps = connection.prepare(
            "SELECT run_id, step_id, attempt, COALESCE(failure_class, ''), COALESCE(said, ''),              SUBSTR(COALESCE(output, ''), 1, 2000)              FROM steps ORDER BY started_at DESC LIMIT ?1",
        )?;
        for row in steps.query_map(params![recent_steps as i64], |row| {
            let run: String = row.get(0)?;
            let step: String = row.get(1)?;
            let attempt: i64 = row.get(2)?;
            let text = [1, 3, 4, 5]
                .iter()
                .map(|column| row.get::<_, String>(*column))
                .collect::<Result<Vec<_>, _>>()?
                .join(" ");
            Ok((format!("step:{run}/{step}#{attempt}"), text))
        })? {
            documents.push(row?);
        }
        let mut events = connection.prepare(
            "SELECT seq, kind, COALESCE(run_id, ''), COALESCE(step_id, ''), SUBSTR(payload, 1, 2000)              FROM events.events ORDER BY seq DESC LIMIT ?1",
        )?;
        for row in events.query_map(params![recent_events as i64], |row| {
            let seq: i64 = row.get(0)?;
            let text = [1, 2, 3, 4]
                .iter()
                .map(|column| row.get::<_, String>(*column))
                .collect::<Result<Vec<_>, _>>()?
                .join(" ");
            Ok((format!("event:{seq}"), text))
        })? {
            documents.push(row?);
        }
        let mut store = connection.prepare("SELECT collection, key, value FROM store")?;
        for row in store.query_map([], |row| {
            let collection: String = row.get(0)?;
            let key: String = row.get(1)?;
            let value: String = row.get(2)?;
            Ok((format!("store:{collection}/{key}"), format!("{collection} {key} {value}")))
        })? {
            documents.push(row?);
        }
        Ok(documents)
    }

    /// The runs, steps, events and store entries that mention every word,
    /// best first.
    pub fn search(
        &self,
        query: &str,
        recent_runs: usize,
        recent_steps: usize,
        recent_events: usize,
    ) -> Result<Vec<Hit>, LedgerError> {
        rank_texts(&self.documents_to_search(recent_runs, recent_steps, recent_events)?, query)
    }
}

/// Each word quoted, so a colon or a dash in what a person typed is a
/// character and not FTS5 syntax; words side by side mean all of them.
fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|word| word.replace('"', ""))
        .filter(|word| word.chars().count() >= 2)
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn documents() -> Vec<(String, String)> {
        vec![
            ("sweep".to_owned(), "the tree is swept of what nobody claims".to_owned()),
            ("watch".to_owned(), "who works where, and a run waiting since yesterday".to_owned()),
            ("relay".to_owned(), "the mandate is typed into the terminal".to_owned()),
        ]
    }

    /// A word that appears in one document finds that one and no other.
    #[test]
    fn a_word_in_one_document_finds_that_one_and_no_other() {
        let hits = rank_texts(&documents(), "yesterday").expect("a ranking");
        let ids: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
        assert_eq!(ids, vec!["watch"]);
        assert!(hits[0].excerpt.contains("«yesterday»"), "{}", hits[0].excerpt);
    }

    /// Two words are both required, and a stem is not a match: the ranking
    /// answers for the words as typed.
    #[test]
    fn every_word_typed_has_to_be_there() {
        let both = rank_texts(&documents(), "tree claims").expect("a ranking");
        assert_eq!(both.len(), 1, "{both:?}");
        let neither = rank_texts(&documents(), "tree yesterday").expect("a ranking");
        assert!(neither.is_empty(), "{neither:?}");
    }

    /// Punctuation a person types is looked for, not parsed.
    #[test]
    fn a_colon_or_a_quote_is_a_character_and_not_syntax() {
        let hits = rank_texts(&documents(), "\"who: works\"").expect("a ranking");
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    /// A run, a step and a store entry are each one document, found by a word
    /// only they say; the id says which of the three it is. The store entry
    /// is written as an event, so the event that wrote it answers as well.
    #[test]
    fn what_the_ledger_holds_is_found_by_its_words() {
        let dir = std::env::temp_dir().join(format!("sailor-ledger-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("a ledger");
        {
            let connection = ledger.lock().expect("the connection");
            connection
                .execute(
                    "INSERT INTO runs (run_id, kind, entity, started_by, status, total_cost_micros, started_at)                      VALUES ('r1', 'flow', 'sweep-the-tree', 'window', 'completed', 0, 1)",
                    [],
                )
                .expect("a run");
            connection
                .execute(
                    "INSERT INTO steps (run_id, step_id, attempt, epoch, deps, input_digest, input, gates, started_at, said)                      VALUES ('r1', 'rewrite', 1, '0', '[]', 'd', '{}', '[]', 2, 'the comments were pruned to zwieback')",
                    [],
                )
                .expect("a step");
        }
        ledger
            .put_record(&crate::StoreRecord {
                collection: "notes".to_owned(),
                key: "one".to_owned(),
                value: serde_json::json!({ "text": "a marzipan reminder" }),
                written_by: "test".to_owned(),
                written_at: 3,
            })
            .expect("a record");

        let ids = |query: &str| -> Vec<String> {
            ledger
                .search(query, 100, 100, 100)
                .expect("a ranking")
                .into_iter()
                .map(|hit| hit.id)
                .collect()
        };
        let by_step = ids("zwieback");
        let by_store = ids("marzipan");
        let by_run = ids("sweep-the-tree");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(by_step, vec!["step:r1/rewrite#1"]);
        assert_eq!(by_store, vec!["store:notes/one", "event:1"]);
        assert_eq!(by_run, vec!["run:r1"]);
    }

    /// An event is one document too, found by a word only its payload says:
    /// the parent run's id is in the event's payload, and neither in the run's
    /// text nor in the event's own columns.
    #[test]
    fn an_event_is_found_by_a_word_only_its_payload_says() {
        let dir = std::env::temp_dir().join(format!("sailor-ledger-event-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("a ledger");
        ledger
            .record_run(&crate::RunRecord {
                run_id: "r1".to_owned(),
                kind: "flow".to_owned(),
                entity: "sweep-the-tree".to_owned(),
                parent_run_id: Some("quokka".to_owned()),
                started_by: "window".to_owned(),
                status: "running".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 1,
                ended_at: None,
                worktree: None,
            })
            .expect("a run");
        let ids: Vec<String> = ledger
            .search("quokka", 100, 100, 100)
            .expect("a ranking")
            .into_iter()
            .map(|hit| hit.id)
            .collect();
        let none_when_events_are_left_out: Vec<String> = ledger
            .search("quokka", 100, 100, 0)
            .expect("a ranking")
            .into_iter()
            .map(|hit| hit.id)
            .collect();
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(ids, vec!["event:1"]);
        assert!(none_when_events_are_left_out.is_empty(), "{none_when_events_are_left_out:?}");
    }

    #[test]
    fn a_search_with_no_word_is_refused() {
        assert!(matches!(rank_texts(&documents(), "  a "), Err(LedgerError::Refused(_))));
    }
}
