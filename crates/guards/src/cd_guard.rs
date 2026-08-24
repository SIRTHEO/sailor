//! Ferma i `cd <path> && …` fuori dal workspace che ha già il suo freno.
//!
//! Porta di `skills/hooks/cd-guard.py`. Il comportamento deve restare identico:
//! la prova che conta non sono i casi scritti qui sotto, è il confronto con lo
//! script Python su comandi veri presi dal registro (`tests/equivalence.rs`).
//!
//! PERCHÉ ESISTE. Lo stato della shell non persiste fra due chiamate: un `cd`
//! in un comando composto vale per quel comando e sparisce. Misura del
//! 10/08/2026 su 57.251 comandi in trenta giorni — dove il freno c'è, i comandi
//! con `cd` sono il 5%; dove non c'è, il 27-40%.
//!
//! DUE GRAVITÀ, e non una. Si **blocca** dove la sostituzione è meccanica e
//! certa (`cd X && git …` → `git -C X …`, nessun giudizio da dare) e si
//! **avvisa** dove la riscrittura dipende dal comando. Bloccare tutto vorrebbe
//! dire trecento stop al giorno; avvisare e basta non cambia le abitudini.

use hook_io::Decision;
use regex::Regex;
use std::sync::OnceLock;

fn cd_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `(?![-\s])(\S+)` del Python diventa `([^-\s]\S*)`: `\S+` esclude già gli
    // spazi, quindi al lookahead restava solo il compito di vietare il trattino
    // iniziale — e `cd -` non ha un percorso da riscrivere.
    RE.get_or_init(|| Regex::new(r"(?:^|[\n;]|&&|\|\|)\s*(?:cd|pushd)\s+([^-\s]\S*)").unwrap())
}

fn follows_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:&&|\|\||;|\n)\s*\S").unwrap())
}

/// Toglie ciò che non è comando: heredoc, sostituzioni, apici, virgolette.
///
/// Un `cd` dentro uno heredoc appartiene allo script che lo riceve, non a
/// questa chiamata: bloccarlo sarebbe un falso positivo. Meglio cieco che
/// sbagliato — è la stessa scelta dell'originale.
pub fn strip_noise(command: &str) -> String {
    let s = strip_heredocs(command);
    let s = strip_command_substitutions(&s);
    let s = strip_delimited(&s, '`', '`');
    let s = strip_delimited(&s, '\'', '\'');
    strip_delimited(&s, '"', '"')
}

/// Dal marcatore alla sua riga di chiusura. Se il marcatore non si chiude, via
/// tutto ciò che segue: un heredoc aperto si porta dietro il resto del comando.
fn strip_heredocs(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let bytes = command.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'<' {
            let mut j = i + 2;
            if j < bytes.len() && bytes[j] == b'-' {
                j += 1;
            }
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            let quote = if j < bytes.len() && (bytes[j] == b'\'' || bytes[j] == b'"') {
                let q = bytes[j];
                j += 1;
                Some(q)
            } else {
                None
            };
            let start = j;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if j == start {
                // `<<` senza marcatore: non è uno heredoc, si copia e si va avanti
                out.push_str("<<");
                i += 2;
                continue;
            }
            let marker = &command[start..j];
            if let Some(q) = quote {
                if j < bytes.len() && bytes[j] == q {
                    j += 1;
                }
            }
            out.push(' ');
            match find_terminator(command, j, marker) {
                Some(end) => i = end,
                // marcatore mai chiuso: il resto del comando è dentro lo heredoc
                None => return out,
            }
        } else {
            let ch = command[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// La riga che contiene solo il marcatore, eventualmente indentata.
fn find_terminator(s: &str, from: usize, marker: &str) -> Option<usize> {
    let mut pos = from;
    while let Some(nl) = s[pos..].find('\n') {
        let line_start = pos + nl + 1;
        let line_end = s[line_start..]
            .find('\n')
            .map(|k| line_start + k)
            .unwrap_or(s.len());
        if s[line_start..line_end].trim() == marker {
            return Some(line_end);
        }
        pos = line_start;
        if pos >= s.len() {
            break;
        }
    }
    None
}

/// `R=$(cd .claude && pwd)` è l'idioma normale per ottenere un percorso
/// assoluto, e quel `cd` vive in una sottoshell: non tocca la working-dir di
/// chi lo scrive, quindi non è l'errore che questo freno cerca.
fn strip_command_substitutions(command: &str) -> String {
    let mut current = command.to_string();
    for _ in 0..4 {
        // annidamenti: $( … $( … ) … )
        let mut out = String::with_capacity(current.len());
        let bytes = current.as_bytes();
        let mut i = 0;
        let mut changed = false;
        while i < bytes.len() {
            if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
                if let Some(close) = current[i + 2..].find(')') {
                    let inner = &current[i + 2..i + 2 + close];
                    if !inner.contains('(') {
                        out.push(' ');
                        i = i + 2 + close + 1;
                        changed = true;
                        continue;
                    }
                }
            }
            let ch = current[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        current = out;
        if !changed {
            break;
        }
    }
    current
}

/// Sostituisce con uno spazio ogni coppia di delimitatori, come farebbe
/// `re.sub(r"'[^']*'", ' ', …)`.
///
/// **Un delimitatore spaiato non è una coppia**, e va lasciato dov'è insieme a
/// tutto ciò che segue: la regex semplicemente non trova il match. La prima
/// versione qui buttava via il resto del comando, ed è l'unica divergenza che
/// il confronto su 3.137 comandi veri ha trovato — un `grep -oE '…`…`…'` la cui
/// coda spariva, e con lei il `cd` che andava segnalato.
fn strip_delimited(command: &str, open: char, close: char) -> String {
    let mut out = String::with_capacity(command.len());
    let mut rest = command;
    while let Some(i) = rest.find(open) {
        let after = &rest[i + open.len_utf8()..];
        match after.find(close) {
            Some(j) => {
                out.push_str(&rest[..i]);
                out.push(' ');
                rest = &after[j + close.len_utf8()..];
            }
            None => break, // spaiato: da qui in poi si copia tutto tale e quale
        }
    }
    out.push_str(rest);
    out
}

pub fn judge(command: &str) -> Decision {
    let bare = strip_noise(command);
    let Some(m) = cd_pattern().captures(&bare) else {
        return Decision::Pass;
    };
    let whole = m.get(0).unwrap();
    let rest = &bare[whole.end()..];
    if !follows_pattern().is_match(rest) {
        return Decision::Pass; // `cd x` e basta: legittimo
    }
    let target = m.get(1).unwrap().as_str();
    let trimmed = rest.trim_start_matches([' ', '\t', ';', '&', '|', '\n']);
    let following: Vec<&str> = trimmed.split_whitespace().collect();

    if following.first() == Some(&"git") {
        let tail = following[1..following.len().min(3)].join(" ");
        return Decision::Block(format!(
            "niente `cd {target} && git …`: lo stato della shell non persiste fra \
             due chiamate, e git sa già lavorare altrove.\n  · scrivi:  git -C {target} {tail} …"
        ));
    }
    // IL RAMO CHE AVVISA PROPONE UN SOSTITUTO, non spiega l'errore.
    //
    // Misurato il 21/08/2026 sul registro: dove questo gancio NEGA il gesto si
    // ripete nel 48% dei casi entro dieci minuti; dove avvisa, nell'80% su 2.184
    // righe. La differenza non è disciplina — è che il ramo che nega offre
    // `git -C` e questo non offriva niente di eseguibile. I percorsi assoluti non
    // servono a chi deve eseguire un comando *dentro* un'altra cartella, e il
    // consiglio di fare `cd` da solo prometteva un effetto che non esiste: la
    // working-dir viene ripristinata dopo ogni chiamata. Un messaggio che non
    // chiede niente di eseguibile lo scavalcano tutti, anche chi conosce la
    // regola — nella stessa notte l'hanno scavalcato il capitano e il macchinista.
    //
    // `env -C` fa ciò che serve ed è provato su questa macchina. Resta un avviso:
    // se fra una settimana la percentuale non scende sotto il 50% con
    // l'alternativa in mano, l'avviso è rumore e si toglie invece di rafforzarlo.
    // Soglia fissata dal capitano il 21/08/2026.
    Decision::Warn(format!(
        "`cd {target} && …`: lo stato della shell non persiste fra due chiamate, \
         e nemmeno `cd {target}` da solo serve — la working-dir torna indietro \
         dopo ogni comando.\n  · scrivi:  env -C {target} <comando>\n  · oppure un \
         prefisso, se il comando accetta percorsi — R={target}; <comando> \"$R/file\""
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Il ramo che avvisa deve dare un comando da eseguire, non una spiegazione.
    ///
    /// MUTANTE: rimesso il messaggio vecchio, questo caso va in rosso — ed era
    /// proprio il messaggio vecchio a essere scavalcato quattro volte su cinque.
    #[test]
    fn the_warning_branch_offers_a_command_to_run() {
        match judge("cd /repo && cargo test") {
            Decision::Warn(m) => {
                assert!(m.contains("env -C /repo"), "manca il sostituto: {m}");
                // La vecchia via d'uscita prometteva un effetto che non esiste:
                // `cd` da solo passa, ma la working-dir torna indietro subito.
                assert!(
                    !m.contains("resta permesso"),
                    "non si suggerisce un gesto che non produce l'effetto: {m}"
                );
            }
            other => panic!("atteso un avviso, non {other:?}"),
        }
        // Dove un sostituto specifico esiste già, resta quello e non `env -C`.
        match judge("cd /repo && git status") {
            Decision::Block(m) => assert!(m.contains("git -C /repo"), "{m}"),
            other => panic!("atteso un blocco, non {other:?}"),
        }
    }

    /// Gli stessi tredici casi dell'originale, con gli stessi esiti attesi.
    #[test]
    fn it_matches_the_python_selftest_cases() {
        let cases: &[(&str, &str)] = &[
            ("git -C /repo status", "pass"),
            ("cd /repo", "pass"),
            ("cd /repo && git status", "block"),
            ("cd /repo && npx vitest", "warn"),
            ("ls; cd /repo && git log", "block"),
            ("cd /repo\nmake", "warn"),
            (r#"echo "cd /repo && git push""#, "pass"),
            ("echo 'cd /repo && git push'", "pass"),
            ("bash <<EOF\ncd /repo && git push\nEOF", "pass"),
            (r#"R=$(cd .claude && pwd); echo "$R""#, "pass"),
            ("R=`cd .claude && pwd`; echo \"$R\"", "pass"),
            ("cd -", "pass"),
            ("python3 - <<PY\nimport os\nPY", "pass"),
        ];
        for (command, expected) in cases {
            let got = match judge(command) {
                Decision::Pass => "pass",
                Decision::Warn(_) => "warn",
                Decision::Block(_) => "block",
                // questo freno non nega permessi: se ci arriva, è un difetto
                Decision::Deny(_) => "deny",
            };
            assert_eq!(&got, expected, "caso: {command:?}");
        }
    }

    #[test]
    fn an_unterminated_heredoc_swallows_what_follows() {
        assert_eq!(judge("bash <<EOF\ncd /repo && git push"), Decision::Pass);
    }

    /// Uno heredoc CHIUSO non si porta via il comando che viene dopo.
    ///
    /// È il caso opposto a quello qui sopra, e senza di lui il freno diventa
    /// cieco a metà giornata: basta che `find_terminator` non trovi più la riga
    /// di chiusura perché ogni heredoc risulti aperto e tutto ciò che segue —
    /// compreso un `cd /altrove && git …` — sparisca prima del giudizio.
    #[test]
    fn a_closed_heredoc_leaves_the_command_after_it_visible() {
        match judge("bash <<EOF\nsolo testo\nEOF\ncd /repo && git push") {
            Decision::Block(m) => assert!(m.contains("git -C /repo"), "{m}"),
            other => panic!("il comando dopo lo heredoc chiuso deve restare visibile, non {other:?}"),
        }
        // E ciò che sta DENTRO lo heredoc resta dello script che lo riceve.
        assert_eq!(
            judge("bash <<EOF\ncd /repo && git push\nEOF\nls"),
            Decision::Pass
        );
    }

    /// La riga di chiusura si cerca anche oltre la prima riga del corpo.
    ///
    /// Il passo intermedio si prova qui perché `strip_noise` è una catena di
    /// cinque funzioni: un indice sbagliato dentro `find_terminator` si vede
    /// sul risultato finale solo per certi comandi, e in tutti gli altri passa.
    #[test]
    fn the_terminator_is_the_line_that_holds_only_the_marker() {
        let s = "bash <<EOF\nprima\nEOF\ncoda";
        // `from` è il byte subito dopo il marcatore aperto, cioè il `\n` di riga 1.
        let end = find_terminator(s, 10, "EOF").expect("la riga EOF c'è");
        assert_eq!(&s[..end], "bash <<EOF\nprima\nEOF");
        // Una riga che CONTIENE il marcatore non lo chiude.
        assert_eq!(find_terminator("bash <<EOF\nEOFX\ncoda", 10, "EOF"), None);
        // Indentata sì: `<<-` la ammette e il confronto passa da `trim`.
        assert!(find_terminator("bash <<-EOF\nx\n\tEOF\n", 11, "EOF").is_some());
    }

    /// Il primo passo della catena, guardato da solo: cosa resta del comando
    /// dopo aver tolto gli heredoc. Le asserzioni sono sul testo esatto perché
    /// è l'unico modo di sorvegliare i confini — un byte in più o in meno
    /// cambia quale `cd` il freno vedrà.
    #[test]
    fn heredocs_are_cut_from_the_marker_to_their_closing_line() {
        let cases: &[(&str, &str)] = &[
            ("bash <<EOF\nbody\nEOF\ntail", "bash  \ntail"),
            // `<<-` ammette marcatore e corpo indentati.
            ("bash <<-EOF\n\tbody\n\tEOF\ntail", "bash  \ntail"),
            // Uno spazio fra `<<` e il marcatore è legale in shell.
            ("bash << EOF\nbody\nEOF\ntail", "bash  \ntail"),
            // Marcatore fra apici: la chiusura si cerca sul nome nudo.
            ("bash <<'EOF'\nbody\nEOF\ntail", "bash  \ntail"),
            // Apice mai chiuso: il marcatore resta `EOF` e la riga lo chiude.
            ("bash <<'EOF\nEOF\ntail", "bash  \ntail"),
            // Mai chiuso: il resto del comando è dentro lo heredoc.
            ("bash <<EOF", "bash  "),
            ("bash <<'EOF", "bash  "),
            // `<<` senza marcatore non è uno heredoc: si copia e si va avanti.
            ("bash <<", "bash <<"),
            ("bash << ", "bash << "),
            // `<<<` è un here-string, non uno heredoc: niente da togliere.
            ("bash <<<\"$x\"", "bash <<<\"$x\""),
            // Un `<` solo è una redirezione, anche in fondo al comando.
            ("echo <", "echo <"),
            ("echo <file", "echo <file"),
        ];
        for (command, expected) in cases {
            assert_eq!(&strip_heredocs(command), expected, "caso: {command:?}");
        }
    }

    /// `$( … )` sparisce, e con lui il `cd` che vive nella sottoshell. Il
    /// giudizio finale non basta a sorvegliare questi indici: un `cd` dentro
    /// una sostituzione non ha davanti un separatore, quindi non lo vedrebbe
    /// nemmeno se la sostituzione restasse lì.
    #[test]
    fn command_substitutions_are_replaced_by_a_single_space() {
        let cases: &[(&str, &str)] = &[
            ("echo $(pwd) done", "echo   done"),
            // Annidata: quattro passate, dalla più interna in fuori.
            ("ROOT=$(dirname $(pwd))", "ROOT= "),
            // `$` senza `(` non apre niente, nemmeno in fondo al comando.
            ("echo $R (ok)", "echo $R (ok)"),
            ("costa 100$", "costa 100$"),
            // Una parentesi senza `$` davanti non è una sostituzione.
            ("echo (x)", "echo (x)"),
        ];
        for (command, expected) in cases {
            assert_eq!(
                &strip_command_substitutions(command),
                expected,
                "caso: {command:?}"
            );
        }
    }

    /// Il `cd` di una sottoshell non è quello che questo freno cerca, ma il
    /// separatore che lo precede sì: senza togliere la sostituzione, un
    /// `$(ls; cd /repo && git push)` diventerebbe un blocco a torto.
    #[test]
    fn a_cd_inside_a_subshell_is_not_this_guards_business() {
        assert_eq!(judge("R=$(ls; cd /repo && git push); echo \"$R\""), Decision::Pass);
    }

    /// Ogni coppia di delimitatori vale uno spazio; uno spaiato lascia in pace
    /// tutto ciò che segue — è l'unica divergenza che il confronto con il
    /// Python su 3.137 comandi veri aveva trovato.
    #[test]
    fn each_pair_of_delimiters_becomes_one_space_and_an_odd_one_is_left_alone() {
        assert_eq!(strip_delimited("echo 'a' 'b'", '\'', '\''), "echo    ");
        assert_eq!(strip_delimited("echo 'a'x'b'y", '\'', '\''), "echo  x y");
        // Spaiato: da qui in poi si copia tale e quale, coda compresa.
        assert_eq!(
            strip_delimited("grep -oE '`x`' ; cd /repo", '`', '`'),
            "grep -oE ' ' ; cd /repo"
        );
        assert_eq!(strip_delimited("echo 'aperto", '\'', '\''), "echo 'aperto");
    }
}
