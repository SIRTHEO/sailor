//! The families of the inventory are spelled out in one place, and this
//! judge insists on it. A behaviour test stays green when a copy of the list
//! comes back: only the number of places changes, and that is counted here by
//! reading the shipped sources. See fault 10.

use std::path::{Path, PathBuf};

/// The one file that may spell the whole list, in both of its forms.
const THE_SOURCE: &str = "crates/inventory/src/lib.rs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    VariantPaths,
    Labels,
}

impl Form {
    fn describe(self) -> &'static str {
        match self {
            Form::VariantPaths => "form (a), the variant paths `Kind::…` together",
            Form::Labels => "form (b), the lowercase labels together",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Place {
    line: usize,
    form: Form,
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

/// Every `.rs` under `crates/*/src`: the tests directories stay out, a test
/// may spell the list as fixture data.
fn shipped_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir(root.join("crates")).expect("the crates directory exists");
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

/// Two views of one file, character for character the same length, so an
/// offset in one is the same offset in the other: `code` has comments,
/// strings and character literals blanked; `with_strings` keeps the strings.
struct Views {
    code: Vec<char>,
    with_strings: Vec<char>,
}

fn blank(views: &mut Views, chars: &[char], from: usize, to: usize, keep_strings: bool) {
    for character in &chars[from..to.min(chars.len())] {
        let blanked = if *character == '\n' { '\n' } else { ' ' };
        views.code.push(blanked);
        views.with_strings.push(if keep_strings { *character } else { blanked });
    }
}

fn strip(text: &str) -> Views {
    let chars: Vec<char> = text.chars().collect();
    let mut views = Views {
        code: Vec::with_capacity(chars.len()),
        with_strings: Vec::with_capacity(chars.len()),
    };
    let mut index = 0usize;
    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();

        if current == '/' && next == Some('/') {
            let mut end = index;
            while end < chars.len() && chars[end] != '\n' {
                end += 1;
            }
            blank(&mut views, &chars, index, end, false);
            index = end;
            continue;
        }

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
            blank(&mut views, &chars, index, end, false);
            index = end;
            continue;
        }

        if (current == 'r' || current == 'b') && !preceded_by_identifier(&chars, index) {
            if let Some(end) = raw_string_end(&chars, index) {
                blank(&mut views, &chars, index, end, true);
                index = end;
                continue;
            }
        }

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
            let end = end.min(chars.len());
            blank(&mut views, &chars, index, end, true);
            index = end;
            continue;
        }

        // A quote opens either a character literal or a lifetime; a lifetime
        // keeps its name, only the quote goes.
        if current == '\'' {
            let end = char_literal_end(&chars, index).unwrap_or(index + 1);
            blank(&mut views, &chars, index, end, false);
            index = end;
            continue;
        }

        views.code.push(current);
        views.with_strings.push(current);
        index += 1;
    }
    views
}

fn preceded_by_identifier(chars: &[char], index: usize) -> bool {
    index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '_')
}

fn raw_string_end(chars: &[char], index: usize) -> Option<usize> {
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
    while cursor < chars.len() {
        if chars[cursor] == '"' {
            let closes = (1..=hashes).all(|step| chars.get(cursor + step) == Some(&'#'));
            if closes {
                return Some(cursor + 1 + hashes);
            }
        }
        cursor += 1;
    }
    Some(chars.len())
}

/// An escaped literal closes within a few characters, and the search starts
/// past the escaped one so `'\''` is not closed too early.
fn char_literal_end(chars: &[char], index: usize) -> Option<usize> {
    match chars.get(index + 1) {
        Some('\\') => (index + 3..=index + 10)
            .find(|at| chars.get(*at) == Some(&'\''))
            .map(|at| at + 1),
        Some(_) if chars.get(index + 2) == Some(&'\'') => Some(index + 3),
        _ => None,
    }
}

fn line_bounds(code: &[char]) -> Vec<(usize, usize)> {
    let mut bounds = Vec::new();
    let mut start = 0usize;
    for (at, character) in code.iter().enumerate() {
        if *character == '\n' {
            bounds.push((start, at));
            start = at + 1;
        }
    }
    if start < code.len() {
        bounds.push((start, code.len()));
    }
    bounds
}

/// Blanks every `#[cfg(test)]` block in both views, balancing braces on the
/// code view. A block that never closes is `Err(line)`: from there on the
/// judge would see nothing, and it says so instead of staying quiet.
fn without_test_blocks(mut views: Views) -> Result<Views, usize> {
    let bounds = line_bounds(&views.code);
    let mut to_blank: Vec<(usize, usize)> = Vec::new();
    let mut number = 0usize;
    while number < bounds.len() {
        let (start, end) = bounds[number];
        let line: String = views.code[start..end].iter().collect();
        if !line.trim_start().starts_with("#[cfg(test)]") {
            number += 1;
            continue;
        }
        let opened_at = number;
        let mut depth: i32 = 0;
        let mut entered = false;
        let mut closed = false;
        number += 1;
        while number < bounds.len() {
            let (start, end) = bounds[number];
            let skipped: String = views.code[start..end].iter().collect();
            number += 1;
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
            return Err(opened_at + 1);
        }
        to_blank.push((bounds[opened_at].0, bounds[number - 1].1));
    }
    for (from, to) in to_blank {
        for at in from..to {
            if views.code[at] != '\n' {
                views.code[at] = ' ';
                views.with_strings[at] = ' ';
            }
        }
    }
    Ok(views)
}

fn occurrences(hay: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return Vec::new();
    }
    (0..=hay.len() - needle.len())
        .filter(|at| &hay[*at..*at + needle.len()] == needle)
        .collect()
}

/// Every balanced `[…]`, `{…}` or `(…)` group of the code view, as offsets.
fn groups(code: &[char]) -> Vec<(usize, usize)> {
    let mut open: Vec<(char, usize)> = Vec::new();
    let mut found = Vec::new();
    for (at, character) in code.iter().enumerate() {
        match character {
            '[' | '{' | '(' => open.push((*character, at)),
            ']' | '}' | ')' => {
                let wants = match character {
                    ']' => '[',
                    '}' => '{',
                    _ => '(',
                };
                if let Some(position) = open.iter().rposition(|(opener, _)| *opener == wants) {
                    let (_, start) = open[position];
                    open.truncate(position);
                    found.push((start, at + 1));
                }
            }
            _ => {}
        }
    }
    found
}

fn needles(form: Form) -> Vec<Vec<char>> {
    inventory::Kind::ALL
        .iter()
        .map(|kind| match form {
            Form::VariantPaths => format!("Kind::{kind:?}"),
            Form::Labels => format!("\"{}\"", kind.label()),
        })
        .map(|needle| needle.chars().collect())
        .collect()
}

/// The innermost groups that hold the whole list: a function body around an
/// array is not a second place, the array is.
fn places_of(views: &Views, form: Form) -> Vec<Place> {
    let view = match form {
        Form::VariantPaths => &views.code,
        Form::Labels => &views.with_strings,
    };
    let hits: Vec<Vec<usize>> = needles(form)
        .iter()
        .map(|needle| occurrences(view, needle))
        .collect();
    let holding: Vec<(usize, usize)> = groups(&views.code)
        .into_iter()
        .filter(|(start, end)| {
            hits.iter()
                .all(|at| at.iter().any(|hit| hit >= start && hit < end))
        })
        .collect();
    let mut places: Vec<Place> = holding
        .iter()
        .filter(|(start, end)| {
            !holding
                .iter()
                .any(|(inner_start, inner_end)| {
                    (inner_start, inner_end) != (start, end)
                        && inner_start >= start
                        && inner_end <= end
                })
        })
        .map(|(start, _)| Place {
            line: views.code[..*start].iter().filter(|c| **c == '\n').count() + 1,
            form,
        })
        .collect();
    places.sort_by_key(|place| place.line);
    places
}

fn places(text: &str) -> Result<Vec<Place>, usize> {
    let views = without_test_blocks(strip(text))?;
    let mut found = places_of(&views, Form::VariantPaths);
    found.extend(places_of(&views, Form::Labels));
    found.sort_by_key(|place| place.line);
    Ok(found)
}

#[test]
fn the_families_of_the_inventory_are_spelled_out_only_in_their_source() {
    let root = repository_root();
    let files = shipped_sources(&root);
    let mut blind: Vec<String> = Vec::new();
    let mut copies: Vec<String> = Vec::new();
    let mut source_forms: Option<Vec<Form>> = None;

    for file in &files {
        let relative = file
            .strip_prefix(&root)
            .expect("the files come from under the root")
            .display()
            .to_string();
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|error| panic!("reading {relative}: {error}"));
        let found = match places(&text) {
            Ok(found) => found,
            Err(line) => {
                blind.push(format!("{relative}:{line}"));
                continue;
            }
        };
        if relative == THE_SOURCE {
            source_forms = Some(found.iter().map(|place| place.form).collect());
            continue;
        }
        for place in found {
            copies.push(format!(
                "{relative}:{}: {}",
                place.line,
                place.form.describe()
            ));
        }
    }
    workspace::measured_against(
        files.len(),
        "rust sources read",
        inventory::Kind::ALL.len(),
        "families the inventory has",
    );

    assert!(
        blind.is_empty(),
        "in these files a `#[cfg(test)]` block never closes, so from there on \
         this judge reads nothing:\n  {}",
        blind.join("\n  ")
    );
    assert!(!files.is_empty(), "no Rust source was read under crates/*/src");
    let source_forms = source_forms.unwrap_or_else(|| {
        panic!("{THE_SOURCE} was not read: the source of the families is gone")
    });
    for form in [Form::VariantPaths, Form::Labels] {
        assert!(
            source_forms.contains(&form),
            "{THE_SOURCE} no longer spells the families as {}: the source is gone, \
             and a judge that finds no copies because there is nothing to copy is blind",
            form.describe()
        );
    }
    assert!(
        copies.is_empty(),
        "the families of the inventory are listed in one place, `{THE_SOURCE}` \
         (`Kind::ALL` and `Kind::label`), and here they are spelled out again:\n  {}\n\n\
         One source, `inventory::Kind::ALL` and `Kind::label`; ask it instead of \
         copying it, see fault 10.",
        copies.join("\n  ")
    );
}

// ── the reader itself is measured ────────────────────────────────────────

const A_COPY: &str = "fn kinds() {\n    let kinds = [Kind::Skill, Kind::Agent, Kind::Command, Kind::Rule, Kind::Hook];\n}\n";

const A_LABEL_MATCH: &str = "fn parse(raw: &str) -> Option<Kind> {\n    match raw {\n        \"skill\" => Some(Kind::Skill),\n        \"agent\" => Some(Kind::Agent),\n        \"command\" => Some(Kind::Command),\n        \"rule\" => Some(Kind::Rule),\n        \"hook\" => Some(Kind::Hook),\n        _ => None,\n    }\n}\n";

#[test]
fn a_list_in_code_is_found_at_its_own_line() {
    let text = format!("use inventory::Kind;\n\n{A_COPY}{A_LABEL_MATCH}");

    let found = places(&text).expect("no test block");

    assert_eq!(
        found,
        vec![
            Place {
                line: 4,
                form: Form::VariantPaths
            },
            Place {
                line: 7,
                form: Form::VariantPaths
            },
            Place {
                line: 7,
                form: Form::Labels
            },
        ]
    );
}

#[test]
fn a_list_in_a_comment_a_string_or_a_test_block_does_not_count() {
    let in_a_comment = "// [Kind::Skill, Kind::Agent, Kind::Command, Kind::Rule, Kind::Hook]\nfn a() {}\n";
    let in_a_string = "const T: &str = \"[Kind::Skill, Kind::Agent, Kind::Command, Kind::Rule, Kind::Hook]\";\n";
    let in_a_test_block = format!("fn a() {{}}\n#[cfg(test)]\nmod tests {{\n{A_COPY}{A_LABEL_MATCH}}}\nfn b() {{}}\n");

    for text in [in_a_comment, in_a_string, in_a_test_block.as_str()] {
        let found = places(text).expect("every test block closes");
        assert!(found.is_empty(), "counted in\n{text}\n{found:?}");
    }
}

#[test]
fn a_test_block_that_never_closes_is_reported_instead_of_swallowing_the_file() {
    let text = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() {\n";

    let refused = places(text).expect_err("the block never closes");

    assert_eq!(refused, 2, "the line the file goes blind from");
}

#[test]
fn stripping_keeps_the_line_numbers_and_the_two_views_aligned() {
    let text = "one\n/* two\n   three */\nlet s = \"four\nfive\";\n";

    let views = strip(text);

    assert_eq!(views.code.len(), views.with_strings.len());
    assert_eq!(views.code.iter().filter(|c| **c == '\n').count(), 5);
    let code: String = views.code.iter().collect();
    assert_eq!(code.lines().nth(3), Some("let s =      "));
}
