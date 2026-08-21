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

/// Dove finisce un comando e comincia il successivo, **senza spezzare dentro
/// una stringa**.
///
/// PERCHÉ UN FRENO CHE NEGA NE HA BISOGNO. Il bersaglio che un rifiuto nomina
/// dev'essere un argomento vero di *quel* comando. Il freno di terze parti
/// prende ciò che segue la parola di cancellazione fino alla fine del testo, e
/// il 21/08/2026 ha negato sei volte accusando un file che il comando stava
/// soltanto **leggendo**. Non conoscere i separatori e non conoscere le
/// virgolette sono lo stesso difetto visto da due lati.
///
/// COSA CONTA COME CONFINE: `&&`, `||`, `;`, `|`, l'a capo, e l'apertura e la
/// chiusura di una sostituzione (`$(…)`, backtick) — perché lì dentro c'è un
/// comando vero. `&` da solo NON spezza: in `rm -rf x 2>&1` spezzerebbe una
/// redirezione a metà, e il rifiuto finirebbe per nominare `1`.
///
/// I FRAMMENTI DI STRINGA ESCONO DALL'ELENCO. In `echo "$(date) rm -rf /x"` la
/// coda dopo la sostituzione è testo dentro le virgolette, non un comando:
/// giudicarla è esattamente il falso positivo che si sta togliendo a monte.
///
/// `worktree_deletes` ha un separatore suo e resta dov'è: quello è il porto di
/// una `re.split` del Python e il suo pregio è essere identico all'originale.
pub fn split_segments(command: &str) -> Vec<String> {
    #[derive(PartialEq)]
    enum Ctx {
        /// Virgolette doppie: i separatori tacciono, le sostituzioni no.
        Dq,
        /// Dentro `$(…)`: si torna a un comando a tutti gli effetti.
        Subst,
        Tick,
    }
    // Una bandierina sola non regge `"… $( … " … " … ) …"`: serve una pila.
    let mut stack: Vec<Ctx> = Vec::new();
    // Vero se, al livello di comando corrente, c'è una virgoletta doppia aperta.
    let in_string = |s: &Vec<Ctx>| {
        s.iter()
            .rev()
            .take_while(|c| **c == Ctx::Dq)
            .any(|c| *c == Ctx::Dq)
    };

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut literal = false;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            current.push(c);
            current.push(chars[i + 1]);
            i += 2;
            continue;
        }
        // Fra apici singoli non è attivo niente: si copia fino alla chiusura.
        if c == '\'' && stack.last() != Some(&Ctx::Dq) {
            current.push(c);
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                current.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                current.push('\'');
                i += 1;
            }
            continue;
        }
        if c == '$' && chars.get(i + 1) == Some(&'(') {
            if !literal {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            stack.push(Ctx::Subst);
            literal = false;
            i += 2;
            continue;
        }
        if c == ')' && stack.last() == Some(&Ctx::Subst) {
            stack.pop();
            if !literal {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            literal = in_string(&stack);
            i += 1;
            continue;
        }
        if c == '`' {
            if stack.last() == Some(&Ctx::Tick) {
                stack.pop();
            } else {
                stack.push(Ctx::Tick);
            }
            if !literal {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            literal = in_string(&stack);
            i += 1;
            continue;
        }
        if c == '"' {
            if stack.last() == Some(&Ctx::Dq) {
                stack.pop();
            } else {
                stack.push(Ctx::Dq);
            }
            current.push(c);
            i += 1;
            continue;
        }
        if stack.last() != Some(&Ctx::Dq) {
            if (c == '&' || c == '|') && chars.get(i + 1) == Some(&c) {
                if !literal {
                    out.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                literal = false;
                i += 2;
                continue;
            }
            if matches!(c, ';' | '\n' | '|') {
                if !literal {
                    out.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                literal = false;
                i += 1;
                continue;
            }
        }
        current.push(c);
        i += 1;
    }
    if !literal {
        out.push(current);
    }
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_splits_a_compound_command_into_its_commands() {
        assert_eq!(split_segments("a && b || c ; d | e\nf"), vec!["a ", " b ", " c ", " d ", " e", "f"]);
    }

    #[test]
    fn a_separator_inside_quotes_is_text_and_does_not_split() {
        assert_eq!(split_segments(r#"echo "a && b; c""#), vec![r#"echo "a && b; c""#]);
        assert_eq!(split_segments("echo 'a | b'"), vec!["echo 'a | b'"]);
    }

    /// `2>&1` va tenuto intero: spezzarlo lascerebbe un `1` che il chiamante
    /// scambia per un bersaglio, ed è così che si nomina il file sbagliato.
    #[test]
    fn a_lone_ampersand_does_not_split_a_redirection() {
        assert_eq!(split_segments("rm -rf /x 2>&1"), vec!["rm -rf /x 2>&1"]);
    }

    #[test]
    fn a_substitution_is_a_command_of_its_own() {
        assert_eq!(split_segments("X=$(rm -rf /y)"), vec!["X=", "rm -rf /y"]);
        assert_eq!(split_segments("echo `rm -rf /y`"), vec!["echo ", "rm -rf /y"]);
    }

    /// Il caso che separa questo separatore da uno ingenuo: dopo la
    /// sostituzione la coda è ancora dentro le virgolette, quindi è testo.
    #[test]
    fn the_tail_of_a_string_after_a_substitution_is_not_a_command() {
        assert_eq!(split_segments(r#"echo "$(date) rm -rf /x""#), vec![r#"echo ""#, "date"]);
    }

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
