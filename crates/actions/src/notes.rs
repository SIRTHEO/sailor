//! A written document Sailor keeps, so a working note need not live in the
//! repository: the text goes into the store's `notes` collection whole, and
//! comes back out byte for byte.
//!
//! A new collection asks the ledger for nothing beyond `put_record`: the
//! `store` table is addressed by collection and key, so no projection moves.

use ledger::{Ledger, LedgerError, StoreRecord};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const NOTES_COLLECTION: &str = "notes";

/// What the store writes in its own `written_by` column.
const WRITTEN_BY: &str = "notes";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub slug: String,
    pub title: String,
    /// The file as it came in. **Nothing is trimmed**: a trailing newline is
    /// part of the document, and a render that dropped it would hand back a
    /// file that is not the one imported.
    #[serde(default)]
    pub text: String,
    /// The tree it was imported in, as [`crate::memory::tree_of`] names it.
    #[serde(default)]
    pub tree: Option<String>,
    pub imported_at: i64,
    /// When it was taken out. The text leaves with it, so a note removed stops
    /// answering a search as well as a listing.
    #[serde(default)]
    pub removed_at: Option<i64>,
}

impl Note {
    /// Whether the note is still one the store holds.
    pub fn kept(&self) -> bool {
        self.removed_at.is_none()
    }

    pub fn bytes(&self) -> usize {
        self.text.len()
    }
}

/// What an import did: the note as it now stands, and whether it took the
/// place of one already under that slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub note: Note,
    pub replaced: bool,
}

/// The slug a file is filed under: its name without the suffix, lowercased,
/// its words joined by dashes.
pub fn slug_of(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::memory::label_key(&stem)
}

/// The title a document declares: the first heading in it, and the slug when
/// it declares none.
pub fn title_of(text: &str, slug: &str) -> String {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix('#'))
        .map(|rest| rest.trim_start_matches('#').trim())
        .filter(|found| !found.is_empty())
        .map_or_else(|| slug.to_owned(), str::to_owned)
}

/// Takes a note in. **A second import under the same slug replaces the first**
/// rather than filing a copy beside it: the slug is the address, and two
/// documents at one address is the state nobody can read back.
pub fn import(ledger: &Ledger, note: Note) -> Result<Imported, LedgerError> {
    if note.slug.trim().is_empty() {
        return Err(LedgerError::Refused("a note needs a slug".to_owned()));
    }
    let replaced = read(ledger, &note.slug)?.is_some_and(|held| held.kept());
    write_note(ledger, &note)?;
    Ok(Imported { note, replaced })
}

fn write_note(ledger: &Ledger, note: &Note) -> Result<(), LedgerError> {
    ledger.put_record(&StoreRecord {
        collection: NOTES_COLLECTION.to_owned(),
        key: note.slug.clone(),
        value: serde_json::to_value(note)?,
        written_by: WRITTEN_BY.to_owned(),
        written_at: note.imported_at,
    })
}

/// What the store holds under a slug, taken out or not.
pub fn read(ledger: &Ledger, slug: &str) -> Result<Option<Note>, LedgerError> {
    let Some(record) = ledger.read_record(NOTES_COLLECTION, slug)? else {
        return Ok(None);
    };
    Ok(serde_json::from_value(record.value).ok())
}

/// Every note still held, newest first; the slug settles a tie so two imported
/// in the same second do not swap places between two listings.
pub fn all(ledger: &Ledger) -> Result<Vec<Note>, LedgerError> {
    let mut found: Vec<Note> = ledger
        .records_in(NOTES_COLLECTION)?
        .into_iter()
        .filter_map(|record| serde_json::from_value::<Note>(record.value).ok())
        .filter(Note::kept)
        .collect();
    found.sort_by(|left, right| {
        right
            .imported_at
            .cmp(&left.imported_at)
            .then(left.slug.cmp(&right.slug))
    });
    Ok(found)
}

/// Takes a note out, answering whether there was one to take.
///
/// The event log keeps every write, as it does for a memory; what leaves is the
/// row the store answers from, and the text with it.
pub fn remove(ledger: &Ledger, slug: &str, at: i64) -> Result<bool, LedgerError> {
    let Some(held) = read(ledger, slug)?.filter(Note::kept) else {
        return Ok(false);
    };
    let gone = Note {
        text: String::new(),
        removed_at: Some(at),
        ..held
    };
    write_note(ledger, &gone)?;
    Ok(true)
}

/// An instant as a person reads it, **in UTC and never the machine's zone**:
/// what one person sees on a listing, another sees on theirs.
pub fn instant(seconds: i64) -> String {
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let inside = seconds.rem_euclid(86_400);
    let (hour, minute) = (inside / 3_600, (inside % 3_600) / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Days since the epoch as a calendar date. The year is shifted to start in
/// March, which puts the leap day last and spares a table of month lengths.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-notes-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn note(slug: &str, text: &str, at: i64) -> Note {
        Note {
            slug: slug.to_owned(),
            title: title_of(text, slug),
            text: text.to_owned(),
            tree: None,
            imported_at: at,
            removed_at: None,
        }
    }

    /// The slug comes off the file name, and the title off the first heading —
    /// or off the slug when the document declares none.
    #[test]
    fn the_slug_is_the_file_name_and_the_title_is_the_first_heading() {
        assert_eq!(slug_of(Path::new("/a/place/A Night's Log.md")), "a-night-s-log");
        assert_eq!(title_of("## The second level\n\ntext", "a-slug"), "The second level");
        assert_eq!(title_of("no heading at all\n", "a-slug"), "a-slug");
    }

    /// Newest first, and a tie broken by the slug rather than by whatever the
    /// store happened to return.
    #[test]
    fn the_listing_is_newest_first() {
        let dir = scratch("listing");
        let ledger = Ledger::open(&dir).expect("a ledger");
        for (slug, at) in [("older", 10), ("newer", 30), ("also-newer", 30)] {
            import(&ledger, note(slug, "a body", at)).expect("imported");
        }
        let held: Vec<String> = all(&ledger).expect("a listing").into_iter().map(|one| one.slug).collect();
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(held, vec!["also-newer", "newer", "older"]);
    }

    /// A date this arithmetic gets wrong would put every note in the wrong
    /// year, and no other check reads it.
    #[test]
    fn an_instant_reads_as_a_date_in_utc() {
        assert_eq!(instant(0), "1970-01-01 00:00");
        assert_eq!(instant(951_782_400), "2000-02-29 00:00");
        assert_eq!(instant(1_757_030_400), "2025-09-05 00:00");
    }
}
