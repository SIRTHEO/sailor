//! Comments do not crowd out the code: three numbers that can only go down.
//!
//! Not taste. The semantic index embeds comments literally — see the rule in
//! `AGENTS.md` — so a long block takes the code's place in a search result.
//!
//! **THIS FILE IS SCAFFOLDING AND SHOULD DISAPPEAR.** Every seed below is a
//! debt, not a target. When one reaches zero its constant goes and the test
//! asks for zero outright; the day all three do, so does this file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The per-block cap. Above it, a comment is chronicle: it belongs in the fault
/// ledger or the commit, not here.
///
/// **A COMMENT IN SEVERAL PARAGRAPHS IS ONE BLOCK**, because the line between
/// them is `///` and stays a comment. The cap bites harder than it reads, which
/// is why 636 blocks once carried two thirds of the volume.
const MAX_BLOCK: usize = 6;

/// How many blocks run over today. **It can only go down**: lowering it is the
/// repair, raising it has to be argued and shows in the diff.
const LONG_BLOCKS_TODAY: usize = 513;

/// How many comments cite a date. Same rule: downwards only.
const DATED_COMMENTS_TODAY: usize = 195;

/// How many comment lines are still not in English.
///
/// **THE ONLY HONEST RAISE** is a merge bringing in non-English comments
/// written elsewhere: there you re-measure, raise to the measured number, and
/// say so in the commit. Raising it because it went red is disarming it.
const COMMENT_LINES_NOT_IN_ENGLISH: usize = 7_520;

/// Comment lines per thousand code lines, per crate, as measured today.
/// Downwards only; a crate under 100 is where the sweep stops.
const COMMENT_PERMILLE_TODAY: &[(&str, usize)] = &[
    ("actions", 350),
    ("catalogue", 268),
    ("desktop", 268),
    ("faults", 205),
    ("flow", 221),
    ("inventory", 278),
    ("ledger", 170),
    ("models", 276),
    ("profiles", 177),
    ("registry", 407),
    ("relay", 155),
    ("release", 520),
    ("sailor", 256),
    ("sessions", 252),
    ("supervisor", 316),
    ("terminal", 287),
    ("toolbox", 297),
    ("trigger", 248),
    ("ui", 192),
    ("workspace", 181),
];

/// How far a seed may drift above what the tree actually holds. **Zero.**
///
/// **A SEED IS A NUMBER IN A FILE, AND A FILE MERGES.** A merge taking the older
/// side raises the ceiling with no conflict and no signal. It was 20, an
/// absolute number over three counters of different scale, and 20 does not tell
/// whoever re-measured from whoever passed by two.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

/// Words no English sentence uses, which a sentence in this tree's other
/// language cannot do without.
///
/// **THIS LIST IS THE DETECTION, NOT THE RULE.** The rule is that comments are
/// in English; the list is how a machine guesses they are not, and it would be
/// replaced wholesale for a codebase carrying a different language.
const WORDS_NO_ENGLISH_SENTENCE_USES: &[&str] = &[
    "che", "non", "della", "delle", "degli", "nella", "nelle", "questo", "questa", "quello",
    "quella", "perché", "perche", "cioè", "cioe", "invece", "quindi", "anche", "essere", "senza",
    "più", "piu", "già", "gia", "sono", "dove", "quando", "sulla", "dalla", "dello", "il", "lo",
    "le", "gli", "un", "una", "nel", "dei", "alla", "allo", "alle", "sul", "sui", "dal", "dalle",
    "questi", "queste", "quali", "quale", "ogni", "solo", "ancora", "adesso", "prima", "dopo",
];

/// **THE COUNT ERRS DOWNWARDS, ON PURPOSE.** Words that exist in both languages
/// are left out — a line using only those is not counted, so this number is a
/// floor. Hidden debt stays hidden; an English line is never accused, which
/// would make the debt unpayable even by translating.
fn is_not_english(line: &str) -> bool {
    let lowered = line.to_lowercase();
    lowered
        .split(|c: char| !c.is_alphabetic() && c != '\'')
        .any(|word| WORDS_NO_ENGLISH_SENTENCE_USES.contains(&word))
}

/// **THE WINDOW TOO, OR THE NUMBER LIES.** Counting `crates` alone measured
/// 11,476 non-English lines while `desktop/` — twelve thousand lines of
/// TypeScript and CSS — was watched by nobody. An invisible debt falls to zero
/// on its own without anyone paying it.
fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root");
    let mut found = Vec::new();
    for place in [
        "crates",
        "desktop/src",
        "desktop/src-tauri/src",
        "desktop/scripts",
    ] {
        walk(&root.join(place), &mut found);
    }
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | ".git") {
                walk(&path, found);
            }
        } else if name != "comments_do_not_crowd_out_the_code.rs"
            && [".rs", ".ts", ".tsx", ".css", ".mjs"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        {
            found.push(path);
        }
    }
}

/// **THREE SHAPES, BECAUSE STYLESHEETS HAVE NO `//` AND JSX HAS NEITHER.** The
/// `in_block` travels from line to line: without it a ninety-line CSS banner
/// would count as one line and the rest would pass for code.
///
/// **AND `{/* … */}` OPENS WITH A BRACE**, so it is neither: 65 blocks in 13
/// files of `desktop/src` — 256 lines, 178 of them not in English — passed for
/// code, which is exactly the window's long prose this count exists to watch.
/// Counting them lifts the three seeds by 9, 2 and 178 with nobody having
/// written a line: **the counter started seeing**, the debt did not grow.
fn is_comment(line: &str, in_block: &mut bool) -> bool {
    let trimmed = line.trim_start();
    if *in_block {
        if trimmed.contains("*/") {
            *in_block = false;
        }
        return true;
    }
    if trimmed.starts_with("//") {
        return true;
    }
    for opener in ["/*", "{/*"] {
        if let Some(rest) = trimmed.strip_prefix(opener) {
            if !rest.contains("*/") {
                *in_block = true;
            }
            return true;
        }
    }
    false
}

/// `31/08/2026` and the like. A date in a comment is chronicle by definition.
fn cites_a_date(line: &str) -> bool {
    let bytes: Vec<char> = line.chars().collect();
    bytes.windows(10).any(|w| {
        w[0].is_ascii_digit()
            && w[1].is_ascii_digit()
            && w[2] == '/'
            && w[3].is_ascii_digit()
            && w[4].is_ascii_digit()
            && w[5] == '/'
            && w[6..10].iter().all(char::is_ascii_digit)
    })
}

/// **THE WAY ROUND THE CAP: SPLIT INSTEAD OF SHORTENING.** Two doc groups with
/// a blank line between them are one doc comment — both attach to the same item
/// — so twelve lines become two blocks of six and the cap is satisfied with
/// nothing removed. Found by general-01 in `desktop/src`, where the same move
/// went from 4 to 41 and orphans the first block outright, since in JSDoc only
/// the last comment before a declaration documents it.
///
/// **THE THREE FAMILIES, AND WHY NOT ONE.** `///` documents the item below,
/// `//!` the module around it, `/** */` both in TypeScript. Each is closed
/// against itself and not against the others: a `///` followed by a `//!` is
/// two comments about two different things, and joining them would count a
/// block nobody wrote. Measured on this tree: closing `///` alone left the
/// count at 775 for a split in `desktop/src` and for one in a `//!` header —
/// the way round stayed open in the very place it was found.
///
/// **AND `*/` ALONE ON ITS LINE, NOT `*/` ANYWHERE.** A one-line banner —
/// `/* ═══ 3. THE INVITATIONS ═══ */` — also ends with `*/`, and joining it to
/// the doc comment underneath invented two long blocks in
/// `unhappystates.test.tsx` that nobody had written.
fn splits_one_doc_comment(lines: &[&str], at: usize) -> bool {
    if !lines[at].trim().is_empty() || at == 0 {
        return false;
    }
    let resumes = next_line_with_something_on_it(lines, at);
    if resumes >= lines.len() {
        return false;
    }
    let ends = lines[at - 1].trim();
    let opens = lines[resumes].trim_start();
    ["///", "//!"]
        .iter()
        .any(|mark| ends.starts_with(mark) && opens.starts_with(mark))
        || (ends == "*/" && opens.starts_with("/**"))
}

fn next_line_with_something_on_it(lines: &[&str], from: usize) -> usize {
    let mut at = from;
    while at < lines.len() && lines[at].trim().is_empty() {
        at += 1;
    }
    at
}

struct Counts {
    long_blocks: usize,
    dated: usize,
    not_english: usize,
    worst: (usize, String),
    /// Where each one is, so a red gate is a job and not a search. Whoever had
    /// to find them wrote a second counter beside this one to do it, and a
    /// second counter is a second answer: this one points instead.
    long_at: Vec<String>,
    not_english_at: Vec<String>,
}

fn count() -> Counts {
    let mut counts = Counts {
        long_blocks: 0,
        dated: 0,
        not_english: 0,
        worst: (0, String::new()),
        long_at: Vec::new(),
        not_english_at: Vec::new(),
    };
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        let mut run = 0usize;
        let mut in_block = false;
        let mut index = 0usize;
        while index < lines.len() {
            let line = lines[index];
            if is_comment(line, &mut in_block) {
                run += 1;
                if cites_a_date(line) {
                    counts.dated += 1;
                }
                if is_not_english(line) {
                    counts.not_english += 1;
                    counts
                        .not_english_at
                        .push(format!("{}:{}", path.display(), index + 1));
                }
                index += 1;
                continue;
            }
            if run > 0 && splits_one_doc_comment(&lines, index) {
                index = next_line_with_something_on_it(&lines, index);
                continue;
            }
            if run > MAX_BLOCK {
                counts.long_blocks += 1;
                counts.long_at.push(format!(
                    "{}:{} ({run} lines)",
                    path.display(),
                    index - run + 1
                ));
                if run > counts.worst.0 {
                    counts.worst = (run, path.display().to_string());
                }
            }
            run = 0;
            index += 1;
        }
        // A file that ends inside a comment: the loop above never meets the code
        // line that closes the run, so the last one is counted here — and it was
        // counted without ever being placed or compared to the worst.
        if run > MAX_BLOCK {
            counts.long_blocks += 1;
            counts.long_at.push(format!(
                "{}:{} ({run} lines, at the end of the file)",
                path.display(),
                lines.len() - run + 1
            ));
            if run > counts.worst.0 {
                counts.worst = (run, path.display().to_string());
            }
        }
    }
    counts
}

/// **ALL THREE NUMBERS, WHATEVER FAILED.** Each test names its own, so whoever
/// repairs re-measures that one alone and leaves the other two above the tree —
/// by one or two, under the band, invisible to everything. Asked for by
/// general-ad, who pruned and re-measured only the line the message named.
/// The twelve files holding most of what a red gate is asking about.
///
/// A count alone sends whoever repairs to look for it, and looking for it is
/// how a second counter gets written beside this one — which then answers
/// something slightly different and settles nothing.
fn heaviest(places: &[String]) -> String {
    let mut per_file: BTreeMap<&str, usize> = BTreeMap::new();
    for place in places {
        let file = place.split(':').next().unwrap_or(place);
        *per_file.entry(file).or_default() += 1;
    }
    let mut ranked: Vec<(&str, usize)> = per_file.into_iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
    let mut said = String::from("\nWhere they are, heaviest first:");
    for (file, howmany) in ranked.iter().take(12) {
        said.push_str(&format!("\n  {howmany:>5}  {file}"));
    }
    if ranked.len() > 12 {
        said.push_str(&format!("\n  … and {} more files", ranked.len() - 12));
    }
    said
}

fn all_three(counts: &Counts) -> String {
    format!(
        "\nMeasured right now, all three: {} blocks over {MAX_BLOCK} lines, \
         {} comments citing a date, {} comment lines not in English. \
         When you re-measure one, rewrite them all.",
        counts.long_blocks, counts.dated, counts.not_english
    )
}

/// Comment lines and code lines per crate: the crate is the first directory
/// under `crates/`, and the shell is `desktop`.
fn permille_per_crate() -> BTreeMap<String, usize> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root");
    let mut comments: BTreeMap<String, usize> = BTreeMap::new();
    let mut code: BTreeMap<String, usize> = BTreeMap::new();
    for path in sources() {
        let relative = path.strip_prefix(&root).unwrap_or(&path);
        let mut parts = relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned());
        let crate_name = match parts.next().as_deref() {
            Some("crates") => parts.next().unwrap_or_default(),
            Some("desktop") => "desktop".to_owned(),
            _ => continue,
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut in_block = false;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if is_comment(line, &mut in_block) {
                *comments.entry(crate_name.clone()).or_default() += 1;
            } else {
                *code.entry(crate_name.clone()).or_default() += 1;
            }
        }
    }
    code.into_iter()
        .map(|(name, lines)| {
            let said = comments.get(&name).copied().unwrap_or(0);
            (name, said * 1000 / lines.max(1))
        })
        .collect()
}

/// Every crate's ratio is seeded exactly and may only fall: a crate whose
/// comments grew faster than its code is red, and a seed left above the
/// tree is a seed nobody re-measured.
#[test]
fn no_crate_lets_its_comments_outtalk_its_code_more_than_today() {
    let measured = permille_per_crate();
    let table: Vec<String> = measured.iter().map(|(name, permille)| format!("    (\"{name}\", {permille}),")).collect();
    let seeded: BTreeMap<&str, usize> = COMMENT_PERMILLE_TODAY.iter().copied().collect();
    for (name, permille) in &measured {
        let seed = seeded.get(name.as_str()).copied();
        assert!(
            seed.is_some_and(|seed| *permille <= seed),
            "crate «{name}» carries {permille}‰ comment lines against a seed of {seed:?}. Cut comments, or the table is stale; measured now:\n{}",
            table.join("\n")
        );
        assert!(
            seed.is_some_and(|seed| seed <= permille + HOW_STALE_A_SEED_MAY_BE),
            "crate «{name}» is seeded at {seed:?}‰ and holds {permille}‰: lower the seed. Measured now:\n{}",
            table.join("\n")
        );
    }
    assert_eq!(seeded.len(), measured.len(), "the table names crates the tree lacks, or lacks some; measured now:\n{}", table.join("\n"));
}

#[test]
fn no_new_comment_block_runs_past_the_cap() {
    let counts = count();
    assert!(
        counts.long_blocks <= LONG_BLOCKS_TODAY,
        "blocks over {MAX_BLOCK} lines: {} (the declared cap is {LONG_BLOCKS_TODAY}). \
         The longest runs {} lines in {}. Shorten it, or move the chronicle into \
         the commit message{}",
        counts.long_blocks,
        counts.worst.0,
        counts.worst.1,
        format!("{}{}", all_three(&counts), heaviest(&counts.long_at))
    );
}

#[test]
fn no_new_comment_tells_a_date() {
    let counts = count();
    assert!(
        counts.dated <= DATED_COMMENTS_TODAY,
        "comments citing a date: {} (declared {DATED_COMMENTS_TODAY}). \
         Git keeps the date, with the real author{}",
        counts.dated,
        all_three(&counts)
    );
}

/// The debt goes down and never up. Whoever raises it is writing new comments
/// in a language this repository does not publish in.
#[test]
fn the_comments_not_in_english_only_shrink() {
    let counts = count();
    assert!(
        counts.not_english <= COMMENT_LINES_NOT_IN_ENGLISH,
        "comment lines not in English: {} (declared {COMMENT_LINES_NOT_IN_ENGLISH}). \
         If you are writing a new comment, write it in English; if you are \
         translating, lower the number{}",
        counts.not_english,
        format!("{}{}", all_three(&counts), heaviest(&counts.not_english_at))
    );
}

/// The other side of every ratchet: a ceiling that stops describing the tree.
///
/// The three tests above only ask that the count not exceed the seed, so a seed
/// that drifted upwards is invisible to them — and the seeds are constants in a
/// file, which a merge can raise without a conflict. Found by general-01, who
/// watched a clean merge silently undo a repair because both sides were green.
#[test]
fn a_seed_that_no_longer_describes_the_tree_is_a_seed_nobody_re_measured() {
    let counts = count();
    for (what, declared, measured) in [
        ("long blocks", LONG_BLOCKS_TODAY, counts.long_blocks),
        ("dated comments", DATED_COMMENTS_TODAY, counts.dated),
        (
            "comment lines not in English",
            COMMENT_LINES_NOT_IN_ENGLISH,
            counts.not_english,
        ),
    ] {
        assert!(
            declared <= measured + HOW_STALE_A_SEED_MAY_BE,
            "the seed «{what}» says {declared}, the tree holds {measured}: \
             {} apart. Either a merge raised the ceiling, or somebody pruned \
             without re-measuring — either way the number to write here is \
             {measured}{}",
            declared - measured,
            all_three(&counts)
        );
    }
}

/// **WHOEVER MEASURES GETS MEASURED.** If `is_comment` or `cites_a_date`
/// stopped seeing, the numbers would collapse to zero and the tests above would
/// stay green for ever.
#[test]
fn the_check_can_still_see_what_it_counts() {
    let mut block = false;
    assert!(is_comment("    // like this", &mut block));
    assert!(is_comment("/// and this", &mut block));
    assert!(!is_comment(
        "let x = 1; // not this: the line is code",
        &mut block
    ));
    // And a stylesheet banner, which without the state would count one line.
    assert!(is_comment("/* the banner opens", &mut block));
    assert!(block, "and the next line is still inside the comment");
    assert!(is_comment("   still inside", &mut block));
    assert!(is_comment("   and it ends here */", &mut block));
    assert!(!block, "the block closes");
    assert!(!is_comment(".a-class { color: red; }", &mut block));
    // And the JSX comment, which opens with a brace: without this line it
    // vanishes quietly, because all three thresholds **permit** a fall.
    assert!(is_comment("        {/* the window's prose", &mut block));
    assert!(block, "`{{/*` opens a block that carries on too");
    assert!(is_comment("            and it goes on here", &mut block));
    assert!(is_comment("            up to here */}", &mut block));
    assert!(!block, "and `*/}}` closes it");
    assert!(cites_a_date("// measured on 31/08/2026"));
    assert!(!cites_a_date("// no date here"));
    let counts = count();
    // Today's three numbers, for whoever prunes: `cargo test -p sailor --test
    // comments_do_not_crowd_out_the_code -- --nocapture`. Without this the only
    // way to learn them was to zero a seed and read the failure.
    println!(
        "today: {} blocks over {MAX_BLOCK} lines, {} comments citing a date, \
         {} comment lines not in English",
        counts.long_blocks, counts.dated, counts.not_english
    );
    // Splitting a block in two does not shorten it: twelve lines stay twelve.
    let split = [
        "/// one",
        "/// two",
        "/// three",
        "/// four",
        "",
        "/// five",
        "/// six",
        "/// seven",
        "fn something() {}",
    ];
    assert!(
        splits_one_doc_comment(&split, 4),
        "a blank line between two /// groups does not split a rustdoc, and must not split the count"
    );
    let real_end = ["/// one", "", "fn something() {}"];
    assert!(
        !splits_one_doc_comment(&real_end, 1),
        "a blank line followed by code really does close the block"
    );
    // The other two families: the module banner and the window's JSDoc. Closing
    // the way round for `///` only left it open where it had been found.
    let module = ["//! one", "//! two", "", "//! three", "use std::fmt;"];
    assert!(
        splits_one_doc_comment(&module, 2),
        "two //! groups are one banner: the module they document is the same"
    );
    let jsdoc = [
        "/**",
        " * one",
        " */",
        "",
        "/**",
        " * two",
        " */",
        "export const x = 1;",
    ];
    assert!(
        splits_one_doc_comment(&jsdoc, 3),
        "in JSDoc only the last block documents: splitting orphans, it does not shorten"
    );
    // And the negatives, or the rule counts blocks nobody wrote.
    let mixed = ["/// one", "", "//! two", "fn something() {}"];
    assert!(
        !splits_one_doc_comment(&mixed, 1),
        "/// documents what follows, //! the module around: they are two comments"
    );
    let banner = [
        "/* ═══ 3. THE SECTIONS ═══ */",
        "",
        "/**",
        " * one",
        " */",
        "const y = 2;",
    ];
    assert!(
        !splits_one_doc_comment(&banner, 1),
        "a one-line banner ends with */ but is not a split block"
    );
    assert!(
        counts.long_blocks > 0,
        "zero long blocks: the counter is not looking"
    );
    assert!(counts.dated > 0, "zero dates: the counter is not looking");
    assert!(is_not_english("// perché questo non basta"));
    assert!(
        !is_not_english("// the cap must truncate, not merely be measured"),
        "an English line must not count as non-English, or the debt could never \
         fall even by translating"
    );
    assert!(
        counts.not_english > 0,
        "zero non-English: the counter is not looking"
    );
}
