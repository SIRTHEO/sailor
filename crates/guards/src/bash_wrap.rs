//! Quali comandi si avvolgono nel filtro delle uscite, e come.
//!
//! PERCHÉ UNA LISTA RISTRETTA, e non tutti i comandi. Quando un gancio
//! `PreToolUse` restituisce `updatedInput`, il motore **ricontrolla i permessi
//! sull'input riscritto**: una forma che nessuna regola riconosce diventa una
//! richiesta di permesso, e la sessione si riempie di domande. Riscrivere solo
//! le famiglie che pesano — batterie, compilazioni, ricerche lunghe — tiene
//! l'esposizione dove il guadagno la ripaga. Tutto il resto passa **identico**,
//! e non passa nemmeno da qui.
//!
//! COSA HA SCELTO LA LISTA, misurato il 24/08/2026 su 42.732 chiamate a Bash di
//! sette giorni: queste famiglie portano 10,99 MB su 54,59 MB, cioè il 20,1%
//! dei byte che entrano in contesto dai comandi. Le altre stanno tutte sotto lo
//! 0,2% e non valgono l'esposizione.
//!
//! I VETI NON SONO PRUDENZA GENERICA, sono i tre modi in cui il gruppo
//! `{ …; }` non regge: un comando mandato in fondo (l'uscita non arriverebbe
//! mai al filtro), un `exit` dentro (uscirebbe dalla shell prima del filtro),
//! un documento passato per heredoc. Pipe, redirezioni e catene invece il
//! gruppo le regge, ed è la ragione per cui la lista rende: qui quasi ogni
//! comando pesante è già scritto con un `| tail` o un `> file` dentro — col
//! veto sulle pipe restava lo 0,4% del corpus invece del 20,1%.

use crate::shell::split_segments;

/// Una famiglia: la parola di comando e, se serve, i sottocomandi che contano.
/// Elenco vuoto significa «qualunque forma di questo comando».
pub struct Family {
    pub word: &'static str,
    pub subcommands: &'static [&'static str],
}

const fn family(word: &'static str, subcommands: &'static [&'static str]) -> Family {
    Family { word, subcommands }
}

/// La lista ristretta. Ogni voce porta il proprio peso misurato nel commento
/// del modulo: si allarga con una misura, non con un'intuizione.
pub const FAMILIES: &[Family] = &[
    // 4,5% dei byte: batterie e compilazioni Rust.
    family(
        "cargo",
        &["test", "build", "check", "clippy", "nextest", "mutants"],
    ),
    // 7,2%: la ricerca ricorsiva, la voce più pesante di tutte.
    family("grep", &[]),
    family("rg", &[]),
    // 4,2%: la storia di un ramo, che si legge in testa e in coda.
    family("git", &["log"]),
    // 3,7%: elenchi di file.
    family("find", &[]),
    family("ls", &[]),
    // 1,7% + 0,5%: batterie e installazioni del mondo JavaScript.
    family("vitest", &[]),
    family("jest", &[]),
    family("tsc", &[]),
    family("npx", &["vitest", "jest", "tsc"]),
    family("npm", &["test", "run", "install", "ci", "build"]),
    family("pnpm", &["test", "run", "install", "build"]),
    family("yarn", &["test", "run", "install", "build"]),
    family("bun", &["test", "run", "install", "build"]),
    // Sotto lo 0,2% ciascuna, tenute perché sono batterie: quando ci sono,
    // sono lunghe.
    family("pytest", &[]),
    family("go", &["test", "build", "vet"]),
    family("make", &[]),
];

/// Perché un comando **non** si riscrive, quando la ragione non è la famiglia.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Veto {
    /// Finisce in fondo: l'uscita non arriverebbe mai al filtro.
    Background,
    /// Contiene `exit`: uscirebbe dalla shell prima del filtro.
    Exit,
    /// Heredoc: il testo che segue non sopravvive all'avvolgimento.
    Heredoc,
    /// Già avvolto: il gancio ha già lavorato su questo comando.
    AlreadyWrapped,
}

impl Veto {
    pub fn why(&self) -> &'static str {
        match self {
            Veto::Background => "va in fondo: l'uscita non tornerebbe mai",
            Veto::Exit => "contiene `exit`: uscirebbe prima del filtro",
            Veto::Heredoc => "porta un heredoc, che l'avvolgimento spezzerebbe",
            Veto::AlreadyWrapped => "è già avvolto",
        }
    }
}

/// Il marcatore che rende riconoscibile un comando già passato di qui. Senza,
/// un secondo giro del gancio avvolgerebbe l'avvolgimento.
pub const MARKER: &str = "# filtro-uscite";

/// Il veto che ferma questo comando, se ce n'è uno.
pub fn veto_of(command: &str) -> Option<Veto> {
    if command.contains(MARKER) {
        return Some(Veto::AlreadyWrapped);
    }
    if command.contains("<<") {
        return Some(Veto::Heredoc);
    }
    if has_word(command, "exit") {
        return Some(Veto::Exit);
    }
    if ends_in_background(command) {
        return Some(Veto::Background);
    }
    None
}

/// Un `&` che manda in fondo: in coda a una riga e non parte di un `&&`.
fn ends_in_background(command: &str) -> bool {
    command.lines().any(|line| {
        let line = line.trim_end();
        line.ends_with('&') && !line.ends_with("&&")
    })
}

/// La parola c'è come parola, non dentro un'altra: `exit` sì, `exited` no.
fn has_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|w| w == word)
}

/// La famiglia riconosciuta in questo comando, se è in lista.
///
/// SI GUARDA OGNI SEGMENTO, non la prima parola: nel corpus vero il primo
/// token è `cd` nel 12% dei casi, e `cd X && cargo test` è la forma normale. La
/// segmentazione è quella dei freni (`shell::split_segments`), che sa dove
/// finisce un comando senza spezzare dentro una stringa.
pub fn family_of(command: &str) -> Option<&'static Family> {
    for segment in split_segments(command) {
        let mut words = segment
            .split_whitespace()
            .skip_while(|w| w.contains('=') || matches!(*w, "sudo" | "time" | "env"));
        let Some(word) = words.next() else { continue };
        // Un percorso assoluto conta per il suo ultimo pezzo: `/usr/bin/find`.
        let word = word.rsplit('/').next().unwrap_or(word);
        for fam in FAMILIES {
            if fam.word != word {
                continue;
            }
            if fam.subcommands.is_empty() {
                if produces_a_long_list(word, &segment) {
                    return Some(fam);
                }
                continue;
            }
            if subcommand_of(words.clone()).is_some_and(|s| fam.subcommands.contains(&s)) {
                return Some(fam);
            }
        }
    }
    None
}

/// Il sottocomando: la prima parola che non è un'opzione **né l'argomento di
/// un'opzione**.
///
/// Il salto dell'argomento non è pedanteria: `git -C /Users/theo/.claude log`
/// è la forma normale in questa casa, e senza questo il sottocomando risultava
/// `/Users/theo/.claude` — un `git log` su cinque non veniva riconosciuto.
fn subcommand_of<'a>(words: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    // Le opzioni che si portano dietro un valore staccato, per i comandi in
    // lista: `git -C <dir>`, `git -c <k=v>`, `cargo -Z <flag>`,
    // `pnpm --dir <dir>`, `npm --prefix <dir>`. La forma `--opzione=valore` non
    // ha il problema: sta tutta in una parola.
    const TAKES_A_VALUE: &[&str] = &[
        "-C",
        "-c",
        "-Z",
        "--dir",
        "--prefix",
        "--filter",
        "--cwd",
        "--manifest-path",
    ];
    let mut skip_next = false;
    for word in words {
        if skip_next {
            skip_next = false;
            continue;
        }
        if word.starts_with('-') {
            skip_next = TAKES_A_VALUE.contains(&word);
            continue;
        }
        return Some(word);
    }
    None
}

/// `grep` e `ls` valgono solo nella forma ricorsiva, l'unica che produce
/// elenchi lunghi. Senza questo, un `ls` di una cartella verrebbe avvolto per
/// niente — e ogni avvolgimento inutile è un rischio di permesso senza
/// guadagno.
fn produces_a_long_list(word: &str, segment: &str) -> bool {
    match word {
        "grep" | "ls" => segment
            .split_whitespace()
            .filter(|w| w.starts_with('-') && !w.starts_with("--"))
            .any(|w| w.contains('r') || w.contains('R')),
        _ => true,
    }
}

/// Questo comando va avvolto?
pub fn should_wrap(command: &str) -> bool {
    veto_of(command).is_none() && family_of(command).is_some()
}

/// Il comando riscritto: stessa esecuzione, uscita che passa dal filtro,
/// **stesso codice di uscita**.
///
/// Il codice di uscita è la parte che non si può sbagliare: senza l'ultima riga
/// il tool tornerebbe l'esito del filtro (sempre zero) e una batteria rossa
/// arriverebbe come verde. È lo stesso difetto per cui in questa casa non si
/// incanala `cargo test` in una pipeline.
///
/// Il file grezzo sta in `TMPDIR` — non sotto `~/.claude/state`, che dentro la
/// sandbox di una sessione non è scrivibile — e `--archive` lo passa al filtro
/// perché ne dichiari il percorso invece di farne una seconda copia.
pub fn rewrite(command: &str, binary: &str) -> String {
    format!(
        "{MARKER}\n\
         OUT=\"${{TMPDIR:-/tmp}}/claude-output-$$.txt\"\n\
         {{\n{command}\n}} > \"$OUT\" 2>&1\n\
         rc=$?\n\
         {binary} filter-output --exit-code \"$rc\" --archive \"$OUT\" < \"$OUT\"\n\
         exit \"$rc\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le famiglie della lista si riconoscono anche in fondo a una catena, che
    /// è la forma in cui arrivano quasi sempre.
    #[test]
    fn the_listed_families_are_recognised_inside_a_chain() {
        for command in [
            "cargo test -p guards",
            "cd /Users/theo/.claude/rust && cargo test -p guards 2>&1 | tail -40",
            "grep -rn 'foo' /Users/theo/orca --include='*.ts'",
            "git -C /Users/theo/.claude log --oneline -30",
            "find /Users/theo/.claude -name '*.rs'",
            "pnpm --dir /Users/theo/gyver/work -r test",
            "npx vitest run",
            "ls -R /Users/theo/orca/workspaces",
        ] {
            assert!(should_wrap(command), "doveva avvolgere: {command:?}");
        }
    }

    /// Fuori lista non si tocca niente — ed è la metà del contratto: il rischio
    /// di permesso non si presenta perché il comando non viene riscritto.
    ///
    /// MUTANTE: fatto tornare la prima famiglia a `family_of` per qualunque
    /// comando, questo va in rosso.
    #[test]
    fn anything_outside_the_list_is_left_alone() {
        for command in [
            "echo ciao",
            "cat /Users/theo/.claude/settings.json",
            "sed -n '1,40p' file.rs",
            "python3 misura.py",
            "gh pr view 479",
            "orca worktree list",
            "git status --short",
            "git commit -m 'qualcosa'",
            // Le due forme corte delle famiglie che valgono solo ricorsive.
            "grep -n 'foo' file.txt",
            "ls -la /Users/theo",
        ] {
            assert!(!should_wrap(command), "non doveva avvolgere: {command:?}");
        }
    }

    /// I tre veti, uno per uno, su comandi che **sono** in lista: senza il veto
    /// passerebbero.
    #[test]
    fn the_three_vetoes_stop_a_listed_command() {
        assert_eq!(
            veto_of("cargo test > /tmp/x.txt 2>&1 &"),
            Some(Veto::Background)
        );
        assert_eq!(veto_of("cargo test || exit 1"), Some(Veto::Exit));
        assert_eq!(
            veto_of("cargo test <<'EOF'\nqualcosa\nEOF"),
            Some(Veto::Heredoc)
        );
        for command in [
            "cargo test > /tmp/x.txt 2>&1 &",
            "cargo test || exit 1",
            "cargo test <<'EOF'\nqualcosa\nEOF",
        ] {
            assert!(!should_wrap(command), "il veto non ha fermato {command:?}");
        }
    }

    /// `&&` non è un comando in fondo, e `2>&1` nemmeno: fermarli toglierebbe
    /// le due forme più comuni di tutte.
    #[test]
    fn a_double_ampersand_and_a_redirect_are_not_background() {
        assert_eq!(veto_of("cd /repo && cargo build"), None);
        assert_eq!(veto_of("cargo test 2>&1 | tail -40"), None);
        assert!(should_wrap("cd /repo && cargo build"));
        assert!(should_wrap("cargo test 2>&1 | tail -40"));
    }

    /// Un comando già avvolto non si avvolge una seconda volta.
    #[test]
    fn an_already_wrapped_command_is_refused() {
        let once = rewrite("cargo test", "/bin/claude-hooks");
        assert_eq!(veto_of(&once), Some(Veto::AlreadyWrapped));
        assert!(!should_wrap(&once));
    }

    /// La riscrittura esegue lo stesso comando, manda l'uscita al filtro e
    /// **restituisce il codice di uscita vero**.
    #[test]
    fn the_rewrite_keeps_the_command_and_the_exit_code() {
        let out = rewrite("cargo test -p guards", "/bin/claude-hooks");
        assert!(out.contains("cargo test -p guards"), "{out}");
        assert!(out.contains("/bin/claude-hooks filter-output"), "{out}");
        assert!(out.contains("--exit-code \"$rc\""), "{out}");
        assert!(out.trim_end().ends_with("exit \"$rc\""), "{out}");
        // Il file grezzo sta in TMPDIR, non sotto ~/.claude/state: dentro la
        // sandbox di una sessione quello non è scrivibile.
        assert!(out.contains("${TMPDIR:-/tmp}"), "{out}");
        assert!(!out.contains(".claude/state"), "{out}");
    }

    /// La chiusura del gruppo comincia una riga sua: un commento in coda al
    /// comando si mangerebbe la graffa, e la shell resterebbe aperta.
    #[test]
    fn a_trailing_comment_does_not_swallow_the_closing_brace() {
        let out = rewrite("cargo test # la prova", "/bin/claude-hooks");
        assert!(out.contains("\n} >"), "{out}");
    }

    /// L'argomento di un'opzione non è il sottocomando: `git -C <dir> log` è la
    /// forma normale qui, e senza il salto non veniva riconosciuta.
    #[test]
    fn an_option_value_is_not_mistaken_for_the_subcommand() {
        assert_eq!(
            subcommand_of("-C /Users/theo/.claude log --oneline".split_whitespace()),
            Some("log")
        );
        assert_eq!(
            subcommand_of("status --short".split_whitespace()),
            Some("status")
        );
        assert_eq!(
            subcommand_of("--dir /Users/theo/gyver/work -r test".split_whitespace()),
            Some("test")
        );
        assert_eq!(subcommand_of("--version".split_whitespace()), None);
        // E il caso opposto resta fermo: `git -C <dir> status` non è in lista.
        assert!(!should_wrap("git -C /Users/theo/.claude status --short"));
    }

    /// Una parola che contiene `exit` non è un `exit`.
    #[test]
    fn a_word_containing_exit_is_not_the_exit_builtin() {
        assert!(has_word("cargo test || exit 1", "exit"));
        assert!(!has_word("grep -rn exited /repo", "exit"));
        assert_eq!(veto_of("grep -rn exited /repo"), None);
    }
}
