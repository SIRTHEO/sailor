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

#[cfg(test)]
mod tests {
    use super::*;

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
