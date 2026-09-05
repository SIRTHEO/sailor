//! Full-text ranking over texts handed in. SQLite's FTS5 is already compiled
//! into this crate, and an index rebuilt in memory from a few dozen documents
//! costs less than keeping one fresh on disk.

use crate::LedgerError;
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

    #[test]
    fn a_search_with_no_word_is_refused() {
        assert!(matches!(rank_texts(&documents(), "  a "), Err(LedgerError::Refused(_))));
    }
}
