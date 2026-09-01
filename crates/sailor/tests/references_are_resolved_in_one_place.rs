//! **I RINVII SI SCIOLGONO IN UN POSTO SOLO, E QUESTO CONTROLLO LO PRETENDE.**
//!
//! **PERCHÉ ESISTE.** Il guasto 28 non è che il deposito non risolvesse i
//! rinvii: è che *risolvere i rinvii* fosse una faccenda della singola azione.
//! Da lì il difetto si è ripresentato due volte con la stessa forma. Prima come
//! assenza — due azioni su nove ce l'avevano, e le altre sette ricevevano
//! `{"$from": …}` come oggetto. Poi come **cura peggiore del male**: la riga è
//! stata ricopiata, e il 01/09/2026 in quest'albero se ne contavano **dodici su
//! sedici azioni registrate**, con quattro ancora scoperte (`history_ask`,
//! `detect_tools`, `trigger`, `subflow`). Dodici copie della stessa riga sono il
//! guasto 10 in dodici esemplari, e ogni azione nuova continuava a nascere
//! senza che nulla diventasse rosso.
//!
//! **COSA IMPEDISCE, CHE UNA PROVA DI COMPORTAMENTO NON PUÒ.**
//! `crates/flow/tests/a_reference_reaches_every_action.rs` prova che l'ingresso
//! arriva sciolto; resterebbe **verde** se domani qualcuno rimettesse la riga
//! dentro un'azione, perché il comportamento non cambierebbe. Cambierebbe solo
//! il numero di posti in cui la regola vive — che è il guasto. Questo lo si
//! misura contando i posti, e si conta qui.
//!
//! **PERCHÉ LEGGE IL CODICE RIPULITO, E LA PRIMA VERSIONE ERA CIECA.** Saltare
//! i blocchi `#[cfg(test)]` vuol dire contare graffe, e la prima stesura le
//! contava sul testo grezzo: quelle dentro le stringhe e i commenti entravano
//! nel conto. Un blocco che non si bilancia **inghiotte tutto fino a fine
//! file**, in silenzio. Misurato da un giudice il 01/09/2026 su codice
//! spedito e non toccato da nessuno: cinque file ciechi — `actions/src/lib.rs`
//! da riga 4200, `models/src/remaining.rs` da 325, `models/src/store.rs` da 37,
//! `sailor/src/flow_cmd.rs` da 2448, `ui/src/gather.rs` da 240 — e una funzione
//! vera, che compilava, con dentro una chiamata a `resolve_references`, messa
//! in fondo a `flow_cmd.rs`: **la prova non l'ha vista**, due verdi e zero
//! rossi.
//!
//! Il difetto non era il falso positivo, che si vede: era il **falso negativo
//! silenzioso**, cioè esattamente ciò per cui questo file esiste. La cura è
//! doppia e le due metà servono a cose diverse: si contano le graffe sul codice
//! **ripulito** da commenti, stringhe e letterali di carattere; e se uno
//! scavalcamento arriva comunque a fine file, la prova **diventa rossa
//! nominando il file** invece di proseguire cieca. Un controllo che può
//! spegnersi da solo non è un controllo.
//!
//! **NON È UN ANALIZZATORE.** Non pretende di capire il Rust: riconosce
//! commenti, stringhe (grezze comprese) e letterali di carattere, e nient'altro.
//! Il prezzo è dichiarato, ed è il modo in cui questa casa scrive i controlli
//! sul testo — come `identifiers_are_in_english`. Ma quando non capisce, si
//! ferma dicendolo.

use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// Il nome della funzione che scioglie i rinvii, cercato come testo.
const THE_CALL: &str = "resolve_references(";

/// I due soli file del codice spedito che possono nominarla, col perché.
///
/// **NON È UN ELENCO DA ALLUNGARE.** Una voce in più qui è una copia in più
/// della regola, cioè il guasto che questo controllo esiste per fermare. Chi ha
/// bisogno dei rinvii in un posto nuovo li riceve già sciolti: passa da
/// `step_input` come tutti.
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

/// Ogni `.rs` sotto `crates/*/src`, cioè il codice che gira in produzione.
///
/// Le cartelle `tests/` restano fuori apposta: una prova che invoca `execute`
/// direttamente deve poter comporre l'ingresso come lo comporrebbe l'esecutore,
/// e chiamare la funzione vera per farlo è giusto — è ricopiarne la *decisione*
/// dentro un'azione che è il difetto.
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

/// Il file con commenti, stringhe e letterali di carattere sostituiti da spazi.
///
/// **LE RIGHE RESTANO AL LORO POSTO**: ogni «a capo» dentro ciò che si toglie
/// viene rimesso, così il numero di riga di una violazione è quello vero e chi
/// legge il messaggio apre il file al punto giusto.
///
/// **UN APICE NON È SEMPRE UN CARATTERE**: `'a` è una vita, `'{'` è una graffa
/// che non conta. Si distinguono guardando avanti — un letterale di carattere si
/// chiude entro due passi — e chi non si chiude è una vita, di cui si butta via
/// il solo apice. Senza questa distinzione una vita qualunque farebbe sparire
/// tutto il codice fino all'apice successivo.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut kept = String::with_capacity(text.len());
    let mut index = 0usize;

    // Ricopia gli «a capo» di un pezzo scartato, per non spostare le righe.
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

        // Commento di riga, documentazione compresa.
        if current == '/' && next == Some('/') {
            let mut end = index;
            while end < chars.len() && chars[end] != '\n' {
                end += 1;
            }
            index = end;
            continue;
        }

        // Commento a blocchi, che in Rust si annida.
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

        // Stringa grezza: `r"…"`, `r#"…"#`, `br##"…"##`. Il prefisso vale solo
        // se non è la coda di un identificatore.
        if (current == 'r' || current == 'b') && !preceded_by_identifier(&chars, index) {
            if let Some((end, newlines)) = raw_string_end(&chars, index) {
                for _ in 0..newlines {
                    kept.push('\n');
                }
                index = end;
                continue;
            }
        }

        // Stringa normale, con le sue sequenze di escape.
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

        // Letterale di carattere, distinto da una vita.
        if current == '\'' {
            if let Some(end) = char_literal_end(&chars, index) {
                index = end;
                continue;
            }
            // Una vita: si butta il solo apice, il nome resta e non contiene
            // graffe.
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

/// Dove finisce una stringa grezza che comincia a `index`, e quante righe
/// occupa. `None` se lì non ne comincia nessuna.
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

/// Dove finisce un letterale di carattere che comincia a `index`. `None` quando
/// quell'apice apre una vita e non un carattere.
fn char_literal_end(chars: &[char], index: usize) -> Option<usize> {
    match chars.get(index + 1) {
        Some('\\') => {
            // `'\n'`, `'\''`, `'\u{7b}'`: si cerca l'apice di chiusura poco
            // avanti, senza inseguirlo per tutto il file.
            //
            // **SI PARTE DA `index + 3`, E IL CARATTERE PRIMA ERA UN BUCO.**
            // Con `index + 2` la ricerca comincia dal carattere **scappato**:
            // su `'\''` quello è un apice, quindi il letterale veniva chiuso un
            // carattere troppo presto. L'apice avanzato si ricombinava con ciò
            // che seguiva — in `['\'','"']` il pezzo `','` passava per un
            // carattere — e il `"` successivo apriva una **stringa fantasma**
            // che cancellava in silenzio tutto fino al `"` dopo, codice
            // spedito compreso. Misurato il 01/09/2026 da un giudice: la stessa
            // funzione che questa prova deve prendere, messa fra due costanti
            // così, passava verde. E `'\''` sta già in tre punti dell'albero
            // (`inventory/src/lib.rs:573` e `:601`, `terminal/src/routing.rs:313`),
            // salvi solo dallo spazio che rustfmt mette dopo la virgola: un
            // controllo che si spegne per una spaziatura si spegne per sbaglio.
            (index + 3..=index + 10)
                .find(|at| chars.get(*at) == Some(&'\''))
                .map(|at| at + 1)
        }
        Some(_) if chars.get(index + 2) == Some(&'\'') => Some(index + 3),
        _ => None,
    }
}

/// Il codice spedito di un file: ripulito, e senza i blocchi `#[cfg(test)]`.
///
/// **UNO SCAVALCAMENTO CHE NON SI CHIUDE È UN ERRORE, NON UN SILENZIO.** Se le
/// graffe non tornano entro la fine del file, si torna `Err` con la riga da cui
/// il blocco è cominciato: da lì in poi il controllo non vedrebbe più niente, ed
/// è il modo in cui questa prova è già stata cieca su cinque file.
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
    for file in shipped_sources(&root) {
        let relative = file
            .strip_prefix(&root)
            .expect("i file vengono da sotto la radice")
            .display()
            .to_string();
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("leggere {relative}: {error}"));
        let code = match shipped_code(&text) {
            Ok(code) => code,
            Err(line) => {
                blind.push(format!("{relative}:{line}"));
                continue;
            }
        };
        for (number, line) in code.lines().enumerate() {
            if line.contains(THE_CALL) && !allowed.contains(&relative.as_str()) {
                copies.push(format!("{relative}:{}: {}", number + 1, line.trim()));
            }
        }
    }

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

/// **E IL POSTO DICHIARATO DEVE ESSERE DAVVERO OCCUPATO.**
///
/// Senza questa metà, la prova qui sopra diventerebbe verde anche togliendo la
/// risoluzione da tutte le parti: zero copie, e zero cure. È lo stesso difetto
/// del campo `partly` calcolato e mai interrogato del guasto 40 — una difesa
/// che si può soddisfare non facendo niente.
#[test]
fn the_one_place_that_may_resolve_them_actually_does() {
    let root = repository_root();
    let composer = root.join("crates/flow/src/executor.rs");
    let text = std::fs::read_to_string(&composer).expect("leggere l'esecutore");
    let shipped = shipped_code(&text).expect("l'esecutore si legge fino in fondo");

    assert!(
        shipped.contains(THE_CALL),
        "`crates/flow/src/executor.rs` non scioglie più nessun rinvio: allora \
         non li scioglie nessuno, e ogni `{{\"$from\": …}}` arriva alle azioni \
         come oggetto"
    );
    let inside_step_input = shipped
        .split_once("pub fn step_input(")
        .map(|(_, after)| after.to_owned())
        .expect("`step_input` esiste");
    assert!(
        inside_step_input.contains(THE_CALL),
        "la chiamata non sta più dentro `step_input`: fuori di lì non è più \
         l'unico punto attraversato da ogni passo"
    );
}

// ── che il modo di leggere veda davvero ──────────────────────────────────
//
// **CHI MISURA VA MISURATO.** Le tre prove qui sotto interrogano il lettore,
// non il codice spedito: sono nate dal fatto che la prima versione di questo
// file passava, verde, con una chiamata vera davanti agli occhi.

/// Le graffe dentro stringhe, commenti e caratteri **non contano**, e un apice
/// scappato non apre una stringa fantasma.
///
/// Ognuno di questi casi, da solo, bastava a mandare cieca una versione di
/// questo lettore. L'ultimo — `'\''` **senza spazio** dopo la virgola — è il
/// secondo buco, trovato il 01/09/2026 dopo la prima riparazione: il letterale
/// si chiudeva un carattere troppo presto, il `","` che seguiva passava per un
/// carattere, e il `"` dopo apriva una stringa che inghiottiva il codice
/// spedito fino alla stringa successiva. La `const TAIL` in fondo è lì apposta:
/// è il `"` che chiudeva la stringa fantasma, cioè ciò che rendeva il difetto
/// silenzioso invece che rumoroso.
///
/// **LO SPAZIO NON È UN DETTAGLIO DI STILE.** Con `['\'', '"']`, come lo scrive
/// rustfmt, il difetto non si vedeva; senza, sì. Un controllo che dipende da una
/// spaziatura si spegne per sbaglio, e `'\''` sta già in tre punti dell'albero.
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
        code.contains(THE_CALL),
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

/// Gli altri escape restano letti giusti, e non è una formalità: la cura è
/// stata spostare l'inizio della ricerca di un carattere, e un carattere in più
/// romperebbe `'\n'` senza che nulla lo dica.
///
/// **OGNI CASO PORTA LA CODA CHE MORDE, E PRIMA NO.** Fino a stasera la fixture
/// era `const C: char = <letterale>;` e un giudice ha misurato che restava
/// **verde con tutte e tre le mutazioni** del ramo dell'escape — compresa
/// `index + 4`, cioè proprio la riparazione opposta da cui il commento qui
/// sopra prometteva di difendere. Il motivo: un letterale letto storto, se non
/// ha accanto un apice doppio, non apre nessuna stringa fantasma, quindi non
/// sposta niente e le due asserzioni restano vere qualunque cosa si faccia.
/// Era una prova che non poteva venire diversa — il metro di casa applicato a
/// se stesso.
///
/// Adesso il letterale sta dentro `[<letterale>,'"']`: se viene consumato di un
/// carattere sbagliato, l'apice doppio che segue diventa l'apertura di una
/// stringa che si chiude solo sul `"` della `const T`, e con lei sparisce il
/// codice in mezzo. Le due asserzioni cominciano a poter fallire.
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
            code.contains(THE_CALL),
            "dopo {literal} il codice spedito è sparito:\n{code}"
        );
        assert!(
            !code.contains(tail),
            "dopo {literal} la stringa in fondo è sopravvissuta: il letterale è stato \
             letto storto e ha spostato tutto ciò che segue"
        );
    }
}

/// **UNO SCAVALCAMENTO CHE NON SI CHIUDE SI DICHIARA.** È il caso che prima
/// passava in silenzio, e il silenzio era il difetto.
#[test]
fn a_test_block_that_never_closes_is_reported_instead_of_swallowing_the_file() {
    let text = "fn prima() {}\n#[cfg(test)]\nmod tests {\n    fn a() {\n";

    let refused = shipped_code(text).expect_err("il blocco non si chiude");

    assert_eq!(refused, 2, "la riga da cui il file diventa cieco");
}

/// Il codice ripulito conserva i numeri di riga: un messaggio che manda alla
/// riga sbagliata fa cercare nel posto sbagliato, ed è il guasto 11 in
/// miniatura.
#[test]
fn cleaning_the_code_keeps_the_line_numbers() {
    let text = "uno\n/* due\n   tre */\nquattro\n";

    let code = code_only(text);

    assert_eq!(code.lines().count(), 4, "{code:?}");
    assert_eq!(code.lines().nth(3), Some("quattro"));
}
