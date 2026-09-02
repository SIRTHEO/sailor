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
    match fault.standing() {
        ::faults::Standing::Open => "open",
        ::faults::Standing::PartlyClosed => "partly closed",
        ::faults::Standing::Closed => "closed",
        ::faults::Standing::Unrecognised => "unrecognised",
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
    /// **A STATUS NOBODY TAUGHT THIS MUST NOT READ AS CLOSED.** The register is
    /// prose written by people; the day somebody writes a status in another
    /// wording, the count of what is still open must refuse it rather than
    /// quietly subtract it.
    #[test]
    fn prose_the_register_does_not_know_is_refused_not_closed() {
        let unknown = ::faults::Fault {
            number: 1,
            happened_on: "2026-09-02".to_owned(),
            what_happened: "x".to_owned(),
            how_it_showed: "y".to_owned(),
            what_would_prevent: "z".to_owned(),
            status: "mezzo sistemato, credo".to_owned(),
        };
        assert_eq!(super::standing(&unknown), "unrecognised");

        // THE CONTROL: the words it does know still read as themselves, or the
        // check above would hold on a function that answers one thing always.
        let closed = ::faults::Fault {
            status: "**chiuso**".to_owned(),
            ..unknown.clone()
        };
        let open = ::faults::Fault {
            status: "**aperto**".to_owned(),
            ..unknown.clone()
        };
        let half = ::faults::Fault {
            status: "**chiuso in parte**".to_owned(),
            ..unknown.clone()
        };
        assert_eq!(
            [
                super::standing(&closed),
                super::standing(&open),
                super::standing(&half)
            ],
            ["closed", "open", "partly closed"],
        );
    }
}
