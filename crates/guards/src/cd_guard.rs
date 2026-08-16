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
    Decision::Warn(format!(
        "`cd {target} && …`: lo stato della shell non persiste fra due chiamate, \
         e la working-dir sbagliata è già costata errori misurati.\n  · usa percorsi \
         assoluti, o un prefisso — R={target}; <comando> \"$R/file\"\n  · `cd {target}` \
         da solo, in una chiamata a parte, resta permesso."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            };
            assert_eq!(&got, expected, "caso: {command:?}");
        }
    }

    #[test]
    fn an_unterminated_heredoc_swallows_what_follows() {
        assert_eq!(judge("bash <<EOF\ncd /repo && git push"), Decision::Pass);
    }
}
