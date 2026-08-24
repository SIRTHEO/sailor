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
///
/// DENTRO UN CICLO LA TESTA NON È DOVE SEMBRA. L'a capo separa due comandi
/// quanto un `;`, e `do`, `then`, `else`, `{` aprono un blocco senza essere
/// comandi: senza contarli, in un ciclo `for … do` che porta
/// `FRENO_DISTRUTTIVO=off rm -rf "$d"` sulla riga successiva,
/// il segmento comincia per `do` e la valvola resta invisibile. Il 24/08/2026 è
/// costato un diniego su una cancellazione lecita, provato in tutte e due le
/// forme che il messaggio del freno suggerisce: una valvola che non apre insegna
/// che l'unica strada è truccare il comando finché il freno non lo riconosce.
pub fn valve_in_front(command: &str, valve: &str) -> bool {
    command
        .split(|c| c == ';' || c == '&' || c == '|' || c == '\n')
        .any(|segment| {
            segment
                .split_whitespace()
                .skip_while(|w| BLOCK_OPENERS.contains(w))
                .take_while(|w| w.contains('=') && !w.starts_with('-'))
                .any(|w| w.eq_ignore_ascii_case(valve))
        })
}

/// Le parole che aprono un blocco: stanno dove starebbe un comando, ma il
/// comando viene dopo. Non allargano dove la valvola vale — resta in testa, fra
/// le assegnazioni — spostano solo il punto in cui la testa comincia.
const BLOCK_OPENERS: &[&str] = &["do", "then", "else", "{", "("];

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

// ── Il verbo di un segmento ─────────────────────────────────────────────────
//
// Estratto qui e non nel gancio che lo usa perché due letture divergono al
// primo caso nuovo: un divieto (`linear_readonly`) e una concessione
// (`read_only_allow`) guardano lo stesso «chi comanda in questo pezzo di
// shell», in direzioni opposte. `sed` imparato in un elenco e non nell'altro
// era già successo prima che questa funzione esistesse.

/// I costrutti di shell: mai un verbo. Un segmento che comincia con uno di
/// questi non ha niente da eseguire, e non si guarda cosa segue — per un
/// ciclo o un condizionale non sappiamo dire se il resto del segmento è
/// davvero il corpo o solo un pezzo tagliato a metà da un separatore che il
/// costrutto non intendeva come tale.
const CONSTRUCTS: &[&str] = &[
    "cd", "for", "while", "until", "if", "do", "done", "then", "fi", "case", "set", "export",
];

/// Cosa si è capito del verbo di un segmento.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Verb {
    /// Il segmento era solo costrutti e assegnazioni: non c'è niente da
    /// eseguire.
    None,
    /// Il verbo vero, col sottocomando quando ce n'è uno riconoscibile e il
    /// resto degli argomenti per chi deve giudicarli.
    Known {
        head: String,
        sub: Option<String>,
        rest: Vec<String>,
    },
    /// Non si sa dire: una sostituzione, una virgoletta aperta, o un verbo
    /// che è una variabile invece di un nome.
    Unknown,
}

fn looks_like_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Salta assegnazioni in testa (`VAR=x`) e l'`env` che ne introduce altre
/// (`env VAR=x cmd`), fino al primo token che è davvero un comando o un
/// costrutto. `env` nudo, senza niente dopo da eseguire, resta lui il verbo.
fn skip_prefixes(words: &[String]) -> usize {
    let mut i = 0;
    loop {
        match words.get(i) {
            Some(w) if looks_like_assignment(w) => i += 1,
            Some(w) if w == "env" => {
                let mut j = i + 1;
                while words.get(j).map(|w| looks_like_assignment(w)).unwrap_or(false) {
                    j += 1;
                }
                if j < words.len() {
                    i = j;
                } else {
                    return i;
                }
            }
            _ => return i,
        }
    }
}

/// Le opzioni globali di git che precedono il sottocomando: saltarle è
/// necessario perché `git -C <repo> <sottocomando>` è il 96% del git scritto
/// in questa casa, e senza saltarle `<repo>` verrebbe letto come sottocomando.
fn skip_git_globals(words: &[String], mut i: usize) -> usize {
    while i < words.len() {
        let takes_value = matches!(words[i].as_str(), "-C" | "-c" | "--git-dir" | "--work-tree");
        if takes_value {
            // Il flag c'è ma l'argomento no: ci si ferma qui invece di
            // saltare oltre la fine del vettore. Col profilo di rilascio a
            // `panic = "abort"` un indice fuori intervallo qui non è un
            // errore che si propaga — è il binario che si ferma, il
            // contrario del «fallisce chiuso» che questo modulo promette.
            if i + 1 >= words.len() {
                return i + 1;
            }
            i += 2;
            continue;
        }
        match words[i].as_str() {
            "--no-pager" => i += 1,
            w if w.starts_with("--git-dir=") || w.starts_with("--work-tree=") => i += 1,
            _ => break,
        }
    }
    i
}

/// Il verbo vero di un segmento — quello che deciderà se legge o scrive.
///
/// Non giudica: chi chiama decide cosa fare di `Known`, e deve trattare
/// `Unknown` diversamente da `None`. Un divieto e una concessione leggono lo
/// stesso «non lo so» in direzioni opposte — l'uno nega, l'altro tace — e
/// sbagliano entrambi se lo confondono con «non c'è verbo».
pub fn segment_verb(segment: &str) -> Verb {
    if segment.contains("$(")
        || segment.contains('`')
        || segment.contains("<(")
        || segment.contains(">(")
    {
        return Verb::Unknown;
    }
    let Some(words) = split_words(segment) else {
        return Verb::Unknown;
    };
    let i = skip_prefixes(&words);
    let Some(head) = words.get(i) else {
        return Verb::None;
    };
    if CONSTRUCTS.contains(&head.as_str()) {
        return Verb::None;
    }
    if head.starts_with('$') {
        return Verb::Unknown;
    }
    // Clamp difensivo: `skip_git_globals` non dovrebbe più restituire un
    // indice fuori dal vettore, ma un secondo cancello qui non costa niente
    // ed è quello che tiene fuori il panico se la funzione cambia domani.
    let after_head = if head == "git" {
        skip_git_globals(&words, i + 1)
    } else {
        i + 1
    }
    .min(words.len());
    let sub = words
        .get(after_head)
        .filter(|w| !w.starts_with('-'))
        .cloned();
    let rest_start = if sub.is_some() { after_head + 1 } else { after_head };
    Verb::Known {
        head: head.clone(),
        sub,
        rest: words[rest_start..].to_vec(),
    }
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

    /// Il caso vero del 24/08/2026: la valvola scritta dove il messaggio del
    /// freno dice di scriverla, dentro un ciclo, e il freno che nega lo stesso.
    #[test]
    fn a_valve_inside_a_loop_opens_like_one_in_front_of_a_bare_command() {
        let v = "FRENO_DISTRUTTIVO=off";
        // L'a capo separa due comandi quanto un `;`.
        assert!(valve_in_front(
            "for n in A B; do\n  FRENO_DISTRUTTIVO=off rm -rf \"$d\"\ndone",
            v
        ));
        // E il blocco aperto sulla stessa riga: la testa viene dopo `do`.
        assert!(valve_in_front("for n in A B; do FRENO_DISTRUTTIVO=off rm -rf \"$d\"; done", v));
        assert!(valve_in_front("if [ -d x ]; then FRENO_DISTRUTTIVO=off rm -rf x; fi", v));
    }

    /// Il verso che conta di più: aver spostato la testa non deve aver reso la
    /// valvola riconoscibile dove è solo testo, o il freno si apre a chi la
    /// nomina in un messaggio.
    #[test]
    fn the_wider_head_does_not_make_a_mentioned_valve_count() {
        let v = "FRENO_DISTRUTTIVO=off";
        assert!(!valve_in_front("rm -rf /x --nota FRENO_DISTRUTTIVO=off", v));
        assert!(!valve_in_front("for n in A B; do rm -rf \"$d\" FRENO_DISTRUTTIVO=off; done", v));
        assert!(!valve_in_front("echo \"scrivi FRENO_DISTRUTTIVO=off davanti\"", v));
        // `do` sposta la testa, non la cancella: dopo il comando è troppo tardi.
        assert!(!valve_in_front("do rm -rf /x FRENO_DISTRUTTIVO=off", v));
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

    #[test]
    fn git_dash_c_before_the_subcommand_is_the_case_that_matters_most() {
        assert_eq!(
            segment_verb("git -C /x log --oneline"),
            Verb::Known {
                head: "git".to_string(),
                sub: Some("log".to_string()),
                rest: vec!["--oneline".to_string()],
            }
        );
        assert_eq!(
            segment_verb("git -C /x -c user.name=y log"),
            Verb::Known {
                head: "git".to_string(),
                sub: Some("log".to_string()),
                rest: vec![],
            }
        );
        assert_eq!(
            segment_verb("git --no-pager -C /x diff"),
            Verb::Known {
                head: "git".to_string(),
                sub: Some("diff".to_string()),
                rest: vec![],
            }
        );
        assert_eq!(
            segment_verb(r#"git -C "$R" show"#),
            Verb::Known {
                head: "git".to_string(),
                sub: Some("show".to_string()),
                rest: vec![],
            }
        );
    }

    /// Il difetto 7 del secondo verdetto: `-C`/`-c`/`--git-dir`/`--work-tree`
    /// come ultimo token facevano affettare il vettore oltre la sua fine.
    /// Col profilo di rilascio a `panic = "abort"` questo abortiva il
    /// binario, non falliva chiuso come il commento prometteva.
    #[test]
    fn a_global_flag_missing_its_argument_does_not_panic() {
        for cmd in ["git -C", "git -c", "git --git-dir", "git --work-tree"] {
            assert_eq!(
                segment_verb(cmd),
                Verb::Known {
                    head: "git".to_string(),
                    sub: None,
                    rest: vec![],
                },
                "cmd: {cmd}"
            );
        }
    }

    #[test]
    fn cd_is_a_construct_with_no_verb_after_it() {
        assert_eq!(segment_verb("cd /x"), Verb::None);
    }

    /// Il caso che ha insegnato la distinzione fra i tre esiti: un ciclo non
    /// si capisce dai suoi pezzi, e il pezzo dopo `do` non va promosso a
    /// verbo solo perché somiglia a uno.
    #[test]
    fn a_loop_construct_swallows_its_own_line_without_looking_past_it() {
        assert_eq!(segment_verb("for f in *"), Verb::None);
        assert_eq!(segment_verb("do cat $f"), Verb::None);
        assert_eq!(segment_verb("done"), Verb::None);
    }

    #[test]
    fn env_unwraps_to_the_real_verb() {
        assert_eq!(
            segment_verb("env FOO=1 cat f.rs"),
            Verb::Known {
                head: "cat".to_string(),
                sub: Some("f.rs".to_string()),
                rest: vec![],
            }
        );
        assert_eq!(
            segment_verb("env"),
            Verb::Known {
                head: "env".to_string(),
                sub: None,
                rest: vec![],
            }
        );
    }

    #[test]
    fn a_variable_or_a_substitution_as_the_verb_is_unknown_not_absent() {
        assert_eq!(segment_verb("$CMD arg"), Verb::Unknown);
        assert_eq!(segment_verb("echo $(rm -rf /x)"), Verb::Unknown);
        assert_eq!(segment_verb(r#"git commit -m "aperta"#), Verb::Unknown);
    }
}
