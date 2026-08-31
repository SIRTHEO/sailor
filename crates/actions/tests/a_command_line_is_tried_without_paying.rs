//! La prova a secco di una riga di comando: si monta, si esegue **senza dare
//! la domanda**, e si giudica da ciò che il motore dice.
//!
//! **PERCHÉ ESISTE.** Il guasto 1 e il guasto 27 sono lo stesso difetto a due
//! anni di distanza l'uno dall'altro nella stessa settimana: una riga composta
//! da pezzi giusti separatamente, sbagliata insieme, e mai eseguita prima di
//! finire in un flusso che si paga. La cura scritta accanto al guasto 1 —
//! «eseguire davvero ogni riga di comando prima che finisca in un flusso» — era
//! rimasta scoperta perché eseguirla sembrava voler dire spendere. Non vuol
//! dire: senza la domanda non si chiama nessun fornitore, e il parsing degli
//! argomenti è lo stesso.
//!
//! **PERCHÉ I MOTORI QUI SONO FINTI.** Una prova che avvia `claude`, `codex` e
//! `agy` veri dipende da cosa è installato su chi la esegue e da come sta messa
//! la quota quel giorno: non potrebbe venire diversa per la ragione che
//! dichiara. Qui i quattro casi si costruiscono, e i testi che i finti stampano
//! sono quelli veri, misurati il 31/08/2026 su questa macchina.

use actions::{
    judge_dry_run, probe_dry_run, AskRecipe, DryProbe, DryRun, ProbeVerdict, PromptVia,
    RealDryProbe,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "actions-{name}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("la cartella di prova si crea");
        Sandbox { root }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Un eseguibile finto che stampa quello che gli diciamo di stampare, con il
/// codice d'uscita che gli diciamo di avere.
fn fake_binary(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("scrittura dell'eseguibile finto");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("bit di esecuzione");
    path
}

/// La ricetta di `agy` così com'è spedita, che è quella su cui il guasto 27 è
/// vissuto: la domanda va in coda, e `--print` deve restarle attaccato.
fn agy_recipe(refuses: &[&str]) -> AskRecipe {
    AskRecipe {
        args: vec!["--mode".to_owned(), "plan".to_owned()],
        prompt: PromptVia::LastArg,
        args_before_prompt: vec!["--print".to_owned()],
        unusable_when: Vec::new(),
        refuses_without_prompt: refuses.iter().map(|s| (*s).to_owned()).collect(),
        usage: None,
    }
}

// ── i quattro finti motori, quattro verdetti ────────────────────────────

/// **IL CASO SANO.** Il motore dice «mancava solo la domanda»: la riga è
/// montata bene, e nessuno ha pagato niente per saperlo.
#[test]
fn an_engine_that_only_misses_the_prompt_is_declared_sound() {
    let sandbox = Sandbox::new("sana");
    let bin = fake_binary(
        &sandbox.root,
        "agy-sano",
        "echo 'flag needs an argument: -print' >&2\nexit 2",
    );
    let verdict = probe_dry_run(
        &RealDryProbe,
        &bin.to_string_lossy(),
        &agy_recipe(&["flag needs an argument: -print"]),
    );
    assert_eq!(verdict, ProbeVerdict::Sound, "{verdict:?}");
}

/// **IL CASO DEL GUASTO 27.** Lo stesso motore, lo stesso codice d'uscita, e
/// una riga che si lamenta di tutt'altro: il testo è la diagnosi, e arriva a
/// chi legge parola per parola.
#[test]
fn an_engine_that_complains_about_something_else_is_declared_broken_with_its_own_words() {
    let sandbox = Sandbox::new("rotta");
    let bin = fake_binary(
        &sandbox.root,
        "agy-rotto",
        "echo 'Error: --print took \"--output-format\" as its prompt, so the intended prompt was left as an argument and ignored.' >&2\nexit 2",
    );
    let verdict = probe_dry_run(
        &RealDryProbe,
        &bin.to_string_lossy(),
        &agy_recipe(&["flag needs an argument: -print"]),
    );
    match verdict {
        ProbeVerdict::Broken { said } => {
            // LE PAROLE DEL MOTORE SONO IL PRODOTTO. Sul guasto 27 la frase
            // diceva quale bandiera aveva mangiato quale argomento: nessuna
            // classificazione nostra avrebbe potuto dire altrettanto, e un
            // «rotta» senza di esse manda a indovinare.
            assert!(said.contains("--print took"), "{said}");
            assert!(said.contains("--output-format"), "{said}");
        }
        other => panic!("doveva essere rotta, è {other:?}"),
    }
}

/// **UN MOTORE ESAURITO NON È UN MOTORE ROTTO**, e i due si somigliano solo se
/// li si legge nell'ordine sbagliato: la riga di `claude` è sana, è la quota
/// che è finita.
#[test]
fn an_exhausted_engine_is_not_called_broken() {
    let sandbox = Sandbox::new("esaurita");
    let bin = fake_binary(
        &sandbox.root,
        "claude-esaurito",
        "echo \"You've hit your weekly limit · resets 7am\" >&2\nexit 1",
    );
    let recipe = AskRecipe {
        args: vec!["-p".to_owned()],
        prompt: PromptVia::Stdin,
        args_before_prompt: Vec::new(),
        unusable_when: vec!["weekly limit".to_owned()],
        refuses_without_prompt: vec!["input must be provided either through stdin".to_owned()],
        usage: None,
    };
    let verdict = probe_dry_run(&RealDryProbe, &bin.to_string_lossy(), &recipe);
    match verdict {
        ProbeVerdict::CannotWork { said } => assert!(said.contains("weekly limit"), "{said}"),
        other => panic!("doveva essere «non può lavorare adesso», è {other:?}"),
    }
}

/// **CHI TACE NON È SANO.** Un descrittore senza `refuses_without_prompt` non
/// dice che la riga va bene: dice che nessuno l'ha guardata. Chiamarlo sano
/// sarebbe il modo più silenzioso di smettere di controllare — la stessa
/// differenza che il blocco `capabilities` tiene fra «non ce l'ha» e «nessuno
/// ha guardato».
#[test]
fn an_engine_whose_descriptor_says_nothing_is_not_declared_sound() {
    let sandbox = Sandbox::new("taciuta");
    let bin = fake_binary(
        &sandbox.root,
        "agy-muto",
        "echo 'flag needs an argument: -print' >&2\nexit 2",
    );
    let verdict = probe_dry_run(&RealDryProbe, &bin.to_string_lossy(), &agy_recipe(&[]));
    assert_eq!(
        verdict,
        ProbeVerdict::NotDeclared,
        "il motore ha pure rifiutato bene, ma nessuno aveva dichiarato come lo dice"
    );
}

// ── la riga che parte davvero ───────────────────────────────────────────

/// Chi guarda cosa è stato eseguito, senza eseguire niente.
#[derive(Default)]
struct RecordingProbe {
    seen: Mutex<Vec<(String, Vec<String>, Option<Vec<u8>>)>>,
}

impl DryProbe for RecordingProbe {
    fn run(&self, bin: &str, args: &[String], stdin: Option<Vec<u8>>) -> DryRun {
        self.seen
            .lock()
            .expect("il registro della sonda finta")
            .push((bin.to_owned(), args.to_vec(), stdin));
        DryRun::Answered {
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// **LA RIGA PROVATA È LA RIGA VERA, MENO LA DOMANDA.** Se la sonda montasse
/// una riga sua, proverebbe qualcosa che nessuna corsa eseguirà mai — cioè
/// darebbe un verde su un oggetto diverso da quello che si paga.
#[test]
fn the_line_that_is_tried_is_the_real_one_without_the_prompt() {
    let probe = RecordingProbe::default();
    let _ = probe_dry_run(&probe, "agy", &agy_recipe(&["flag needs an argument"]));

    let seen = probe.seen.lock().expect("il registro");
    let (bin, args, stdin) = &seen[0];
    assert_eq!(bin, "agy");
    assert_eq!(args, &["--mode", "plan", "--print"]);
    // A chi vuole la domanda in coda non si dà nessun ingresso: dargli un
    // ingresso vuoto sarebbe innocuo, ma dargliene uno aperto lo farebbe
    // aspettare — e la prova a secco diventerebbe un modo per appendere il
    // controllo, su una macchina dove `timeout` non esiste.
    assert!(stdin.is_none(), "la domanda andava in coda, non sull'ingresso");
}

/// E a chi la vuole sull'ingresso si dà un ingresso **vuoto e chiuso**, che è
/// ciò che fa `< /dev/null` — l'unica forma in cui `claude` e `codex`
/// rispondono invece di aspettare.
#[test]
fn an_engine_that_reads_the_prompt_from_stdin_gets_an_empty_closed_one() {
    let probe = RecordingProbe::default();
    let recipe = AskRecipe {
        args: vec!["-p".to_owned()],
        prompt: PromptVia::Stdin,
        args_before_prompt: Vec::new(),
        unusable_when: Vec::new(),
        refuses_without_prompt: vec!["input must be provided".to_owned()],
        usage: None,
    };
    let _ = probe_dry_run(&probe, "claude", &recipe);

    let seen = probe.seen.lock().expect("il registro");
    assert_eq!(seen[0].2, Some(Vec::new()));
}

/// Un motore che non risponde entro il tetto non è né sano né rotto: non si sa,
/// e il motivo viaggia col verdetto perché un processo che non parte e uno che
/// non risponde si riparano in modi diversi.
#[test]
fn an_engine_that_never_answers_is_neither_sound_nor_broken() {
    let verdict = probe_dry_run(
        &RealDryProbe,
        "/questo/percorso/non/esiste/da/nessuna/parte",
        &agy_recipe(&["flag needs an argument"]),
    );
    match verdict {
        ProbeVerdict::TimedOut { why } => assert!(!why.is_empty(), "il motivo non è vuoto"),
        other => panic!("doveva essere «nessuna risposta», è {other:?}"),
    }
}

// ── l'ordine di lettura, che è la parte che si sbaglia ──────────────────

/// **`unusable_when` SI LEGGE PRIMA.** Un motore esaurito si lamenta della
/// quota, non della riga; letto nell'ordine opposto verrebbe dichiarato rotto,
/// e chi legge andrebbe a correggere un descrittore sano mentre bastava
/// aspettare.
///
/// Qui l'uscita contiene **tutte e due** le cose: è il solo caso in cui
/// l'ordine si può osservare, e quindi il solo che rende rossa un'inversione.
#[test]
fn the_exhausted_reading_comes_first_when_the_output_says_both() {
    let recipe = AskRecipe {
        args: vec!["-p".to_owned()],
        prompt: PromptVia::Stdin,
        args_before_prompt: Vec::new(),
        unusable_when: vec!["weekly limit".to_owned()],
        refuses_without_prompt: vec!["input must be provided".to_owned()],
        usage: None,
    };
    let verdict = judge_dry_run(
        &recipe,
        "",
        "Input must be provided either through stdin\nYou've hit your weekly limit",
    );
    match verdict {
        ProbeVerdict::CannotWork { .. } => {}
        other => panic!(
            "l'ordine di lettura è invertito: un motore esaurito è diventato {other:?}"
        ),
    }
}

/// Il confronto ignora maiuscole e minuscole, perché nessun fornitore promette
/// di non cambiarle.
#[test]
fn the_words_are_matched_whatever_case_the_engine_shouts_them_in() {
    let verdict = judge_dry_run(
        &agy_recipe(&["flag needs an argument: -print"]),
        "",
        "FLAG NEEDS AN ARGUMENT: -PRINT",
    );
    assert_eq!(verdict, ProbeVerdict::Sound);
}

/// Un frammento vuoto non rende sano niente: combacerebbe con qualunque
/// uscita, e un descrittore scritto male trasformerebbe ogni riga rotta in una
/// riga sana — che è il difetto peggiore possibile per questo controllo.
#[test]
fn an_empty_fragment_declares_nothing() {
    let verdict = judge_dry_run(&agy_recipe(&["", "   "]), "", "un errore qualunque");
    assert_eq!(verdict, ProbeVerdict::NotDeclared);
}

/// **IL VERDETTO NON GUARDA IL CODICE D'USCITA, E NON PUÒ.** `judge_dry_run`
/// non lo riceve nemmeno: i due `agy` misurati il 31/08/2026 escono tutti e due
/// **2** — il rifiuto sano e la riga malformata del guasto 27 — quindi una
/// sonda che giudicasse dall'esito li vedrebbe identici. Qui i due testi veri
/// stanno affiancati, ed è l'unica prova che li mette a confronto.
#[test]
fn two_failures_with_the_same_exit_code_get_two_different_verdicts() {
    let recipe = agy_recipe(&["flag needs an argument: -print"]);
    let sound = judge_dry_run(&recipe, "", "flag needs an argument: -print");
    let broken = judge_dry_run(
        &recipe,
        "",
        "Error: --print took \"--output-format\" as its prompt, so the intended \
         prompt was left as an argument and ignored.",
    );
    assert_eq!(sound, ProbeVerdict::Sound);
    assert!(
        matches!(broken, ProbeVerdict::Broken { .. }),
        "stesso codice d'uscita, verdetto diverso: {broken:?}"
    );
    assert_ne!(sound, broken);
}

/// Un motore che esce **zero** senza domanda non è sano per questo: `agy
/// --mode nonsense-value --not-a-real-flag --help` esce 0 il 31/08/2026 su
/// questa macchina, e stampa la guida. È la ragione per cui `--help` non serve
/// a provare una riga, e per cui questo verdetto guarda solo il testo.
#[test]
fn an_engine_that_exits_zero_with_a_help_screen_is_not_sound() {
    let sandbox = Sandbox::new("guida");
    let bin = fake_binary(&sandbox.root, "agy-guida", "echo 'Usage of agy:'\nexit 0");
    let verdict = probe_dry_run(
        &RealDryProbe,
        &bin.to_string_lossy(),
        &agy_recipe(&["flag needs an argument: -print"]),
    );
    match verdict {
        ProbeVerdict::Broken { said } => assert!(said.contains("Usage of agy"), "{said}"),
        other => panic!("uscire zero senza domanda non è una riga sana: {other:?}"),
    }
}
