//! Spezzare un comando come farebbe una shell, per quel poco che serve ai freni.
//!
//! Vive qui e non dentro un gancio perché lo usano in due — `hooks-off` per
//! trovare il `-C` di git, `pr-title` per estrarre il `--title` — e il primo
//! che ne ha avuto bisogno se l'era scritto in casa. Due copie di un parser di
//! virgolette divergono al primo caso limite corretto da una parte sola.
//!
//! È lo `shlex.split` del Python ridotto all'osso: apici, virgolette, backslash.
//! Non fa espansione di variabili né di glob, e non deve farla: qui si guarda
//! **cosa sta per essere eseguito**, non cosa la shell produrrà.

/// Le parole di un comando. `None` se una virgoletta resta aperta — e allora
/// chi chiama tace, invece di indovinare su un comando che non ha capito.
pub fn split_words(s: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    // Dentro le virgolette doppie il backslash fa scappare
                    // **solo** questi caratteri; davanti a qualunque altro resta
                    // letterale, come in bash e come in `shlex`. Mangiarlo
                    // sempre trasformava `è` in `u00e8`: cinque titoli su
                    // 2.255 commit veri, e il messaggio di rifiuto citava un
                    // titolo diverso da quello che l'autore aveva scritto.
                    if c == '\\' && q == '"' {
                        match chars.clone().next() {
                            Some(next) if matches!(next, '$' | '`' | '"' | '\\' | '\n') => {
                                chars.next();
                                current.push(next);
                                continue;
                            }
                            _ => {}
                        }
                    }
                    current.push(c);
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    started = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                        started = true;
                    }
                }
                c if c.is_whitespace() => {
                    if started || !current.is_empty() {
                        words.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                c => {
                    current.push(c);
                    started = true;
                }
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    if started || !current.is_empty() {
        words.push(current);
    }
    Some(words)
}

/// La valvola scritta **davanti al comando**, che è dove i rifiuti dicono di
/// metterla: `TITOLO_RICHIESTA=off gh pr create …`.
///
/// LEGGERLA DALL'AMBIENTE NON BASTA, ed è il motivo per cui questa esiste. Il
/// gancio gira nel processo dell'harness, non nella shell del comando, quindi un
/// prefisso sulla riga non gli arriva mai: `std::env::var` risponde sempre
/// «assente». Provato il 19/08/2026 su `pr-title` — il rifiuto insegnava una via
/// d'uscita che rifiutava a sua volta — e `hooks-off` aveva la stessa forma.
/// Una valvola annunciata e inerte è peggio di nessuna: chi la trova chiusa se ne
/// inventa una che non lascia traccia.
///
/// Sta sulla riga di comando e non nell'ambiente per la stessa ragione di
/// `SESSION_REPLACES=1` in `handoff`: esportata una volta esenterebbe in silenzio
/// tutto ciò che viene dopo; scritta qui vale per quel comando e resta nel
/// registro.
///
/// Conta solo **in testa** a un segmento, fra le assegnazioni che precedono il
/// comando: più in là è una stringa qualunque, e prenderla per una valvola
/// aprirebbe il gate a chi la nomina nel corpo di una richiesta.
pub fn valve_in_front(command: &str, valve: &str) -> bool {
    command.split(|c| c == ';' || c == '&' || c == '|').any(|segment| {
        segment
            .split_whitespace()
            .take_while(|w| w.contains('=') && !w.starts_with('-'))
            .any(|w| w.eq_ignore_ascii_case(valve))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valve_counts_in_front_of_the_command_and_nowhere_else() {
        let v = "TITOLO_RICHIESTA=off";
        assert!(valve_in_front("TITOLO_RICHIESTA=off gh pr create -t x", v));
        assert!(valve_in_front("cd /tmp && TITOLO_RICHIESTA=off gh pr create -t x", v));
        assert!(!valve_in_front("gh pr create -t x --body TITOLO_RICHIESTA=off", v));
        assert!(!valve_in_front("gh pr create -t x", v));
        // Un'altra valvola non apre questa.
        assert!(!valve_in_front("GANCI_SPENTI=off gh pr create -t x", v));
    }

    #[test]
    fn it_splits_words_like_the_python_shlex_did() {
        assert_eq!(
            split_words(r#"git -C /tmp/a commit -m "feat: x y""#).unwrap(),
            vec!["git", "-C", "/tmp/a", "commit", "-m", "feat: x y"]
        );
        assert_eq!(split_words("a  b\tc").unwrap(), vec!["a", "b", "c"]);
        // una stringa vuota fra virgolette è una parola, non niente
        assert_eq!(
            split_words(r#"git commit -m """#).unwrap(),
            vec!["git", "commit", "-m", ""]
        );
    }

    #[test]
    fn an_unbalanced_quote_makes_it_give_up_rather_than_guess() {
        assert!(split_words(r#"git commit -m "aperta"#).is_none());
    }

    #[test]
    fn a_backslash_escapes_the_next_character() {
        assert_eq!(split_words(r"a\ b").unwrap(), vec!["a b"]);
        assert_eq!(split_words(r#""a\"b""#).unwrap(), vec![r#"a"b"#]);
    }

    /// Dentro le virgolette doppie il backslash è letterale davanti a tutto ciò
    /// che non è `$`, `` ` ``, `"`, `\` o newline. Il caso che l'ha insegnato:
    /// un titolo di commit contenente `è` diventava `u00e8`, e il gancio
    /// rimproverava citando un titolo che nessuno aveva scritto.
    #[test]
    fn inside_double_quotes_a_backslash_is_literal_unless_it_escapes_one_of_five() {
        assert_eq!(split_words(r#""non si è caricato""#).unwrap(), vec![r"non si è caricato"]);
        assert_eq!(split_words(r#""a\nb""#).unwrap(), vec![r"a\nb"]);
        assert_eq!(split_words(r#""costa \$5""#).unwrap(), vec!["costa $5"]);
        assert_eq!(split_words(r#""a\\b""#).unwrap(), vec![r"a\b"]);
    }
}
