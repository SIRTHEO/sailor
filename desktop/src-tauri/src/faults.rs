//! The register of what has broken, as the window asks for it.
//!
//! **THE FOURTH ANSWER TRAVELS.** `Standing` has `Unrecognised` on purpose: a
//! predicate answering yes or no gives «not open» to prose nobody taught it,
//! which is the answer a closed fault gets, and the tally drops in the
//! reassuring direction. Flattening it here would undo that at the crossing.

use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Entry {
    number: i64,
    happened_on: String,
    what_happened: String,
    how_it_showed: String,
    /// **THE COLUMN THAT SEPARATES THIS FROM A DIARY.** A fault with nothing
    /// here is not finished, and the window says so rather than drawing a row
    /// that looks complete.
    what_would_prevent: String,
    /// The prose exactly as the register holds it.
    status: String,
    /// How that prose reads: `open`, `partly closed`, `closed`, or
    /// `unrecognised` — never guessed from the text by this side.
    standing: &'static str,
}

#[derive(Serialize)]
pub(crate) struct Register {
    entries: Vec<Entry>,
    /// Where the register is, so a reader can go and look at it.
    path: String,
    /// How many are still open, counted by the engine and not here.
    still_open: usize,
}

fn standing(fault: &::faults::Fault) -> &'static str {
    match fault.standing {
        ::faults::Standing::Open => "open",
        ::faults::Standing::PartlyClosed => "partly closed",
        ::faults::Standing::Closed => "closed",
        ::faults::Standing::Unknown => "unknown",
    }
}

fn store() -> Result<::faults::Faults, String> {
    let path = ::faults::Faults::default_path().map_err(|error| error.to_string())?;
    ::faults::Faults::open(path).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn faults() -> Result<Register, String> {
    let store = store()?;
    let all = store.all().map_err(|error| error.to_string())?;
    Ok(Register {
        path: store.path().display().to_string(),
        still_open: store.still_open().map_err(|error| error.to_string())?,
        entries: all
            .iter()
            .map(|fault| Entry {
                number: fault.number,
                happened_on: fault.happened_on.clone(),
                what_happened: fault.what_happened.clone(),
                how_it_showed: fault.how_it_showed.clone(),
                what_would_prevent: fault.what_would_prevent.clone(),
                status: fault.status.clone(),
                standing: standing(fault),
            })
            .collect(),
    })
}

#[tauri::command]
pub(crate) fn fault_status(number: i64, status: String) -> Result<(), String> {
    store()?
        .set_status(number, &status)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    /// **A STANDING THE REGISTER COULD NOT CLASSIFY MUST NOT READ AS CLOSED.**
    /// The vocabulary is validated in the register now, so this reader only
    /// puts a word on it — but «unknown» and «closed» are the two the window
    /// must never merge: one asks a person to look, the other says nobody has to.
    #[test]
    fn every_standing_the_register_holds_gets_a_word_of_its_own() {
        let with = |standing| ::faults::Fault {
            number: 1,
            happened_on: "2026-09-02".to_owned(),
            happened: ::faults::Happening::default(),
            what_happened: "x".to_owned(),
            how_it_showed: "y".to_owned(),
            what_would_prevent: "z".to_owned(),
            status: "whatever the prose says".to_owned(),
            standing,
        };
        let said: Vec<&str> = ::faults::EVERY_STANDING
            .iter()
            .map(|standing| super::standing(&with(*standing)))
            .collect();

        assert_eq!(said, ["open", "partly closed", "closed", "unknown"]);
        let mut apart = said.clone();
        apart.sort_unstable();
        apart.dedup();
        assert_eq!(apart.len(), said.len(), "two standings share one word");
    }
}
