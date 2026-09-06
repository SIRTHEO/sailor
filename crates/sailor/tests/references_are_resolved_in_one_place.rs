//! **REFERENCES ARE RESOLVED IN ONE PLACE ONLY, AND THIS CHECK DEMANDS IT.**
//!
//! **WHY IT EXISTS.** Fault 28 was not that the ledger failed to resolve
//! references: it was that *resolving references* was each single action's
//! business. From there the defect came back twice in the same shape. First as
//! absence — two actions out of nine had it, and the other seven received
//! `{"$from": …}` as an object. Then as a **cure worse than the disease**: the
//! line was copied, and this tree once held **twelve out of sixteen registered
//! actions**, with four still uncovered (`history_ask`, `detect_tools`,
//! `trigger`, `subflow`). Twelve copies of the same line are fault 10 in twelve
//! specimens, and every new action kept being born with nothing turning red.
//!
//! **WHAT IT PREVENTS, THAT A BEHAVIOUR TEST CANNOT.**
//! `crates/flow/tests/a_reference_reaches_every_action.rs` proves the input
//! arrives resolved; it would stay **green** if tomorrow someone put the line
//! back inside an action, because the behaviour would not change. What would
//! change is the number of places the rule lives in — and that is the fault. It
//! is measured by counting the places, and the counting happens here.
//!
//! **WHY IT READS CLEANED-UP CODE, AND THE FIRST VERSION WAS BLIND.** Skipping
//! `#[cfg(test)]` blocks means counting braces, and the first draft counted them
//! on the raw text: the braces inside strings and comments went into the count.
//! A block that does not balance **swallows everything to the end of the file**,
//! in silence. Measured on shipped, untouched code: five blind files —
//! `actions/src/lib.rs` from line 4200, `models/src/remaining.rs` from 325,
//! `models/src/store.rs` from 37, `sailor/src/flow_cmd.rs` from 2448,
//! `ui/src/gather.rs` from 240 — and a real, compiling function carrying a call
//! to `resolve_references`, placed at the end of `flow_cmd.rs`: **the test did
//! not see it**, two greens and zero reds.
//!
//! The defect was not the false positive, which shows: it was the **silent false
//! negative**, exactly what this file exists for. The cure is double and the two
//! halves serve different ends: braces are counted on code **cleaned** of
//! comments, strings and character literals; and if a skip still runs to the end
//! of the file, the test **turns red naming the file** instead of carrying on
//! blind. A check that can switch itself off is not a check.
//!
//! **IT IS NOT A PARSER.** It does not claim to understand Rust: it recognises
//! comments, strings (raw ones included) and character literals, and nothing
//! else. The price is declared, and it is how this house writes text checks —
//! like `identifiers_are_in_english`. But when it does not understand, it stops
//! and says so.

use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// The two functions that resolve references, looked for as text: the
/// executor's, which walks the `with` alone, and the whole one the tests use.
const THE_CALLS: &[&str] = &["resolve_references(", "resolve_overlay("];

fn names_the_call(text: &str) -> bool {
    THE_CALLS.iter().any(|call| text.contains(call))
}

/// The only two shipped files that may name it, with the reason why.
///
/// **NOT A LIST TO LENGTHEN.** One more entry here is one more copy of the rule,
/// which is the fault this check exists to stop. Whoever needs references in a
/// new place already receives them resolved: they go through `step_input` like
/// everyone else.
const WHERE_IT_MAY_LIVE: [(&str, &str); 2] = [
    (
        "crates/flow/src/reference.rs",
        "è la funzione stessa, e le sue prove",
    ),
    (
        "crates/flow/src/executor.rs",
        "è `step_input`, dove l'ingresso di ogni passo si compone: l'unico posto attraversato da tutte le azioni",
    ),
];

/// Every `.rs` under `crates/*/src`, that is the code running in production.
///
/// The `tests/` directories stay out on purpose: a test calling `execute`
/// directly must be able to compose the input the way the executor would, and
/// calling the real function to do it is right — copying its *decision* into an
/// action is the defect.
fn shipped_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let crates = root.join("crates");
    let entries = std::fs::read_dir(&crates).expect("la cartella dei crate esiste");
    for entry in entries.flatten() {
        let src = entry.path().join("src");
        if src.is_dir() {
            collect_rust_files(&src, &mut found);
        }
    }
    found.sort();
    found
}

fn collect_rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// The file with comments, strings and character literals replaced by spaces.
///
/// **THE LINES STAY WHERE THEY ARE**: every newline inside what is taken out is
/// put back, so the line number of a violation is the true one and whoever reads
/// the message opens the file at the right point.
///
/// **A QUOTE IS NOT ALWAYS A CHARACTER**: `'a` is a lifetime, `'{'` is a brace
/// that does not count. They are told apart by looking ahead — a character
/// literal closes within two steps — and what does not close is a lifetime, of
/// which only the quote is thrown away. Without this distinction any lifetime
/// would make all the code up to the next quote disappear.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut kept = String::with_capacity(text.len());
    let mut index = 0usize;

    // Copies the newlines of a discarded piece, so the lines do not shift.
    let skip_keeping_lines = |kept: &mut String, from: usize, to: usize| {
        for character in &chars[from..to.min(chars.len())] {
            if *character == '\n' {
                kept.push('\n');
            }
        }
    };

    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();

        // Line comment, documentation included.
        if current == '/' && next == Some('/') {
            let mut end = index;
            while end < chars.len() && chars[end] != '\n' {
                end += 1;
            }
            index = end;
            continue;
        }

        // Block comment, which in Rust nests.
        if current == '/' && next == Some('*') {
            let mut depth = 1usize;
            let mut end = index + 2;
            while end < chars.len() && depth > 0 {
                if chars[end] == '/' && chars.get(end + 1) == Some(&'*') {
                    depth += 1;
                    end += 2;
                } else if chars[end] == '*' && chars.get(end + 1) == Some(&'/') {
                    depth -= 1;
                    end += 2;
                } else {
                    end += 1;
                }
            }
            skip_keeping_lines(&mut kept, index, end);
            index = end;
            continue;
        }

        // Raw string: `r"…"`, `r#"…"#`, `br##"…"##`. The prefix counts only if
        // it is not the tail of an identifier.
        if (current == 'r' || current == 'b') && !preceded_by_identifier(&chars, index) {
            if let Some((end, newlines)) = raw_string_end(&chars, index) {
                for _ in 0..newlines {
                    kept.push('\n');
                }
                index = end;
                continue;
            }
        }

        // Ordinary string, with its escape sequences.
        if current == '"' {
            let mut end = index + 1;
            while end < chars.len() {
                if chars[end] == '\\' {
                    end += 2;
                    continue;
                }
                if chars[end] == '"' {
                    end += 1;
                    break;
                }
                end += 1;
            }
            skip_keeping_lines(&mut kept, index, end);
            index = end.min(chars.len());
            continue;
        }

        // Character literal, told apart from a lifetime.
        if current == '\'' {
            if let Some(end) = char_literal_end(&chars, index) {
                index = end;
                continue;
            }
            // A lifetime: only the quote goes, the name stays and holds no
            // braces.
            index += 1;
            continue;
        }

        kept.push(current);
        index += 1;
    }
    kept
}

fn preceded_by_identifier(chars: &[char], index: usize) -> bool {
    index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '_')
}

/// Where a raw string starting at `index` ends, and how many lines it spans.
/// `None` if none starts there.
fn raw_string_end(chars: &[char], index: usize) -> Option<(usize, usize)> {
    let mut cursor = index;
    if chars[cursor] == 'b' {
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'r') {
        return None;
    }
    cursor += 1;
    let mut hashes = 0usize;
    while chars.get(cursor) == Some(&'#') {
        hashes += 1;
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'"') {
        return None;
    }
    cursor += 1;
    let mut newlines = 0usize;
    while cursor < chars.len() {
        if chars[cursor] == '\n' {
            newlines += 1;
        }
        if chars[cursor] == '"' {
            let closes = (1..=hashes).all(|step| chars.get(cursor + step) == Some(&'#'));
            if closes {
                return Some((cursor + 1 + hashes, newlines));
            }
        }
        cursor += 1;
    }
    Some((chars.len(), newlines))
}

/// Where a character literal starting at `index` ends. `None` when that quote
/// opens a lifetime and not a character.
fn char_literal_end(chars: &[char], index: usize) -> Option<usize> {
    match chars.get(index + 1) {
        Some('\\') => {
            // `'\n'`, `'\''`, `'\u{7b}'`: the closing quote is looked for just
            // ahead, without chasing it through the whole file.
            //
            // **THE SEARCH STARTS AT `index + 3`, AND ONE CHARACTER EARLIER WAS
            // A HOLE.** With `index + 2` it starts at the **escaped** character:
            // on `'\''` that is a quote, so the literal was closed one character
            // too soon. The leftover quote recombined with what followed — in
            // `['\'','"']` the piece `','` passed for a character — and the next
            // `"` opened a **ghost string** that silently erased everything up
            // to the `"` after it, shipped code included. And `'\''` already
            // sits in three places in the tree (`inventory/src/lib.rs:573` and
            // `:601`, `terminal/src/routing.rs:313`), saved only by the space
            // rustfmt puts after the comma: a check that switches off over a
            // spacing switches off by accident.
            (index + 3..=index + 10)
                .find(|at| chars.get(*at) == Some(&'\''))
                .map(|at| at + 1)
        }
        Some(_) if chars.get(index + 2) == Some(&'\'') => Some(index + 3),
        _ => None,
    }
}

/// The shipped code of a file: cleaned, and without the `#[cfg(test)]` blocks.
///
/// **A SKIP THAT DOES NOT CLOSE IS AN ERROR, NOT A SILENCE.** If the braces do
/// not balance before the end of the file, `Err` is returned with the line the
/// block started at: from there on the check would see nothing, and that is how
/// this test has already been blind on five files.
fn shipped_code(text: &str) -> Result<String, usize> {
    let code = code_only(text);
    let mut kept = String::with_capacity(code.len());
    let mut lines = code.lines().enumerate().peekable();
    while let Some((number, line)) = lines.next() {
        if !line.trim_start().starts_with("#[cfg(test)]") {
            kept.push_str(line);
            kept.push('\n');
            continue;
        }
        kept.push('\n');
        let mut depth: i32 = 0;
        let mut entered = false;
        let mut closed = false;
        for (_, skipped) in lines.by_ref() {
            kept.push('\n');
            depth += skipped.matches('{').count() as i32;
            depth -= skipped.matches('}').count() as i32;
            if depth > 0 {
                entered = true;
            } else if entered || !skipped.contains('{') {
                closed = true;
                break;
            }
        }
        if !closed {
            return Err(number + 1);
        }
    }
    Ok(kept)
}

#[test]
fn nothing_but_the_place_where_the_input_is_composed_resolves_references() {
    let root = repository_root();
    let allowed: Vec<&str> = WHERE_IT_MAY_LIVE.iter().map(|(path, _)| *path).collect();

    let mut copies: Vec<String> = Vec::new();
    let mut blind: Vec<String> = Vec::new();
    let files = shipped_sources(&root);
    for file in &files {
        let relative = file
            .strip_prefix(&root)
            .expect("i file vengono da sotto la radice")
            .display()
            .to_string();
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|error| panic!("leggere {relative}: {error}"));
        let code = match shipped_code(&text) {
            Ok(code) => code,
            Err(line) => {
                blind.push(format!("{relative}:{line}"));
                continue;
            }
        };
        for (number, line) in code.lines().enumerate() {
            if names_the_call(line) && !allowed.contains(&relative.as_str()) {
                copies.push(format!("{relative}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    workspace::measured_against(
        files.len(),
        "shipped sources read",
        THE_CALLS.len(),
        "calls that resolve a reference",
    );

    assert!(
        blind.is_empty(),
        "in questi file uno scavalcamento `#[cfg(test)]` non si chiude prima della \
         fine, quindi da lì in poi questo controllo non guarda più niente:\n  {}\n\n\
         Non è un dettaglio di conteggio: è il modo in cui questa prova è stata \
         cieca su cinque file spediti senza dirlo a nessuno. Prima si ripara il \
         modo di leggere, poi si può credere al verde.",
        blind.join("\n  ")
    );
    assert!(
        copies.is_empty(),
        "i rinvii si sciolgono in un posto solo — `flow::step_input` — e qui ne \
         compaiono altri {}:\n  {}\n\nUn'azione non risolve i propri rinvii: li \
         riceve già sciolti, come riceve già risolto il `workdir`. Ricopiare \
         questa riga è il guasto 28 daccapo — la volta scorsa sono diventate \
         dodici copie e quattro azioni scoperte, e nessun controllo lo diceva.",
        copies.len(),
        copies.join("\n  ")
    );
}

/// **AND THE DECLARED PLACE MUST REALLY BE OCCUPIED.**
///
/// Without this half, the test above would go green even with the resolution
/// removed everywhere: zero copies, and zero cures. It is the same defect as the
/// `partly` field of fault 40, computed and never questioned — a defence that
/// can be satisfied by doing nothing.
#[test]
fn the_one_place_that_may_resolve_them_actually_does() {
    let root = repository_root();
    let composer = root.join("crates/flow/src/executor.rs");
    let text = std::fs::read_to_string(&composer).expect("leggere l'esecutore");
    let shipped = shipped_code(&text).expect("l'esecutore si legge fino in fondo");

    assert!(
        names_the_call(&shipped),
        "`crates/flow/src/executor.rs` non scioglie più nessun rinvio: allora \
         non li scioglie nessuno, e ogni `{{\"$from\": …}}` arriva alle azioni \
         come oggetto"
    );
    let inside_step_input = shipped
        .split_once("pub fn step_input(")
        .map(|(_, after)| after.to_owned())
        .expect("`step_input` esiste");
    assert!(
        names_the_call(&inside_step_input),
        "la chiamata non sta più dentro `step_input`: fuori di lì non è più \
         l'unico punto attraversato da ogni passo"
    );
}

// ── that the way of reading really sees ──────────────────────────────────
//
// **WHOEVER MEASURES MUST BE MEASURED.** The three tests below question the
// reader, not the shipped code: they were born because the first version of
// this file passed, green, with a real call right in front of its eyes.

/// Braces inside strings, comments and characters **do not count**, and an
/// escaped quote does not open a ghost string.
///
/// Each of these cases, alone, was enough to blind a version of this reader. The
/// last — `'\''` **without a space** after the comma — is the second hole, found
/// after the first repair: the literal closed one character too soon, the `","`
/// that followed passed for a character, and the `"` after it opened a string
/// that swallowed the shipped code up to the next string. The `const TAIL` at
/// the bottom is there on purpose: it is the `"` that closed the ghost string,
/// which is what made the defect silent instead of loud.
///
/// **THE SPACE IS NOT A STYLE DETAIL.** With `['\'', '"']`, the way rustfmt
/// writes it, the defect did not show; without it, it did. A check depending on
/// a spacing switches off by accident, and `'\''` already sits in three places
/// in the tree.
#[test]
fn braces_inside_strings_and_comments_do_not_count() {
    let text = r####"
#[cfg(test)]
mod tests {
    fn a() {
        let _ = "una graffa aperta { e basta";
        // un commento con } dentro
        let _ = '{';
        let _ = r#"una graffa grezza {"#;
    }
}

const QUOTES: [char; 2] = ['\'','"'];

fn shipped() {
    let _ = resolve_references(&input);
}

const TAIL: &str = "coda";
"####;

    let code = shipped_code(text).expect("il blocco di prova si chiude");

    assert!(
        names_the_call(&code),
        "il codice spedito dopo il blocco di prova è sparito:\n{code}"
    );
    assert!(
        !code.contains("una graffa aperta"),
        "le stringhe di prova non devono restare"
    );
    assert!(
        !code.contains("coda"),
        "e nemmeno quella in fondo: se resta, la stringa fantasma non c'è mai stata \
         e questa prova non sta misurando il caso che dice di misurare"
    );
}

/// The other escapes are still read whole, and it is not a formality: the cure
/// was to move the start of the search by one character, and one character more
/// would break `'\n'` with nothing to say so.
///
/// **EVERY CASE CARRIES THE TAIL THAT BITES.** With the fixture written as
/// `const C: char = <literal>;` it stayed **green under all three mutations** of
/// the escape branch — `index + 4` included, the very opposite repair the
/// comment above promised to defend against. The reason: a literal read crooked,
/// with no double quote beside it, opens no ghost string, so it shifts nothing
/// and both assertions stay true whatever is done. It was a test that could not
/// come out any other way — the house measure applied to itself.
///
/// Now the literal sits inside `[<literal>,'"']`: if it is consumed by the wrong
/// number of characters, the double quote that follows becomes the opening of a
/// string that closes only on the `"` of `const T`, and the code in between
/// disappears with it. The two assertions can start to fail.
#[test]
fn the_other_escaped_characters_are_still_read_whole() {
    for (literal, tail) in [
        ("'\\n'", "riga"),
        ("'\\\\'", "barra"),
        ("'\\u{7b}'", "graffa"),
        ("'\\''", "apice"),
    ] {
        let text = format!(
            "const C: [char; 2] = [{literal},'\"'];\nfn shipped() {{ let _ = resolve_references(&input); }}\nconst T: &str = \"{tail}\";\n"
        );

        let code = shipped_code(&text).expect("niente blocchi di prova");

        assert!(
            names_the_call(&code),
            "dopo {literal} il codice spedito è sparito:\n{code}"
        );
        assert!(
            !code.contains(tail),
            "dopo {literal} la stringa in fondo è sopravvissuta: il letterale è stato \
             letto storto e ha spostato tutto ciò che segue"
        );
    }
}

/// **A SKIP THAT DOES NOT CLOSE IS DECLARED.** It is the case that used to pass
/// in silence, and the silence was the defect.
#[test]
fn a_test_block_that_never_closes_is_reported_instead_of_swallowing_the_file() {
    let text = "fn prima() {}\n#[cfg(test)]\nmod tests {\n    fn a() {\n";

    let refused = shipped_code(text).expect_err("il blocco non si chiude");

    assert_eq!(refused, 2, "la riga da cui il file diventa cieco");
}

/// The cleaned code keeps the line numbers: a message that sends you to the
/// wrong line makes you search in the wrong place, and it is fault 11 in
/// miniature.
#[test]
fn cleaning_the_code_keeps_the_line_numbers() {
    let text = "uno\n/* due\n   tre */\nquattro\n";

    let code = code_only(text);

    assert_eq!(code.lines().count(), 4, "{code:?}");
    assert_eq!(code.lines().nth(3), Some("quattro"));
}
