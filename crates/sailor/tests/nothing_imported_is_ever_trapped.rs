//! A note taken into the store comes back out whole. The command is only worth
//! typing if the round trip is lossless: a person who imports a working note
//! and deletes the file has to be able to get that same file back.

use ledger::Ledger;
use sailor::notes_cmd::run;

/// A document with everything that tempts a reader-writer to tidy it: a fenced
/// block, a table, an accented sentence, a line that is only spaces, and a
/// trailing newline. Each one has been silently eaten by some markdown
/// pipeline or other.
const AWKWARD: &str = "# Una nota storta\n\
     \n\
     Però la città è già passata: accenti, e una riga sola.\n\
     \n\
     | colonna | altra |\n\
     | ------- | ----- |\n\
     | uno     | due   |\n\
     \n\
     ```rust\n\
     fn main() { println!(\"{}\", \"a fence\"); }\n\
     ```\n\
     \n   \nafter a line of three spaces\n";

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sailor-notes-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn typed(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn said(path: &std::path::Path) -> String {
    path.display().to_string()
}

/// **WHAT GOES IN COMES BACK OUT, BYTE FOR BYTE.** Compared as bytes and not as
/// text: a trailing newline and a line of spaces are both invisible to the eye
/// and both part of the document.
#[test]
fn a_note_rendered_back_is_the_file_that_was_imported_byte_for_byte() {
    let dir = scratch("round-trip");
    let store = said(&dir.join("store"));
    let source = dir.join("una-nota-storta.md");
    std::fs::write(&source, AWKWARD).expect("the file to import");
    let back = dir.join("back-out.md");

    assert_eq!(
        run(&typed(&["import", &said(&source), "--store", &store])),
        0,
        "the import did not go through"
    );
    assert_eq!(
        run(&typed(&[
            "render",
            "una-nota-storta",
            "--file",
            &said(&back),
            "--store",
            &store
        ])),
        0,
        "the render did not go through"
    );

    let imported = std::fs::read(&source).expect("the file imported");
    let rendered = std::fs::read(&back).expect("the file rendered");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        rendered.len(),
        imported.len(),
        "the round trip changed the length: {} bytes in, {} bytes out",
        imported.len(),
        rendered.len()
    );
    assert_eq!(
        rendered, imported,
        "the round trip changed the bytes; the note is not the file that was imported"
    );
}

/// The slug is the address, and two documents at one address is the state
/// nobody can read back: a second import replaces the first.
#[test]
fn importing_the_same_slug_replaces_it_instead_of_filing_a_second() {
    let dir = scratch("replace");
    let store = dir.join("store");
    let source = dir.join("a-working-note.md");

    std::fs::write(&source, "# First\n\nthe first body\n").expect("the first file");
    assert_eq!(run(&typed(&["import", &said(&source), "--store", &said(&store)])), 0);
    std::fs::write(&source, "# Second\n\nthe second body\n").expect("the second file");
    assert_eq!(run(&typed(&["import", &said(&source), "--store", &said(&store)])), 0);

    let ledger = Ledger::open(&store).expect("a ledger");
    let held = actions::notes::all(&ledger).expect("the notes held");
    let again = actions::notes::import(
        &ledger,
        actions::notes::Note {
            slug: "a-working-note".to_owned(),
            title: "Third".to_owned(),
            text: "the third body\n".to_owned(),
            tree: None,
            imported_at: 9,
            removed_at: None,
        },
    )
    .expect("taken in");
    drop(ledger);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        held.iter().map(|note| note.slug.as_str()).collect::<Vec<_>>(),
        vec!["a-working-note"],
        "two imports of one slug left more than one note"
    );
    assert_eq!(held[0].title, "Second", "the second import did not take the first's place");
    assert!(held[0].text.contains("the second body"), "{}", held[0].text);
    assert!(again.replaced, "the import did not say it had replaced anything");
}

/// A note taken out stops being held, and its text leaves the row the store
/// answers with — otherwise «remove» would only hide it from one listing.
#[test]
fn a_note_removed_is_no_longer_held_and_its_text_leaves_the_store() {
    let dir = scratch("remove");
    let store = dir.join("store");
    let source = dir.join("a-note-to-drop.md");
    std::fs::write(&source, "# To drop\n\nthe body of a note nobody wants kept\n")
        .expect("the file to import");

    assert_eq!(run(&typed(&["import", &said(&source), "--store", &said(&store)])), 0);
    assert_eq!(
        run(&typed(&["remove", "a-note-to-drop", "--store", &said(&store)])),
        0,
        "the removal did not go through"
    );
    assert_eq!(
        run(&typed(&["show", "a-note-to-drop", "--store", &said(&store)])),
        1,
        "a note taken out still shows"
    );

    let ledger = Ledger::open(&store).expect("a ledger");
    let held = actions::notes::all(&ledger).expect("the notes held");
    let row = ledger
        .read_record(actions::notes::NOTES_COLLECTION, "a-note-to-drop")
        .expect("the store answers");
    drop(ledger);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(held.is_empty(), "the note is still held: {held:?}");
    let row = row.expect("the slug is still the mark that a note was there").value;
    assert!(
        !row.to_string().contains("nobody wants kept"),
        "the text is still in the row the store answers with: {row}"
    );
}
