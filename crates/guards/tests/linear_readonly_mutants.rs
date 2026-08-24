//! I casi che la misura dei mutanti del 24/08/2026 ha trovato senza guardiano.
//!
//! PERCHÉ STANNO QUI E NON ACCANTO AL CODICE. `linear_readonly.rs` è dentro il
//! nucleo che quel freno protegge: da una sessione di Claude Code non ci si
//! scrive, ed è il punto del divieto, non un incidente. Le prove nuove vivono
//! quindi in un binario a sé, come quelle del registro del gate.
//!
//! E c'è una seconda ragione, che vale da sola: qui `HOME` si sostituisce. I
//! tre file che il gancio esenta dalla lettura del proprio contenuto **sulla
//! macchina vera non esistono più** — il gemello Python è stato cancellato — e
//! senza una casa finta l'esenzione non la esercitava nessuna prova: il
//! giudizio si fermava prima, sul file che non c'è. Sostituire `HOME` dentro la
//! batteria di `guards` la sposterebbe anche per i casi che girano in
//! parallelo; un file di prova è un binario a sé, e qui dentro non gira
//! nient'altro.

use guards::linear_readonly::{
    declared, expand_variables, is_protected_file, judge_bash, segments, touches_protected_within,
    Verdict,
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// La finestra attorno al nome del file dentro il testo di uno script. Non è
/// pubblica: qui si ripete perché due delle prove la misurano.
const WINDOW: usize = 120;

/// Un comando che sposta una scheda: il contenuto vietato che gli script di
/// queste prove si portano dentro.
const A_WRITE: &str = "orca linear status set HRD-1 --status Done\n";

/// La casa finta di questo binario. Ci vivono i file esentati, che altrove non
/// esistono, e i copioni che le prove fanno eseguire.
fn home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = hook_io::testing::test_dir("linear-readonly-mutants");
        std::fs::create_dir_all(dir.join(".claude/skills/hooks")).unwrap();
        std::env::set_var("HOME", &dir);
        dir
    })
}

/// Ogni strada d'ingresso passa di qui, così `HOME` è già sostituita qualunque
/// sia il caso che parte per primo.
fn verdict_of(command: &str) -> Verdict {
    home();
    judge_bash(command)
}

/// Rifiutato in uno dei due modi: `Sealed` è un rifiuto quanto `Refused`.
fn refused(command: &str) -> bool {
    matches!(
        verdict_of(command),
        Verdict::Refused { .. } | Verdict::Sealed { .. }
    )
}

fn why(command: &str) -> String {
    match verdict_of(command) {
        Verdict::Refused { reason, .. } | Verdict::Sealed { reason } => reason,
        _ => String::new(),
    }
}

/// Uno script vero su disco: il giudizio sugli script eseguiti **apre il
/// file**, quindi non si può provare con una stringa.
fn script(name: &str, body: &str) -> String {
    let path = home().join(name);
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().into_owned()
}

/// Il contenuto di uno script eseguito è il comando vero, in ogni forma con cui
/// lo si lancia. È il difetto già catalogato — «le scritture da script saltano
/// i ganci» — e il ramo intero poteva tornare `None`, cioè spegnersi, senza che
/// una sola prova diventasse rossa.
#[test]
fn what_an_executed_script_contains_is_judged_as_if_it_were_typed() {
    let path = script("chiudi.sh", A_WRITE);
    assert!(refused(&format!("bash {path}")), "dietro un interprete");
    assert!(refused(&format!("bash \"{path}\"")), "fra virgolette");
    assert!(refused(&path), "eseguito per percorso");
    assert!(
        refused(&format!("ssh server {path}")),
        "dietro un esecutore"
    );
    // E leggerlo resta libero: un divieto che impedisce di studiare ciò che
    // sorveglia viene disattivato.
    assert!(!refused(&format!("cat {path}")));
}

/// Il tetto sulla dimensione è dichiarato, quindi si prova dov'è: a 200.000
/// byte esatti lo script si legge ancora, subito sopra no.
#[test]
fn the_size_ceiling_on_a_script_is_where_it_says_it_is() {
    let mut at_ceiling = String::from(A_WRITE);
    let over_ceiling = format!("{at_ceiling}{}", "#".repeat(200_001 - A_WRITE.len()));
    at_ceiling.push_str(&"#".repeat(200_000 - A_WRITE.len()));
    assert_eq!(at_ceiling.len(), 200_000);

    let inside = script("al-tetto.sh", &at_ceiling);
    let outside = script("oltre-il-tetto.sh", &over_ceiling);
    assert!(refused(&format!("bash {inside}")), "200.000 byte esatti");
    assert!(!refused(&format!("bash {outside}")), "oltre il tetto");
}

/// L'esenzione sta sul file che il nucleo protegge, non su ciò a cui qualcuno
/// l'ha fatto puntare.
///
/// Le prove del gancio contengono per necessità gli esempi vietati, quindi il
/// loro contenuto non si legge come sospetto; ma un percorso esentato che sia
/// un **collegamento** non vale, o `ln -s ostile.py <percorso esentato>`
/// sposterebbe la deroga dove punta il collegamento.
#[test]
fn the_exemption_follows_the_file_and_not_the_link() {
    let hooks = home().join(".claude/skills/hooks");
    let exempt = hooks.join("prova-linear-sola-lettura.py");
    std::fs::write(&exempt, A_WRITE).unwrap();

    assert!(!refused(&format!("python3 {}", exempt.display())));
    // Lo stesso file per un'altra strada resta lo stesso file: è così che lo
    // strumento esentato si lancia davvero, dalla cartella sopra la sua.
    let detour = format!("{}/../hooks/prova-linear-sola-lettura.py", hooks.display());
    assert!(!refused(&format!("python3 {detour}")), "{detour}");

    // Un collegamento no: il file dietro il nome non è quello che il nucleo
    // protegge.
    let hostile = script("ostile.py", A_WRITE);
    let link = hooks.join("linear-sola-lettura.py");
    std::os::unix::fs::symlink(&hostile, &link).unwrap();
    let spelled = format!("{}/../hooks/linear-sola-lettura.py", hooks.display());
    assert!(refused(&format!("python3 {spelled}")), "{spelled}");
}

/// La finestra attorno al nome del file esiste per non negare uno script che
/// *nomina* il gancio e mille righe più in là scrive un file diverso. Il suo
/// taglio cade su un confine di carattere: un accento occupa due byte, e
/// tagliarlo a metà fa cadere il gancio — che, essendo fail-open, lascerebbe
/// passare tutto in silenzio.
#[test]
fn the_window_stops_at_a_character_boundary_and_at_its_own_width() {
    // Il segno di scrittura è dentro la finestra: si nega.
    let near = format!("{}; cp x ~/.claude/settings.json", "à".repeat(100));
    let cut = near.find("settings.json").unwrap() - WINDOW;
    assert!(!near.is_char_boundary(cut), "il caso non è quello che dice");
    assert!(touches_protected_within(&near, WINDOW).is_some());

    // Lo stesso segno fuori dalla finestra non parla di questo file.
    let far = format!("cp a b\n{}settings.json", "à".repeat(100));
    assert!(touches_protected_within(&far, WINDOW).is_none());
}

/// Una virgoletta scappata non chiude la stringa. Se la chiudesse, il `;` che
/// segue diventerebbe un separatore e il testo citato verrebbe letto come un
/// comando da eseguire — e cercare il testo di un divieto dev'essere possibile,
/// o il divieto lo si aggira per non litigarci.
#[test]
fn an_escaped_quote_does_not_close_the_string() {
    // Una sola virgoletta scappata: due si annullano — lo stato torna dov'era
    // e la prova non direbbe niente.
    let line = r#"echo "dice \"; linear issue close HRD-1""#;
    assert_eq!(segments(line), vec![line.to_string()]);
    assert!(!refused(line));
    // E una barra rovesciata in fondo non fa cadere il gancio.
    assert!(!refused("echo \"finisce con una barra \\"));
}

/// Un prefisso di lettura non autorizza il verbo che lo segue: `issue` è una
/// lettura da sola, `issue close` no. Nessuna prova passava di qui, perché la
/// CLI di Linear non ha `issue` come lettura nuda e quella di Orca sì.
#[test]
fn a_read_prefix_does_not_authorise_the_verb_behind_it() {
    assert!(refused("orca linear issue close HRD-1"));
    assert!(refused("orca linear issue --team=HRD close HRD-1"));
    assert!(!refused("orca linear issue HRD-123"));
}

/// La valvola vale **davanti** al comando: la prima parola che non è
/// un'assegnazione né un involucro chiude la questione, e ciò che viene dopo
/// non autorizza più niente.
#[test]
fn a_valve_behind_the_command_is_just_an_argument() {
    assert!(!declared("cp OK_UTENTE=1 ~/.claude/settings.json"));
    assert!(!declared("echo ciao OK_UTENTE=1"));
    assert!(matches!(
        verdict_of("cp OK_UTENTE=1 ~/.claude/settings.json"),
        Verdict::Refused { .. }
    ));
}

/// Il testo che *contiene* un comando si giudica sulla vicinanza fra `linear` e
/// il verbo, e su nient'altro: una CLI che non è Linear non è affare di questo
/// mandato.
#[test]
fn a_write_that_never_names_linear_is_not_this_mandate() {
    assert!(!refused(r#"bash -c "gh issue close 12""#));
    assert!(!refused(
        r#"codex exec "chiudi la scheda e poi fai il commit""#
    ));
    // Ma un'opzione in mezzo non allontana il verbo abbastanza.
    assert!(refused(r#"bash -c "linear --team HRD close HRD-1""#));
}

/// Chi consegna il comando a un esecutore generico risponde di ciò che gli
/// consegna: non è un interprete, ed è una strada a sé.
#[test]
fn the_write_handed_to_a_generic_executor_is_seen_too() {
    assert!(refused(r#"ssh server "linear issue close HRD-1""#));
    assert!(refused("watch -n 60 linear issue close HRD-1"));
}

/// Un interprete che riceve un file resta un interprete: il comando vero è lo
/// script, ma la riga l'ha ricevuta lui, e il rifiuto lo dice.
#[test]
fn an_interpreter_that_hands_over_a_script_is_still_named_as_one() {
    assert_eq!(
        why("python3 scripts/inoltra.py linear issue close HRD-1"),
        "linear … close eseguito da un interprete"
    );
}

/// Un interprete senza niente dietro non fa cadere il gancio: se cade, il
/// comando dopo passa senza giudizio.
#[test]
fn an_interpreter_with_nothing_behind_it_does_not_bring_the_hook_down() {
    for line in ["python3 --version", "node -v", "bash"] {
        assert!(!refused(line), "{line}");
    }
}

/// Il lanciatore di pacchetti nasconde la CLI dietro **due** parole, non una.
#[test]
fn a_runner_does_not_hide_the_cli_behind_it() {
    assert!(refused("pnpx exec linear-cli issue close HRD-1"));
    assert!(refused("npx @linear-cli issue close HRD-1"));
}

/// Il riconoscimento per percorso guarda l'eseguibile, non la riga: uno script
/// `.js` qualunque non diventa la CLI di Linear per il fatto di essere un `.js`.
#[test]
fn a_javascript_that_is_not_the_linear_cli_is_left_alone() {
    assert!(!refused("node scripts/rilascio.js produzione"));
    assert!(refused(
        "node /x/@dabble/linear-cli/bin/cli.js issue close HRD-1"
    ));
}

/// L'automazione del browser si nega **sull'interfaccia di Linear**: le stesse
/// parole su un altro sito sono lavoro qualunque.
#[test]
fn browser_automation_is_only_refused_on_the_linear_interface() {
    assert!(refused("orca goto --page x --url https://linear.app/team"));
    assert!(!refused(
        "orca goto --page x --url https://github.com/gyver/suite"
    ));
    assert!(!refused("orca click --page x --selector .salva"));
}

/// Un alias o una funzione che nasconde il nome della CLI non si può risolvere:
/// si smette di fidarsi della riga. Ma una variabile che con Linear non c'entra,
/// e una lettura dietro una variabile, restano lavoro normale.
#[test]
fn an_alias_that_hides_the_cli_is_refused_and_an_unrelated_one_is_not() {
    assert!(refused(r#"alias lc="linear issue close HRD-1""#));
    assert!(refused(r#"alias lc="orca linear issue""#));
    assert!(refused("function lc { orca linear issue; }"));
    assert!(!refused("PROG=git; $PROG status"));
    assert!(!refused("ORCA=orca; $ORCA linear issue HRD-123"));
}

/// La mutazione GraphQL contro l'API. L'host si compone a pezzi perché il
/// pavimento di sicurezza dell'ambiente uccide qualunque comando che se lo trovi
/// scritto per intero, prove comprese.
#[test]
fn a_graphql_mutation_against_the_api_is_refused_but_a_word_alone_is_not() {
    let host = format!("api.{}.app", "linear");
    assert!(refused(&format!(
        "curl -s https://{host}/graphql -d '{{\"query\":\"mutation issueUpdate\"}}'"
    )));
    assert!(!refused("git commit -m 'prova della mutation GraphQL'"));
    assert!(!refused(
        "curl -s https://api.github.com/graphql -d 'issueUpdate'"
    ));
}

/// Il valore di una variabile si legge senza le virgolette: con quelle dentro,
/// la redirezione perde il bersaglio e la scrittura sul nucleo torna invisibile.
#[test]
fn a_quoted_path_in_a_variable_is_resolved_too() {
    assert_eq!(
        expand_variables(r#"F="~/.claude/settings.json"; cat "$F""#),
        r#"F="~/.claude/settings.json"; cat "~/.claude/settings.json""#
    );
    assert!(refused(
        r#"F="~/.claude/skills/hooks/linear-sola-lettura.py"; echo "" > "$F""#
    ));
}

/// I due divieti non si confondono: il nucleo e la configurazione hanno motivi
/// diversi perché hanno valvole diverse, e chi stampa il rifiuto sceglie
/// l'omelia leggendo il motivo. Prima stampava sempre quella sulle schede, e
/// chi toccava `settings.json` si sentiva spiegare che non deve muoverle.
#[test]
fn the_core_and_the_configuration_are_named_apart() {
    let core = why("rm ~/.claude/skills/hooks/linear-sola-lettura.py");
    let config = why("echo x > ~/.claude/settings.json");
    assert!(
        core.contains("il gancio stesso o il suo registro"),
        "{core}"
    );
    assert!(config.contains("la configurazione dei ganci"), "{config}");
    assert!(is_protected_file(&core));
    assert!(is_protected_file(&config));
    assert!(!is_protected_file(&why("linear issue close HRD-1")));
}
