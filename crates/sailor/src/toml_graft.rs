//! Grafting into a settings file written in TOML, **by appending**.
//!
//! **NOTHING ABOVE THE END OF THE FILE IS TOUCHED.** A round trip through a
//! parser hands back a file its author no longer recognises: comments moved,
//! sections reordered, quoting normalised. Appending hands back the same bytes
//! with our own lines after them, and that is the whole promise.

/// **AND A NEW TABLE GOES AT THE BOTTOM, NEVER IN THE MIDDLE.** In TOML every
/// key after a table header belongs to that table, so a section inserted half
/// way swallows the keys that follow it. Measured on a real machine: a global
/// key stopped being global because a section was added above it.
#[derive(Debug)]
pub struct Graft {
    /// The whole file as it must be written.
    pub text: String,
    /// The events that entered. Empty means the file already had them all.
    pub added: Vec<String>,
}

/// The file with our own lines after it, or a reason it cannot be done.
///
/// `under` is the key the hooks sit under, as the descriptor declares it.
/// `commands` pairs an event name with the whole command line to run at it.
/// `marks` are the words that all have to be in a line for it to be ours.
pub fn appended(
    existing: &str,
    under: &[String],
    commands: &[(&str, String)],
    marks: &[&str],
) -> Result<Graft, String> {
    if under.is_empty() {
        return Err(catalogue::say("cli.session.toml_no_key", &[]));
    }
    let tables = tables(existing)?;
    let mut added = Vec::new();
    let mut body = String::new();
    for (event, command) in commands {
        let mut want = under.to_vec();
        want.push((*event).to_owned());
        // **REFUSED WHOLE, NEVER HALF.** A shape we did not expect anywhere in
        // the file means we are reading it wrong, and half a graft into a file
        // read wrong is worse than none.
        if let Some(reason) = blocked(&tables, under, &want) {
            return Err(reason);
        }
        if ours(&tables, &want, marks) {
            continue;
        }
        let path = dots(&want);
        body.push_str(&format!(
            "\n[[{path}]]\n[[{path}.hooks]]\ntype = \"command\"\ncommand = {}\n",
            quoted(command)
        ));
        added.push((*event).to_owned());
    }
    if added.is_empty() {
        return Ok(Graft {
            text: existing.to_owned(),
            added,
        });
    }
    let mut text = existing.to_owned();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("\n# Grafted by Sailor: these lines report a session's moments.\n");
    text.push_str(&body);
    Ok(Graft { text, added })
}

/// One table header and everything written under it, up to the next header.
///
/// The whole file is read as a list of these. It is far less than a TOML
/// parser and exactly what appending needs to know: which tables are already
/// open, which keys they hold, and whether one of the lines is ours.
struct Table {
    path: Vec<String>,
    array: bool,
    /// The key paths assigned directly under this header.
    keys: Vec<Vec<String>>,
    /// The lines themselves, for recognising a line we wrote before.
    body: String,
}

/// The tables of a file, the first one being everything before any header.
///
/// **WHEN IT CANNOT TELL, IT REFUSES.** A header it cannot read, or a file that
/// ends inside a value, comes back as an error and the file is left alone: the
/// alternative is appending after a line we misread.
fn tables(text: &str) -> Result<Vec<Table>, String> {
    let mut out = vec![Table {
        path: Vec::new(),
        array: false,
        keys: Vec::new(),
        body: String::new(),
    }];
    let mut multiline: Option<char> = None;
    let mut depth = 0usize;
    for (number, line) in text.lines().enumerate() {
        if multiline.is_none() && depth == 0 {
            let trimmed = line.trim_start();
            if trimmed.starts_with('[') {
                let (path, array) = header(trimmed).ok_or_else(|| {
                    catalogue::say(
                        "cli.session.toml_header_unreadable",
                        &[("line", &(number + 1).to_string()), ("text", line.trim())],
                    )
                })?;
                out.push(Table {
                    path,
                    array,
                    keys: Vec::new(),
                    body: format!("{line}\n"),
                });
                continue;
            }
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                if let Some(key) = assigned(trimmed) {
                    out.last_mut()
                        .expect("the preamble is there")
                        .keys
                        .push(key);
                }
            }
        }
        let last = out.last_mut().expect("the preamble is there");
        last.body.push_str(line);
        last.body.push('\n');
        walk_value(line, &mut depth, &mut multiline);
    }
    if multiline.is_some() || depth > 0 {
        return Err(catalogue::say("cli.session.toml_ends_inside_a_value", &[]));
    }
    Ok(out)
}

/// A reason appending `[[want]]` at the end would land somewhere else, or not
/// parse at all. `None` means the bottom of the file is free.
fn blocked(tables: &[Table], under: &[String], want: &[String]) -> Option<String> {
    let root = &under[0];
    for key in &tables[0].keys {
        if key.first() == Some(root) {
            return Some(catalogue::say(
                "cli.session.toml_root_assigns_the_key",
                &[("key", &dots(key))],
            ));
        }
    }
    let opened_as_array = tables
        .iter()
        .any(|table| table.path == *want && table.array);
    for table in tables {
        if table.path == *under && table.array {
            return Some(catalogue::say(
                "cli.session.toml_is_an_array_of_tables",
                &[("key", &dots(under))],
            ));
        }
        if table.path == *under {
            for key in &table.keys {
                if key.first() == want.last() {
                    return Some(catalogue::say(
                        "cli.session.toml_key_of_the_same_name",
                        &[("table", &dots(under)), ("key", &dots(key))],
                    ));
                }
            }
        }
        if table.path == *want && !table.array {
            return Some(catalogue::say(
                "cli.session.toml_already_a_plain_table",
                &[("key", &dots(want))],
            ));
        }
        if !opened_as_array && table.path.len() > want.len() && table.path.starts_with(want) {
            return Some(catalogue::say(
                "cli.session.toml_used_as_a_table",
                &[("table", &dots(&table.path)), ("key", &dots(want))],
            ));
        }
    }
    None
}

/// Whether this event already carries a line of ours.
///
/// **RECOGNISED BY WHAT IT RUNS, NOT BY WHERE IT SITS.** A name written beside
/// a hook can be changed without changing what it does; the command cannot.
fn ours(tables: &[Table], want: &[String], marks: &[&str]) -> bool {
    tables
        .iter()
        .filter(|table| table.path.starts_with(want))
        .any(|table| marks.iter().all(|mark| table.body.contains(mark)))
}

/// A header line reduced to the path it opens, and whether it is an array.
fn header(trimmed: &str) -> Option<(Vec<String>, bool)> {
    let array = trimmed.starts_with("[[");
    let close = if array { "]]" } else { "]" };
    let rest = &trimmed[if array { 2 } else { 1 }..];
    let end = find_outside_quotes(rest, close)?;
    let tail = rest[end + close.len()..].trim();
    if !tail.is_empty() && !tail.starts_with('#') {
        return None;
    }
    Some((dotted(&rest[..end])?, array))
}

/// The key path a line assigns to, when it assigns to one.
fn assigned(line: &str) -> Option<Vec<String>> {
    let at = find_outside_quotes(line, "=")?;
    dotted(&line[..at])
}

/// A dotted key path, split outside quotes and unquoted.
fn dotted(text: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let mut keys = Vec::new();
    let mut at = 0usize;
    loop {
        while at < chars.len() && chars[at].is_whitespace() {
            at += 1;
        }
        if at >= chars.len() {
            return None;
        }
        let key = if chars[at] == '"' || chars[at] == '\'' {
            let (value, after) = quoted_key(&chars, at)?;
            at = after;
            value
        } else {
            let start = at;
            while at < chars.len()
                && (chars[at].is_ascii_alphanumeric() || "_-".contains(chars[at]))
            {
                at += 1;
            }
            if at == start {
                return None;
            }
            chars[start..at].iter().collect()
        };
        keys.push(key);
        while at < chars.len() && chars[at].is_whitespace() {
            at += 1;
        }
        if at >= chars.len() {
            return Some(keys);
        }
        if chars[at] != '.' {
            return None;
        }
        at += 1;
    }
}

/// A quoted key and the index just after it. The escapes of a basic string are
/// kept as the character they escape: comparing a name never needs more.
fn quoted_key(chars: &[char], from: usize) -> Option<(String, usize)> {
    let quote = chars[from];
    let mut value = String::new();
    let mut at = from + 1;
    loop {
        let current = *chars.get(at)?;
        if quote == '"' && current == '\\' {
            value.push(*chars.get(at + 1)?);
            at += 2;
            continue;
        }
        at += 1;
        if current == quote {
            return Some((value, at));
        }
        value.push(current);
    }
}

/// Where a needle sits outside every string, before any comment starts.
fn find_outside_quotes(text: &str, needle: &str) -> Option<usize> {
    let mut at = 0usize;
    while at < text.len() {
        let rest = &text[at..];
        let current = rest.chars().next()?;
        if current == '#' {
            return None;
        }
        if rest.starts_with(needle) {
            return Some(at);
        }
        if current == '"' || current == '\'' {
            let chars: Vec<char> = rest.chars().collect();
            let (_, after) = quoted_key(&chars, 0)?;
            at += chars[..after].iter().map(|c| c.len_utf8()).sum::<usize>();
            continue;
        }
        at += current.len_utf8();
    }
    None
}

/// The strings and brackets a line leaves open, so the next line is read in the
/// state this one left. Without it a `["a"]` inside a multi-line array reads as
/// a table header, and everything under it is filed in the wrong place.
fn walk_value(line: &str, depth: &mut usize, multiline: &mut Option<char>) {
    let chars: Vec<char> = line.chars().collect();
    let mut at = 0usize;
    if let Some(quote) = *multiline {
        match triple_end(&chars, 0, quote) {
            Some(after) => {
                *multiline = None;
                at = after;
            }
            None => return,
        }
    }
    while at < chars.len() {
        let current = chars[at];
        match current {
            '#' => return,
            '[' | '{' => {
                *depth += 1;
                at += 1;
            }
            ']' | '}' => {
                *depth = depth.saturating_sub(1);
                at += 1;
            }
            '"' | '\'' => {
                let triple =
                    chars.get(at + 1) == Some(&current) && chars.get(at + 2) == Some(&current);
                if triple {
                    match triple_end(&chars, at + 3, current) {
                        Some(after) => at = after,
                        None => {
                            *multiline = Some(current);
                            return;
                        }
                    }
                } else {
                    at = match quoted_key(&chars, at) {
                        Some((_, after)) => after,
                        None => chars.len(),
                    };
                }
            }
            _ => at += 1,
        }
    }
}

/// Where a multi-line string closes, as an index just after its three quotes.
fn triple_end(chars: &[char], from: usize, quote: char) -> Option<usize> {
    let mut at = from;
    while at + 2 < chars.len() + 1 {
        if chars.get(at) == Some(&quote)
            && chars.get(at + 1) == Some(&quote)
            && chars.get(at + 2) == Some(&quote)
        {
            return Some(at + 3);
        }
        at += 1;
    }
    None
}

/// A key path written back the way TOML wants it, quoted only where it must be.
fn dots(keys: &[String]) -> String {
    keys.iter()
        .map(|key| {
            if !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                key.clone()
            } else {
                quoted(key)
            }
        })
        .collect::<Vec<String>>()
        .join(".")
}

/// A basic string, with the two characters that would end it early escaped.
fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for current in value.chars() {
        match current {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(current),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marks of a line of ours, as the grafting hands them over.
    fn marks() -> Vec<&'static str> {
        vec![" session ", "sailor"]
    }

    fn under() -> Vec<String> {
        vec!["hooks".to_owned()]
    }

    fn two_events() -> Vec<(&'static str, String)> {
        vec![
            ("SessionStart", "/bin/sailor session open".to_owned()),
            ("Stop", "/bin/sailor session event".to_owned()),
        ]
    }

    /// A file written by hand: comments, a global key, and sections after the
    /// place a careless graft would cut in.
    fn written_by_hand() -> &'static str {
        "# the line at the top, which is a comment\n\
         notify = [\"somebody\", \"turn-ended\"]\n\
         \n\
         [hooks.state]\n\
         \n\
         [hooks.state.\"a/file.json:session_start:0:0\"]\n\
         trusted_hash = \"sha256:abc\"\n\
         \n\
         # the servers, which must keep their comment and their place\n\
         [servers.one]\n\
         command = \"/somewhere/one\"\n"
    }

    #[test]
    fn the_file_that_was_there_comes_back_byte_for_byte() {
        let before = written_by_hand();
        let graft = appended(before, &under(), &two_events(), &marks()).expect("the graft works");

        assert!(
            graft.text.starts_with(before),
            "the graft rewrote what was above it:\n{}",
            graft.text
        );
        assert_eq!(graft.added, vec!["SessionStart", "Stop"]);
    }

    /// **THE SECTIONS AFTER THE GRAFT POINT ARE THE WHOLE FAULT.** A table put
    /// half way swallows every key below it, so the check is not «the text is
    /// still there» but «the keys still belong to the same tables».
    #[test]
    fn a_section_written_after_ours_keeps_its_keys() {
        let graft = appended(written_by_hand(), &under(), &two_events(), &marks())
            .expect("the graft works");
        let read = tables(&graft.text).expect("it still reads");

        // The global key first: a table opened above it takes it into itself,
        // and it stops being global with nothing saying so.
        assert_eq!(
            read[0].keys,
            vec![vec!["notify".to_owned()]],
            "a key that was global is now inside a table: {}",
            graft.text
        );
        let servers = read
            .iter()
            .find(|table| table.path == ["servers".to_owned(), "one".to_owned()])
            .expect("the section is still there");
        assert_eq!(
            servers.keys,
            vec![vec!["command".to_owned()]],
            "the key of a section below the graft point moved: {}",
            graft.text
        );
        assert!(
            graft
                .text
                .contains("# the servers, which must keep their comment"),
            "the comment of whoever wrote by hand is gone: {}",
            graft.text
        );
    }

    #[test]
    fn grafting_twice_writes_once() {
        let once = appended(written_by_hand(), &under(), &two_events(), &marks())
            .expect("the first graft");
        let twice =
            appended(&once.text, &under(), &two_events(), &marks()).expect("the second graft");

        assert!(
            twice.added.is_empty(),
            "the second graft added {:?}",
            twice.added
        );
        assert_eq!(once.text, twice.text, "the second graft changed the file");
    }

    /// A file that already holds our lines, written by somebody else's hand
    /// rather than by us: recognised by the command, not by the shape.
    #[test]
    fn a_file_that_already_has_them_gets_nothing() {
        let already = "[[hooks.SessionStart]]\n\
                       [[hooks.SessionStart.hooks]]\n\
                       type = \"command\"\n\
                       command = \"/opt/sailor session open\"\n\
                       \n\
                       [[hooks.Stop]]\n\
                       [[hooks.Stop.hooks]]\n\
                       type = \"command\"\n\
                       command = \"/opt/sailor session event\"\n";
        let graft = appended(already, &under(), &two_events(), &marks()).expect("the graft works");

        assert!(graft.added.is_empty(), "added {:?}", graft.added);
        assert_eq!(graft.text, already);
    }

    /// Somebody else's hook at the same event stays, and ours goes in beside it.
    #[test]
    fn somebody_elses_hook_at_the_same_event_survives() {
        let theirs = "[[hooks.SessionStart]]\n\
                      [[hooks.SessionStart.hooks]]\n\
                      type = \"command\"\n\
                      command = \"/somewhere/theirs.sh\"\n";
        let graft = appended(theirs, &under(), &two_events(), &marks()).expect("the graft works");

        assert_eq!(graft.added, vec!["SessionStart", "Stop"]);
        assert!(graft.text.contains("theirs.sh"), "{}", graft.text);
    }

    /// A plain table where an array of tables would have to go: it would not
    /// parse, so nothing is written and the reason is the answer.
    #[test]
    fn a_shape_that_would_not_parse_is_refused_out_loud() {
        let awkward = "[hooks.SessionStart]\nwhatever = 1\n";
        let refused = appended(awkward, &under(), &two_events(), &marks())
            .expect_err("a shape we cannot append to stops the graft");

        assert!(refused.contains("hooks.SessionStart"), "{refused}");
        assert!(refused.contains("nothing was written"), "{refused}");
    }

    #[test]
    fn a_key_of_the_same_name_stops_the_graft() {
        let awkward = "[hooks]\nSessionStart = []\n";
        let refused =
            appended(awkward, &under(), &two_events(), &marks()).expect_err("it must refuse");

        assert!(refused.contains("SessionStart"), "{refused}");
    }

    #[test]
    fn a_root_that_already_assigns_the_key_stops_the_graft() {
        let awkward = "hooks.state.one = 1\n";
        let refused =
            appended(awkward, &under(), &two_events(), &marks()).expect_err("it must refuse");

        assert!(refused.contains("hooks.state.one"), "{refused}");
    }

    /// **A HEADER INSIDE A VALUE IS NOT A HEADER.** Without the state that
    /// travels from line to line, this file grows a table called `a` and the
    /// keys under it are filed where nobody put them.
    #[test]
    fn a_bracket_inside_a_value_is_not_a_table() {
        let tricky = "roots = [\n  [\"a\"],\n  [\"b\"],\n]\nafter = 1\n";
        let read = tables(tricky).expect("it reads");

        assert_eq!(read.len(), 1, "the file has no table headers at all");
        assert_eq!(
            read[0].keys,
            vec![vec!["roots".to_owned()], vec!["after".to_owned()]]
        );
    }

    /// The same for a multi-line string, which can hold anything at all.
    #[test]
    fn a_table_header_inside_a_multi_line_string_is_not_a_table() {
        let tricky = "note = \"\"\"\n[hooks.SessionStart]\n\"\"\"\nafter = 1\n";
        let read = tables(tricky).expect("it reads");

        assert_eq!(read.len(), 1, "the header is inside a string: {tricky}");
    }

    /// **WHOEVER MEASURES GETS MEASURED.** If the reader stopped seeing table
    /// headers, every refusal above would go quiet and every graft would land
    /// at the bottom of a file nobody checked.
    #[test]
    fn the_reader_still_sees_what_it_reads() {
        let read = tables(written_by_hand()).expect("it reads");
        let paths: Vec<Vec<String>> = read.iter().map(|table| table.path.clone()).collect();

        assert_eq!(paths[0], Vec::<String>::new(), "the preamble comes first");
        assert!(
            paths.contains(&vec!["hooks".to_owned(), "state".to_owned()]),
            "{paths:?}"
        );
        assert!(
            paths.contains(&vec!["servers".to_owned(), "one".to_owned()]),
            "{paths:?}"
        );
        assert_eq!(
            read[2].path,
            vec![
                "hooks".to_owned(),
                "state".to_owned(),
                "a/file.json:session_start:0:0".to_owned()
            ],
            "a quoted key holding dots and slashes is one key"
        );
    }

    /// A command line holding the two characters that would end a TOML string
    /// early. A path can hold both, and a graft that broke here would leave a
    /// file that no longer parses at all.
    #[test]
    fn a_command_with_a_quote_or_a_backslash_stays_readable() {
        let awkward = vec![(
            "SessionStart",
            "/a path/with \"quotes\" and \\ session open sailor".to_owned(),
        )];
        let graft = appended("", &under(), &awkward, &marks()).expect("the graft works");

        assert!(graft.text.contains("\\\"quotes\\\""), "{}", graft.text);
        assert!(graft.text.contains("and \\\\ session"), "{}", graft.text);
        let read = tables(&graft.text).expect("it still reads");
        assert!(
            read.iter()
                .any(|table| table.path == ["hooks".to_owned(), "SessionStart".to_owned()]),
            "the table did not survive its own command: {}",
            graft.text
        );
    }

    #[test]
    fn without_a_key_there_is_nowhere_to_put_them() {
        let refused = appended("", &[], &two_events(), &marks()).expect_err("it must refuse");
        assert!(refused.contains("no key is declared"), "{refused}");
    }
}
