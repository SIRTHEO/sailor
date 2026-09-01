//! I commenti non soffocano il codice: due numeri che possono solo scendere.
//!
//! Non è gusto. L'indice semantico incorpora i commenti alla lettera — vedi la
//! regola in `AGENTS.md` — quindi un blocco lungo prende il posto del codice
//! nella risposta a una ricerca.

use std::path::{Path, PathBuf};

/// Il tetto per blocco. Sopra, è cronaca: va nel registro dei guasti o nel
/// commit, non qui.
///
/// **UN COMMENTO IN PIÙ CAPOVERSI È UN BLOCCO SOLO**, perché la riga che li
/// separa è `///` e resta un commento. Il tetto morde più forte di quanto
/// sembri leggendolo, ed è il motivo per cui 636 blocchi si portavano due terzi
/// del volume. Una riga davvero vuota invece lo spezza.
const MAX_BLOCK: usize = 6;

/// Quanti blocchi sforano oggi. **Può solo scendere**: abbassarlo è la
/// riparazione, alzarlo va discusso e si vede nel diff.
const LONG_BLOCKS_TODAY: usize = 776;

/// Quanti commenti citano una data. Stessa regola: solo verso il basso.
const DATED_COMMENTS_TODAY: usize = 320;

/// Quante righe di commento sono ancora in italiano.
///
/// La conversione all'inglese e' un cantiere: questo numero e' il debito, e
/// puo' solo scendere. Chi lo alza sta scrivendo italiano nuovo.
///
/// **L'UNICO RIALZO ONESTO** e' una fusione che porta dentro italiano gia'
/// scritto altrove: li' si rimisura, si alza col numero misurato, e lo si dice
/// nel commit. Alzarlo perche' e' diventato rosso e' disarmarlo.
const ITALIAN_COMMENT_LINES_TODAY: usize = 13_165;

/// How far a seed may drift above what the tree actually holds.
///
/// **A SEED IS A NUMBER IN A FILE, AND A FILE MERGES.** A merge that takes the
/// older side raises the ceiling with no conflict and no signal, and from then
/// on reverted translations fit underneath it.
const HOW_STALE_A_SEED_MAY_BE: usize = 20;

/// Parole senza le quali una frase italiana non sta in piedi, e che **non sono
/// parole inglesi valide**.
///
/// Mancano apposta: `come`, `con`, `per`, `dice`, `fare`, `la`, `del` — esistono
/// in tutte e due le lingue e accuserebbero l'inglese. E `ai`, che minuscolato
/// è la stessa cosa di «AI». Il prezzo dichiarato: una riga italiana che usa
/// solo quelle non viene contata, quindi **questo numero è un minimo**, non il
/// totale. Sbaglia verso il basso, che qui vuol dire che un debito nascosto
/// resta nascosto — mai che una riga inglese venga accusata.
const ITALIAN_FUNCTION_WORDS: &[&str] = &[
    "che", "non", "della", "delle", "degli", "nella", "nelle", "questo", "questa", "quello",
    "quella", "perché", "perche", "cioè", "cioe", "invece", "quindi", "anche", "essere", "senza",
    "più", "piu", "già", "gia", "sono", "dove", "quando", "sulla", "dalla", "dello", "il", "lo",
    "le", "gli", "un", "una", "nel", "dei", "alla", "allo", "alle", "sul", "sui", "dal", "dalle",
    "questi", "queste", "quali", "quale", "ogni", "solo", "ancora", "adesso", "prima", "dopo",
];

/// **ANCHE LA FINESTRA, O IL NUMERO DICE IL FALSO.** Contando solo `crates` si
/// misuravano 11.476 righe italiane mentre `desktop/` — dodicimila righe fra
/// TypeScript e CSS, con un cartiglio di novanta righe in cima a un foglio di
/// stile — non era guardato da nessuno. Un debito invisibile scende a zero da
/// solo senza che nessuno lo paghi.
fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("il crate sta due livelli sotto la radice");
    let mut found = Vec::new();
    for place in ["crates", "desktop/src", "desktop/src-tauri/src", "desktop/scripts"] {
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

/// **DUE FORME DI COMMENTO, PERCHÉ I FOGLI DI STILE NON HANNO `//`.** Il
/// `in_block` viaggia da una riga all'altra: senza, un cartiglio CSS di novanta
/// righe conterebbe una riga sola e il resto passerebbe per codice.
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
    if let Some(rest) = trimmed.strip_prefix("/*") {
        if !rest.contains("*/") {
            *in_block = true;
        }
        return true;
    }
    false
}

/// `31/08/2026` e simili. Una data in un commento è cronaca per definizione.
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

/// **THE WAY ROUND THE CAP: SPLIT INSTEAD OF SHORTENING.** Two `///` groups
/// with a blank line between them are one rustdoc — both attach to the same
/// item — so twelve lines become two blocks of six and the cap is satisfied
/// with nothing removed. Found by general-01 in `desktop/src`, where the same
/// move went from 4 to 41 and orphans the first block outright, since in JSDoc
/// only the last comment before a declaration documents it.
fn splits_one_doc_comment(lines: &[&str], at: usize) -> bool {
    if !lines[at].trim().is_empty() || at == 0 {
        return false;
    }
    if !lines[at - 1].trim_start().starts_with("///") {
        return false;
    }
    let resumes = next_line_with_something_on_it(lines, at);
    resumes < lines.len() && lines[resumes].trim_start().starts_with("///")
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
    italian: usize,
    worst: (usize, String),
}

/// Una riga di commento che porta almeno una parola funzione italiana.
fn looks_italian(line: &str) -> bool {
    let lowered = line.to_lowercase();
    lowered
        .split(|c: char| !c.is_alphabetic() && c != '\'')
        .any(|word| ITALIAN_FUNCTION_WORDS.contains(&word))
}

fn count() -> Counts {
    let mut counts =
        Counts { long_blocks: 0, dated: 0, italian: 0, worst: (0, String::new()) };
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
                if looks_italian(line) {
                    counts.italian += 1;
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
                if run > counts.worst.0 {
                    counts.worst = (run, path.display().to_string());
                }
            }
            run = 0;
            index += 1;
        }
        if run > MAX_BLOCK {
            counts.long_blocks += 1;
        }
    }
    counts
}

#[test]
fn no_new_comment_block_runs_past_the_cap() {
    let counts = count();
    assert!(
        counts.long_blocks <= LONG_BLOCKS_TODAY,
        "blocchi sopra {MAX_BLOCK} righe: {} (il tetto dichiarato è {LONG_BLOCKS_TODAY}). \
         Il più lungo è di {} righe in {}. Accorcia, o sposta la cronaca nel commit",
        counts.long_blocks,
        counts.worst.0,
        counts.worst.1
    );
}

#[test]
fn no_new_comment_tells_a_date() {
    let counts = count();
    assert!(
        counts.dated <= DATED_COMMENTS_TODAY,
        "commenti che citano una data: {} (dichiarati {DATED_COMMENTS_TODAY}). \
         La data la conserva git, con l'autore vero",
        counts.dated
    );
}

/// L'italiano nei commenti scende e non sale: la conversione decisa il
/// 01/09/2026 e' un cantiere, e questo numero e' quanto ne resta.
#[test]
fn the_italian_left_in_the_comments_only_shrinks() {
    let counts = count();
    assert!(
        counts.italian <= ITALIAN_COMMENT_LINES_TODAY,
        "righe di commento ancora in italiano: {} (dichiarate {ITALIAN_COMMENT_LINES_TODAY}). \
         Se stai scrivendo un commento nuovo, scrivilo in inglese; se ne stai \
         traducendo, abbassa il numero",
        counts.italian
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
        ("blocchi lunghi", LONG_BLOCKS_TODAY, counts.long_blocks),
        ("commenti con una data", DATED_COMMENTS_TODAY, counts.dated),
        ("righe italiane", ITALIAN_COMMENT_LINES_TODAY, counts.italian),
    ] {
        assert!(
            declared <= measured + HOW_STALE_A_SEED_MAY_BE,
            "il seme «{what}» dice {declared}, l'albero ne ha {measured}: \
             {} di scarto. O una fusione ha rialzato il tetto, o qualcuno ha \
             potato senza rimisurare — in tutti e due i casi il numero da \
             scrivere qui è {measured}",
            declared - measured
        );
    }
}

/// **CHI MISURA VA MISURATO.** Se `is_comment` o `cites_a_date` smettessero di
/// vedere, i due numeri crollerebbero a zero e le prove resterebbero verdi per
/// sempre.
#[test]
fn the_check_can_still_see_what_it_counts() {
    let mut block = false;
    assert!(is_comment("    // così", &mut block));
    assert!(is_comment("/// e così", &mut block));
    assert!(!is_comment("let x = 1; // non così: la riga è codice", &mut block));
    // E il cartiglio di un foglio di stile, che senza lo stato conterebbe una
    // riga sola.
    assert!(is_comment("/* il cartiglio comincia", &mut block));
    assert!(block, "e la riga dopo è ancora dentro il commento");
    assert!(is_comment("   sta ancora dentro", &mut block));
    assert!(is_comment("   e qui finisce */", &mut block));
    assert!(!block, "il blocco si chiude");
    assert!(!is_comment(".una-classe { color: red; }", &mut block));
    assert!(cites_a_date("// misurato il 31/08/2026"));
    assert!(!cites_a_date("// nessuna data qui"));
    let counts = count();
    // I tre numeri di adesso, per chi pota: `cargo test -p sailor --test
    // comments_do_not_crowd_out_the_code -- --nocapture`. Senza, l'unico modo
    // di conoscerli era azzerare le soglie e leggere il messaggio di fallimento.
    println!(
        "oggi: {} blocchi sopra {MAX_BLOCK} righe, {} commenti con una data, \
         {} righe di commento italiane",
        counts.long_blocks, counts.dated, counts.italian
    );
    // Spezzare un blocco in due non lo accorcia: dodici righe restano dodici.
    let split = ["/// una", "/// due", "/// tre", "/// quattro", "", "/// cinque", "/// sei",
                 "/// sette", "fn qualcosa() {}"];
    assert!(
        splits_one_doc_comment(&split, 4),
        "la riga vuota fra due gruppi /// non spezza un rustdoc, e non deve spezzare il conto"
    );
    let real_end = ["/// una", "", "fn qualcosa() {}"];
    assert!(
        !splits_one_doc_comment(&real_end, 1),
        "una riga vuota seguita da codice chiude il blocco davvero"
    );
    assert!(counts.long_blocks > 0, "zero blocchi lunghi: il contatore non sta guardando");
    assert!(counts.dated > 0, "zero date: il contatore non sta guardando");
    assert!(looks_italian("// perché questo non basta"));
    assert!(
        !looks_italian("// the cap must truncate, not merely be measured"),
        "una riga inglese non deve contare come italiana, o il debito non \
         scenderebbe mai nemmeno traducendo"
    );
    assert!(counts.italian > 0, "zero italiano: il contatore non sta guardando");
}
