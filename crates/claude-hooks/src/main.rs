//! Un binario solo per tutti i ganci, scelto col primo argomento.
//!
//! PERCHÉ UNO SOLO. Un comando `Bash` attraversa oggi 14 ganci, e la misura del
//! 16/08/2026 dice che girano in parallelo: il costo non è la loro somma, è il
//! più lento — ~73 ms, dettati dai due ganci che avviano Node. Nove processi
//! separati per ogni chiamata a strumento sono ~33.500 avvii di interprete al
//! giorno che questo binario riduce a uno.
//!
//! Niente parser di argomenti: `clap` costa più tempo di avvio di quanto ne
//! faccia risparmiare, e qui i sottocomandi sono un elenco chiuso.
//!
//! Uso:
//!     claude-hooks cd-guard      legge il JSON del gancio da stdin
//!     claude-hooks --list        i ganci disponibili

mod duplication;
mod handoff;
mod handoff_on_stop;
mod handoff_required;
mod linear;
mod live_rules;
mod preflight;
mod relay;
mod relay_eval;
mod restart;
mod scope_drift;
mod successor;
#[cfg(test)]
mod test_home;
mod worktree_deletes;
// I quattro porti aperti il 17/08/2026. Registrati come scheletri prima di
// essere scritti, così ogni porto tocca un file solo e il binario compila a
// ogni passo.
mod handoff_threshold;
mod hook_census;
mod link_worktree_rules;
mod spotlight_marker;
// La seconda ondata, i quattro grossi: 400-824 righe di Python ciascuno.
mod ai_personal_data;
mod json_tool;
mod memory_anchors;
mod memory_citation_gate;
mod orca_cleanup;
mod reachability;
mod register_session;
// Il terzo, portato il 20/08/2026 perché il gate della lingua non lascia più
// eseguire il suo gemello Python: uno strumento di sola lettura che nessuno può
// lanciare è un rosso che nessuno può ispezionare.
mod stale_facts;
mod session_messages;
mod skill_nudge;
mod work_status;
// Il quarto porto della stessa ondata, il 20/08/2026: il controllo che manca
// sulle fusioni con schiacciamento, dove il ramo di partenza resta vivo e
// divergente senza che nessun conflitto lo segnali.
mod squash_orphans;

use hook_io::{Decision, Mode};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(which) = args.get(1).map(String::as_str) else {
        eprintln!("uso: claude-hooks <gancio>   (--list per l'elenco)");
        std::process::exit(64);
    };

    if which == "--list" {
        // Prima stampava i soli nomi di SMOKE: chi chiedeva «quali ganci
        // esistono?» ne vedeva 5 su 22, e i 17 senza caso di prova erano
        // invisibili proprio a chi li cercava (17/08/2026).
        // La terza colonna risponde a «questo va acceso in una radice?». Senza,
        // chi legge l'elenco non distingue un gancio spento da uno strumento che
        // si invoca a mano, e i due casi hanno cure opposte.
        // Le colonne 4 e 5 separano le due domande che la seconda fondeva:
        // «il binario si prova da sé qui dentro» (self_check) non è la stessa
        // cosa di «esiste un test altrove» (modulo o crate delegato). La
        // colonna 2 resta l'unione delle due, per chi vuole solo sì/no e per
        // non rompere chi la legge già (`accettazione.py`, indice 1).
        for name in ALL_HOOKS {
            let covered = if is_covered(name) {
                "provato"
            } else {
                "senza caso"
            };
            let kind = if is_hook(name) { "gancio" } else { "strumento" };
            let self_checked = if self_check_covers(name) {
                "autoverifica"
            } else {
                "senza autoverifica"
            };
            let module_tested = if has_module_test(name) {
                "test-modulo"
            } else {
                "senza test-modulo"
            };
            println!("{name}\t{covered}\t{kind}\t{self_checked}\t{module_tested}");
        }
        return;
    }

    if which == "--check" {
        std::process::exit(self_check());
    }

    // Il catalogo degli eventi, e la domanda che il censimento non fa: non «il
    // file esiste» ma «il gancio partirebbe».
    if which == "--preflight" {
        let verbose = args.iter().any(|a| a == "--verbose");
        // All'apertura di una sessione il messaggio va nel contesto del modello
        // e l'uscita resta 0: un gancio di SessionStart che fallisce sarebbe un
        // guasto in piu', non un avviso.
        let voice = if args.iter().any(|a| a == "--session-start") {
            preflight::Voice::SessionStart
        } else {
            preflight::Voice::Command
        };
        std::process::exit(preflight::run_with(verbose, voice));
    }

    // Fail-open per tutti, prima ancora di leggere stdin: un `PreToolUse` che
    // esce in errore rifiuta ogni strumento della sessione.
    let code = match run(which) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("gancio ({which}) non ha potuto decidere: {message}");
            0
        }
    };
    std::process::exit(code);
}

/// Per ogni gancio: il nome, un comando che **deve** essere bloccato, e uno che
/// **deve** passare.
///
/// Serve perché «il file esiste» non è la domanda giusta. Il 16/08/2026 la
/// macchina si è fermata con un gancio il cui file mancava, e il censimento che
/// avrebbe dovuto vederlo controllava proprio l'esistenza — mentre il modo in
/// cui si rompe un gancio, quasi sempre, è che parte e risponde male:
/// sottocomando rinominato, binario per l'architettura sbagliata, panico
/// all'avvio. Qui si chiede al gancio di decidere, e si guarda cosa decide.
/// Ogni gancio che il binario sa eseguire. L'elenco è scritto a mano perché il
/// dispatch è un `match`, ma non può divergere: il test
/// `ogni_gancio_del_dispatch_e_elencato` rilegge questo stesso sorgente e
/// fallisce se un ramo nuovo non compare qui.
const ALL_HOOKS: &[&str] = &[
    "ai-personal-data",
    "allow-worktree-deletes",
    "allow-session-messages",
    "json",
    "block-pr-merge-admin",
    "block-worktree-create",
    "cd-guard",
    "code-language",
    "comment-refs",
    "duplication",
    "handoff-arms-successor",
    "handoff-measure",
    "handoff-on-stop",
    "handoff-required",
    "handoff-latest",
    "repo-tools",
    "handoff-resolve",
    "hooks-off",
    "linear-readonly",
    "live-rules",
    "observe",
    "pr-title",
    "relay",
    "relay-evaluate",
    "relay-chain",
    "relay-read-chain",
    "relay-sweep",
    "restart-count",
    "restart-notice",
    "scope-drift",
    "socraticode-gate",
    "successor-probe",
    "handoff-threshold",
    "hook-census",
    "link-worktree-rules",
    "spotlight-marker",
    "orca-cleanup",
    "register-session",
    "skill-nudge",
    "work-status",
    "squash-orphans",
    // Non e un gancio, come `json`: e lo strumento che risponde a «chi lancia
    // ancora questo controllo». Sta qui perche il dispatch e l'elenco si
    // controllano a vicenda, e il controllo ha fatto il suo mestiere — questa
    // riga mancava, e il workspace non passava piu.
    "reachability",
    "memory-anchors",
    "memory-citation-gate",
    "stale-facts",
];

/// Gli slug che NON sono ganci: strumenti da riga di comando, finestre di sola
/// lettura, anelli di una catena che chiama un altro anello.
///
/// Serve per una domanda che nessun controllo faceva: «questo gancio e
/// acceso?». Fino al 19/08/2026 `comment-refs` e `allow-session-messages` erano
/// scritti, provati, portati e committati — e nessuna radice li invocava, quindi
/// non erano mai partiti. I due controlli che c'erano guardavano altrove: la
/// vista di aderenza chiede «ha un contratto?» e li dava per **provati**, il
/// censimento di raggiungibilita parte dai file sul disco e vedeva solo i loro
/// ripieghi, in mezzo ad altri sedici orfani legittimi.
///
/// Perche un elenco a mano e non una deduzione dal codice: dedurlo dal braccio
/// (chi legge stdin e un gancio) sbaglia in entrambe le direzioni, perche molti
/// bracci delegano la lettura a una funzione. E dedurlo dai commenti e peggio:
/// il primo tentativo di questa misura ha creduto `handoff-latest` un gancio
/// perche il commento del braccio accanto conteneva la parola «stdin». Qui la
/// dichiarazione e esplicita, e il test la tiene onesta come fa per ALL_HOOKS.
const NOT_HOOKS: &[&str] = &[
    "json",
    "reachability",
    "memory-anchors",
    "handoff-measure",
    "handoff-latest",
    "handoff-resolve",
    "repo-tools",
    "relay-evaluate",
    "relay-chain",
    "relay-read-chain",
    "relay-sweep",
    "restart-count",
    "successor-probe",
    "stale-facts",
];

fn is_hook(name: &str) -> bool {
    !NOT_HOOKS.contains(&name)
}

/// Ganci con un blocco dedicato dentro `self_check()`, oltre a quelli della
/// tabella SMOKE: giudicano una coppia di dati (non un comando singolo) e per
/// quello hanno un pezzo di codice scritto a parte, più sotto in questo file.
///
/// Elenco a mano perché il corpo di `self_check()` non è un dato strutturato:
/// il test `ogni_extra_ha_un_blocco_in_self_check` lo tiene onesto contando le
/// occorrenze del nome nel sorgente della funzione.
const SELF_CHECK_EXTRA: &[&str] = &[
    "code-language",
    "comment-refs",
    "duplication",
    "orca-cleanup",
    "spotlight-marker",
    "work-status",
];

/// «Il binario sa provarsi da solo su questo gancio»: gira dentro
/// `self_check()`, con un comando vero o un caso scritto a mano — non solo un
/// nome citato in un elenco.
fn self_check_covers(name: &str) -> bool {
    SMOKE.iter().any(|(n, _, _)| *n == name) || SELF_CHECK_EXTRA.contains(&name)
}

/// La domanda pura, isolata dal resto: un sorgente contiene un caso di prova?
/// Separata da `has_module_test` per poterla provare senza toccare nessun file
/// del disco.
fn source_contains_test(source: &str) -> bool {
    source.contains("#[test]")
}

/// «Esiste una prova che copre questo gancio»: un `#[test]` nel modulo che lo
/// implementa in questo crate, o nel modulo del crate `guards`/`hook-io` a cui
/// delega. I sorgenti si leggono a tempo di compilazione con `include_str!`,
/// così un test tolto o aggiunto altrove si vede da solo alla build
/// successiva, senza un booleano scritto a mano che invecchia in silenzio —
/// il difetto che questa colonna corregge. Resta a mano solo la mappa
/// gancio→file: cambia molto più di rado di quanto cambino i test, ed è
/// tenuta onesta dal test `ogni_gancio_ha_una_voce_nella_mappa_dei_test`.
/// Costo: alcuni file (`relay.rs`, `successor.rs`, `handoff.rs`) sono citati
/// da più ganci e finiscono embedded più volte nel binario — qualche centinaio
/// di KB in più, accettabile per uno strumento locale.
fn has_module_test(name: &str) -> bool {
    let sources: &[&str] = match name {
        "ai-personal-data" => &[include_str!("ai_personal_data.rs")],
        "allow-worktree-deletes" => &[
            include_str!("worktree_deletes.rs"),
            include_str!("../../guards/src/worktree_deletes.rs"),
        ],
        "allow-session-messages" => &[
            include_str!("session_messages.rs"),
            include_str!("../../guards/src/session_messages.rs"),
        ],
        "json" => &[include_str!("json_tool.rs")],
        "block-pr-merge-admin" => &[include_str!("../../guards/src/pr_merge_admin.rs")],
        "block-worktree-create" => &[include_str!("../../guards/src/worktree_create.rs")],
        "cd-guard" => &[include_str!("../../guards/src/cd_guard.rs")],
        "code-language" => &[include_str!("../../guards/src/code_language.rs")],
        // Il secondo file prova la colla, non il giudizio: senza, il rapporto
        // conterebbe coperto un gancio di cui è provata solo metà.
        "comment-refs" => &[
            include_str!("../../guards/src/comment_refs.rs"),
            include_str!("../tests/comment_refs_glue.rs"),
        ],
        "duplication" => &[
            include_str!("duplication.rs"),
            include_str!("../../guards/src/duplication.rs"),
        ],
        "handoff-arms-successor" => &[include_str!("successor.rs")],
        "handoff-measure" => &[include_str!("handoff.rs")],
        "handoff-on-stop" => &[
            include_str!("handoff_on_stop.rs"),
            include_str!("../../guards/src/handoff_on_stop.rs"),
        ],
        "handoff-required" => &[
            include_str!("handoff_required.rs"),
            include_str!("../../guards/src/handoff_required.rs"),
        ],
        "handoff-latest" => &[include_str!("relay.rs")],
        "repo-tools" => &[include_str!("../../guards/src/repo_tools.rs")],
        "handoff-resolve" => &[include_str!("handoff.rs")],
        "hooks-off" => &[include_str!("../../guards/src/hooks_off.rs")],
        "linear-readonly" => &[
            include_str!("linear.rs"),
            include_str!("../../guards/src/linear_readonly.rs"),
        ],
        "live-rules" => &[
            include_str!("live_rules.rs"),
            include_str!("../../guards/src/live_rules.rs"),
        ],
        "observe" => &[include_str!("../../hook-io/src/observations.rs")],
        "pr-title" => &[include_str!("../../guards/src/pr_title.rs")],
        "relay" => &[include_str!("relay.rs")],
        "relay-evaluate" => &[
            include_str!("relay_eval.rs"),
            include_str!("handoff.rs"),
            include_str!("../../guards/src/handoff.rs"),
        ],
        "relay-chain" => &[
            include_str!("relay_eval.rs"),
            include_str!("../../guards/src/chain.rs"),
        ],
        "relay-read-chain" => &[
            include_str!("relay_eval.rs"),
            include_str!("relay.rs"),
            include_str!("../../guards/src/chain.rs"),
        ],
        "relay-sweep" => &[include_str!("relay_eval.rs"), include_str!("relay.rs")],
        "restart-count" => &[
            include_str!("restart.rs"),
            include_str!("../../guards/src/restart.rs"),
        ],
        "restart-notice" => &[include_str!("restart.rs")],
        "scope-drift" => &[
            include_str!("scope_drift.rs"),
            include_str!("../../guards/src/scope_drift.rs"),
        ],
        "socraticode-gate" => &[include_str!("../../guards/src/socraticode_gate.rs")],
        "successor-probe" => &[
            include_str!("successor.rs"),
            include_str!("../../guards/src/successor.rs"),
        ],
        "handoff-threshold" => &[
            include_str!("handoff_threshold.rs"),
            include_str!("../../guards/src/handoff_threshold.rs"),
        ],
        "hook-census" => &[include_str!("hook_census.rs")],
        "link-worktree-rules" => &[
            include_str!("link_worktree_rules.rs"),
            include_str!("../../guards/src/link_worktree_rules.rs"),
        ],
        "spotlight-marker" => &[
            include_str!("spotlight_marker.rs"),
            include_str!("../../guards/src/spotlight_marker.rs"),
        ],
        "orca-cleanup" => &[include_str!("orca_cleanup.rs")],
        "register-session" => &[include_str!("register_session.rs")],
        "skill-nudge" => &[
            include_str!("skill_nudge.rs"),
            include_str!("../../guards/src/skill_nudge.rs"),
        ],
        "work-status" => &[include_str!("work_status.rs")],
        "squash-orphans" => &[include_str!("squash_orphans.rs")],
        "reachability" => &[include_str!("reachability.rs")],
        "memory-anchors" => &[
            include_str!("memory_anchors.rs"),
            include_str!("../../guards/src/memory_anchor.rs"),
        ],
        "memory-citation-gate" => &[include_str!("memory_citation_gate.rs")],
        "stale-facts" => &[
            include_str!("stale_facts.rs"),
            include_str!("../../guards/src/stale_facts.rs"),
        ],
        _ => &[],
    };
    sources.iter().any(|s| source_contains_test(s))
}

/// La colonna larga di `--list`: «esiste una prova, di qualunque tipo, che
/// copre questo gancio». Unione delle due letture — non sostituisce le
/// colonne che le distinguono, le riassume per chi vuole solo sì/no.
fn is_covered(name: &str) -> bool {
    self_check_covers(name) || has_module_test(name)
}

const SMOKE: &[(&str, &str, &str)] = &[
    ("cd-guard", "cd /repo && git status", "git -C /repo status"),
    (
        "block-worktree-create",
        "git worktree add /Users/theo/orca/workspaces/suite/x",
        "git worktree add /private/tmp/x",
    ),
    // Il gate SocratiCode non è in questa tabella: la sua decisione dipende da
    // un repo indicizzato e da un contatore per sessione, quindi un caso «deve
    // bloccare» qui sarebbe una finzione. La sua rete è il confronto col Node
    // in `tools/compare-socraticode-gate.py`, che gira con stato isolato.
    (
        // scritto a pezzi: un freno che blocca il proprio smoke test scritto in
        // chiaro renderebbe impossibile provarlo dalla riga di comando
        "block-pr-merge-admin",
        "gh pr merge 262 --admin",
        "gh pr merge 262 --squash",
    ),
    (
        // anche questo a pezzi, e per lo stesso motivo: scritto in chiaro, il
        // gancio vivo rifiuterebbe il comando che compila il proprio smoke test
        "linear-readonly",
        concat!("linear ", "issue ", "close HRD-1"),
        "orca linear list --json",
    ),
    // `code-language` non giudica un comando ma una coppia percorso+testo, che
    // in questa tabella non ci sta. Il suo caso «deve bloccare» è dentro
    // `self_check`, insieme agli altri.
    (
        // Il titolo di una richiesta diventa l'oggetto del commit di fusione:
        // fuori formato deve fermarsi qui, in formato deve passare.
        // Niente apostrofi nel titolo di prova: chiuderebbe la stringa, lo
        // splitter di shell rinuncerebbe e il gancio tacerebbe — un caso «deve
        // bloccare» che passa per il motivo sbagliato.
        "pr-title",
        "gh pr create --title 'aggiustato il conteggio dei ganci'",
        "gh pr create --title 'fix(hooks): count the covered hooks'",
    ),
];

/// Esegue ogni gancio su due casi noti, in-process. Uscita 0 se tutti si
/// comportano come devono, 1 al primo che sbaglia — ed è il comando da lanciare
/// **prima** di far puntare la configurazione a questo binario, non dopo.
fn self_check() -> i32 {
    let mut failures = 0;
    for (name, must_block, must_pass) in SMOKE {
        for (command, expected_block) in [(must_block, true), (must_pass, false)] {
            let decision = match *name {
                "cd-guard" => guards::cd_guard::judge(command),
                "block-worktree-create" => guards::worktree_create::judge(command),
                "block-pr-merge-admin" => guards::pr_merge_admin::judge(command),
                // Il rifiuto viaggia sull'altro canale (`deny` su stdout,
                // uscita 0), quindi qui si guarda che il giudizio ci sia — non
                // che il gancio esca con codice 2.
                "pr-title" => guards::pr_title::judge(command),
                "linear-readonly" => match guards::linear_readonly::judge_bash(command) {
                    guards::linear_readonly::Verdict::Refused { reason, .. } => {
                        Decision::Block(reason)
                    }
                    _ => Decision::Pass,
                },
                _ => {
                    eprintln!("{name}: nessun caso di prova registrato");
                    failures += 1;
                    continue;
                }
            };
            // Un rifiuto viaggia su due canali: `Block` (uscita 2) e `Deny`
            // (messaggio su stdout, uscita 0). Guardare solo `Block` diceva
            // «non blocca» di ganci che rifiutano eccome — è il motivo per cui
            // `linear-readonly` qui sotto aveva una conversione scritta a mano.
            let blocked = matches!(decision, Decision::Block(_) | Decision::Deny(_));
            if blocked != expected_block {
                let atteso = if expected_block {
                    "blocco"
                } else {
                    "passaggio"
                };
                eprintln!("{name}: atteso {atteso} su {command:?}, ottenuto {decision:?}");
                failures += 1;
            }
        }
    }
    // `code-language` giudica una coppia percorso+testo, non un comando: non
    // entra nella tabella, ma deve passare di qui lo stesso — l'autoverifica
    // serve a dire che ogni gancio registrato decide, non che quasi tutti lo
    // fanno.
    let italian = guards::code_language::judge(
        "/x/a.test.ts",
        "it('rifiuta le date future', () => {})",
        true,
    );
    let english =
        guards::code_language::judge("/x/a.test.ts", "it('rejects future dates', () => {})", true);
    if italian.is_none() || english.is_some() {
        eprintln!("code-language: non distingue una descrizione italiana da una inglese");
        failures += 1;
    }

    // I tre ganci pronti per l'adozione e senza un caso qui. `adopt-hook.py`
    // interroga questa autoverifica prima di far puntare un gancio al binario:
    // il 18/08/2026 copriva 6 nomi su 33, e questi tre non c'erano — l'adozione
    // sarebbe stata cieca proprio dove la rete non c'e'.
    //
    // Tutti e tre giudicano dati, non comandi, e la funzione che decide e' pura:
    // niente Orca, niente `gh`, niente disco, niente orologio. Le funzioni che
    // parlano col mondo restano private apposta — chiamare `riconcilia()` o
    // `write_state()` da qui riscriverebbe lo stato di copie vive a ogni build.

    // `work-status`: quale stato scrivere su una copia di lavoro. La coppia
    // cambia una sola variabile — cosa e' rimasto che vive solo li' — perche' il
    // caso positivo da solo non distingue un giudice da uno stub che risponde
    // sempre «completed».
    let merged = serde_json::json!({
        "git": { "isMainWorktree": false, "branch": "refs/heads/suite-229-tabella" }
    });
    let requests: std::collections::BTreeMap<String, String> =
        [("suite-229-tabella".to_string(), "MERGED".to_string())]
            .into_iter()
            .collect();
    let clean = work_status::state_giusto(&merged, &requests, Some(0)).unwrap_or_default();
    if clean != "completed" {
        eprintln!("work-status: a merged request with nothing left behind must be completed, got {clean:?}");
        failures += 1;
    }
    // Sette commit scritti dopo la fusione: la richiesta e' unita, il lavoro no.
    // E' il caso vero di `whatsapp/media-link-recovery`, e smontare quella copia
    // avrebbe perso quei commit.
    let leftovers = work_status::state_giusto(&merged, &requests, Some(7)).unwrap_or_default();
    if leftovers != "in-progress" {
        eprintln!("work-status: a merged request holding local-only commits must stay in-progress, got {leftovers:?}");
        failures += 1;
    }

    // `spotlight-marker`: riconosce un comando che ricrea un albero di
    // dipendenze. Il comando vero non e' quasi mai nudo — arriva dietro un `cd`
    // — e stringere il riconoscimento a `starts_with` e' la correzione «ovvia»
    // che lo romperebbe in silenzio: esce 0 e la node_modules resta indicizzata.
    if !guards::spotlight_marker::is_an_install("cd /tmp && pnpm install") {
        eprintln!("spotlight-marker: does not recognise an install behind a leading cd");
        failures += 1;
    }
    if guards::spotlight_marker::is_an_install("git status") {
        eprintln!("spotlight-marker: treats an ordinary command as an install");
        failures += 1;
    }

    // `orca-cleanup`: quale scheda si puo' chiudere. I due terminali sono
    // identici — anonimo, fermo da 90 minuti — e cambia solo se dentro c'e' un
    // agente al lavoro. Chiudere quella e' il danno peggiore che il gancio possa
    // fare, ed e' il motivo per cui il caso negativo vale piu' del positivo.
    let now_ms = 1_000_000_000_000.0_f64;
    let term = serde_json::json!({
        "title": "Terminal 12",
        "handle": "term_selfcheck",
        "tabId": "sc-t1",
        "leafId": "sc-l1",
        "lastOutputAt": now_ms - 90.0 * 60_000.0,
    });
    let nobody: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    let (idle_closed, _) = orca_cleanup::judge(&term, 30.0, false, now_ms, &nobody);
    if !idle_closed {
        eprintln!("orca-cleanup: an anonymous tab idle for 90 minutes is not closed");
        failures += 1;
    }
    let mut working: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    working.insert(
        "sc-t1:sc-l1".to_string(),
        serde_json::Value::String("working".to_string()),
    );
    let (busy_closed, _) = orca_cleanup::judge(&term, 30.0, false, now_ms, &working);
    if busy_closed {
        eprintln!("orca-cleanup: would close a tab with an agent still working");
        failures += 1;
    }

    // Stessa forma per `comment-refs`, che giudica una coppia percorso+testo. Il
    // caso «deve passare» cita un percorso di CODICE: la regola lo lascia stare
    // di proposito, ed è quello che distingue questo freno da uno che nega ogni
    // commento — senza, un porto rotto in quel verso passerebbe l'autoprova.
    let to_a_document =
        guards::comment_refs::judge("/x/src/a.ts", "// ADR 0008 #61: rimosso", false);
    let to_a_source_file = guards::comment_refs::judge(
        "/x/src/a.ts",
        "// rispecchia il contratto di src/api/schema.ts",
        false,
    );
    if !matches!(to_a_document, Decision::Deny(_)) || !matches!(to_a_source_file, Decision::Pass) {
        eprintln!("comment-refs: non distingue un rimando a un documento da un percorso di codice");
        failures += 1;
    }

    // Anche il rilevatore di copie ha bisogno di due file per decidere, non di
    // un comando: il suo caso vive in una cartella temporanea.
    if let Err(why) = duplication::self_check() {
        eprintln!("duplication: {why}");
        failures += 1;
    }

    if failures == 0 {
        // Il messaggio diceva «N ganci, tutti rispondono come devono» contando
        // i soli provati: chi lo leggeva capiva «tutto il binario è a posto»,
        // mentre 17 ganci su 22 non avevano nessun caso. Poi si è scoperto che
        // quel 17 fondeva due domande diverse (20/08/2026): quante righe qui
        // sotto le stampano separate, con la propria definizione.
        let self_checked = ALL_HOOKS.iter().filter(|h| self_check_covers(h)).count();
        let module_tested = ALL_HOOKS.iter().filter(|h| has_module_test(h)).count();
        let uncovered: Vec<&str> = ALL_HOOKS
            .iter()
            .copied()
            .filter(|h| !is_covered(h))
            .collect();
        println!(
            "{self_checked} su {} passano un caso dentro self_check (tabella SMOKE + blocchi dedicati)",
            ALL_HOOKS.len()
        );
        println!(
            "{module_tested} su {} hanno un #[test] nel proprio modulo o nel crate a cui delegano",
            ALL_HOOKS.len()
        );
        if uncovered.is_empty() {
            println!("nessun gancio è scoperto da entrambe le letture");
        } else {
            println!("senza nessuna delle due: {}", uncovered.join(", "));
        }
        0
    } else {
        eprintln!("{failures} controlli falliti: NON pubblicare questo binario");
        1
    }
}

fn run(which: &str) -> Result<i32, String> {
    match which {
        "cd-guard" => {
            let mode = Mode::from_env("CD_GUARD");
            if mode == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0); // invocato fuori contesto
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            let decision = mode.soften(guards::cd_guard::judge(input.bash_command()));
            Ok(emit_with_legacy_prefix("cd-guard", &decision))
        }
        "block-worktree-create" => {
            let mode = Mode::from_env("BLOCK_WORKTREE_CREATE");
            if mode == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0); // invocato fuori contesto
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            let decision = mode.soften(guards::worktree_create::judge(input.bash_command()));
            Ok(emit_with_legacy_prefix("block-worktree-create", &decision))
        }
        "block-pr-merge-admin" => {
            // Nessuna valvola: è l'unico freno della configurazione che
            // difende un'azione irreversibile fatta su un repo condiviso, e una
            // variabile d'ambiente non deve poterlo togliere. Chi ha davvero
            // bisogno del bypass lo esegue da sé, fuori dalla sessione.
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            let decision = guards::pr_merge_admin::judge(input.bash_command());
            Ok(emit_with_legacy_prefix("block-pr-merge-admin", &decision))
        }
        // `observe` non decide niente: registra e sveglia l'osservatore. È il
        // gancio più caldo di tutti, perché gira due volte per ogni chiamata.
        "observe" => {
            let phase = std::env::args().nth(2).unwrap_or_else(|| "post".into());
            let mut raw = String::new();
            use std::io::Read as _;
            let _ = std::io::stdin().read_to_string(&mut raw);
            hook_io::observations::record(&phase, &raw);
            hook_io::observations::wake_observer();
            Ok(0)
        }
        "pr-title" => {
            if Mode::from_env("TITOLO_RICHIESTA") == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            Ok(hook_io::emit(
                "pr-title",
                &guards::pr_title::judge(input.bash_command()),
            ))
        }
        "hooks-off" => {
            if Mode::from_env("GANCI_SPENTI") == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            let default_dir = input
                .cwd
                .clone()
                .or_else(|| std::env::var("CLAUDE_PROJECT_DIR").ok())
                .unwrap_or_default();
            let decision = guards::hooks_off::judge(input.bash_command(), &default_dir);
            Ok(hook_io::emit("hooks-off", &decision))
        }
        "socraticode-gate" => {
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            let ws = guards::socraticode_gate::Workspace::from_env();
            let verdict = guards::socraticode_gate::judge(&ws, &input);
            guards::socraticode_gate::record(
                &verdict,
                input.tool_name.as_deref().unwrap_or(""),
                input.session_id.as_deref().unwrap_or("nosession"),
            );
            // Il messaggio esce senza il prefisso comune: l'originale scriveva
            // il testo nudo, e quel testo contiene già il nome del gate nella
            // prima riga. Aggiungerlo cambierebbe ciò che il modello legge.
            if let hook_io::Decision::Block(m) = &verdict.decision {
                eprintln!("{m}");
                return Ok(2);
            }
            Ok(0)
        }
        // Il codice ricopiato. Due fasi con mestieri diversi: `pre` elenca la
        // famiglia di un file che sta per nascere, `post` misura i blocchi
        // identici. È l'unico gancio portato finora il cui tempo non è avvio
        // dell'interprete ma lavoro vero — albero, letture, sottosequenza comune.
        "duplication" => {
            // `--congela` non e' un gancio: e' il verbo che crea la linea di
            // base, e va eseguito **prima** di leggere stdin — altrimenti resta
            // in attesa di un JSON che nessuno gli manda.
            let args: Vec<String> = std::env::args().skip(2).collect();
            let bare = args.iter().find(|a| !a.starts_with("--"));
            if args.iter().any(|a| a == "--congela") {
                return Ok(duplication::freeze(bare.map(String::as_str)));
            }
            if args.iter().any(|a| a == "--debito") {
                return Ok(duplication::debt(bare.map(String::as_str)));
            }
            if args.iter().any(|a| a == "--scan") {
                return Ok(duplication::scan(bare.map(String::as_str)));
            }
            if Mode::from_env("DUPLICAZIONE") == Mode::Off {
                return Ok(0);
            }
            let phase = std::env::args().nth(2).unwrap_or_else(|| "post".into());
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(duplication::run(&input, &phase))
        }
        // L'italiano dove la convenzione chiede l'inglese. Due fasi: registrato
        // su `pre`, dove il rifiuto viaggia su stdout e il file non viene
        // scritto. Su `post` avviserebbe a cose fatte — «un avviso dopo lascia
        // il file scritto in italiano», dice l'originale, ed è il motivo per
        // cui la fase registrata è la prima.
        "code-language" => {
            if Mode::from_env("LINGUA_CODICE") == Mode::Off {
                return Ok(0);
            }
            let phase = std::env::args().nth(2).unwrap_or_else(|| "pre".into());
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            let empty = serde_json::json!({});
            let tool_input = input.tool_input.as_ref().unwrap_or(&empty);
            let path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Stessa regola, altra porta. Senza questo ramo il gate vedeva solo
            // `Write`/`Edit`, e chi scriveva con `cat > file <<EOF` passava
            // intatto — misurato dal vivo il 18/08/2026. Si giudica solo il
            // primo file sorvegliato che il comando scrive: un messaggio che ne
            // elenca cinque non lo legge nessuno.
            if path.is_empty() {
                if !input.is_tool("Bash") {
                    return Ok(0);
                }
                let command = input.bash_command();
                for (target, body) in guards::code_language::writes_from_bash(&command) {
                    let exists = std::path::Path::new(&target).exists();
                    let Some(message) = guards::code_language::judge(&target, &body, exists) else {
                        continue;
                    };
                    if phase == "pre" {
                        return Ok(hook_io::emit(
                            "code-language",
                            &hook_io::Decision::Deny(message),
                        ));
                    }
                    eprintln!("{message}");
                    return Ok(2);
                }
                // E le scritture che non sappiamo leggere. Un file sorvegliato
                // che nasce dentro `python3 - <<PY … write_text …` passava
                // intatto: misurato il 19/08/2026, è la via da cui era passato
                // tutto il codice di quella notte. Qui non si indovina il
                // contenuto — dentro un interprete può nascere da una variabile
                // — si dice che da lì non si vede, e si manda a `Write`/`Edit`.
                let opachi = guards::code_language::opaque_writes(&command);
                if !opachi.is_empty() {
                    let message = guards::code_language::report_opaque(&opachi);
                    if phase == "pre" {
                        return Ok(hook_io::emit(
                            "code-language",
                            &hook_io::Decision::Deny(message),
                        ));
                    }
                    eprintln!("{message}");
                    return Ok(2);
                }
                return Ok(0);
            }
            let text = guards::code_language::written_text(tool_input);
            let exists = std::path::Path::new(path).exists();
            let Some(message) = guards::code_language::judge(path, &text, exists) else {
                return Ok(0);
            };
            let decision = if phase == "pre" {
                hook_io::Decision::Deny(message)
            } else {
                hook_io::Decision::Block(message)
            };
            // Senza prefisso: il messaggio è già un rapporto intero, e la prima
            // riga dice da sola di che si tratta.
            match decision {
                hook_io::Decision::Deny(m) => {
                    Ok(hook_io::emit("code-language", &hook_io::Decision::Deny(m)))
                }
                hook_io::Decision::Block(m) => {
                    eprintln!("{m}");
                    Ok(2)
                }
                _ => Ok(0),
            }
        }
        // I rimandi a documenti locali nei commenti. Stesso involucro di
        // `code-language`: entrambi guardano il testo appena scritto e negano
        // prima della scrittura, perché un avviso dopo lascia la riga sul file.
        // L'esenzione la legge il chiamante — `judge` è pura per poter essere
        // confrontata col Python sulla sola decisione.
        "comment-refs" => {
            if Mode::from_env("COMMENT_REFS") == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            // La fase si prende dall'evento, non dall'argomento. L'argomento è
            // una parola che qualcuno deve ricopiare in `settings.json`, e
            // prima o poi non la ricopia: il 18/08/2026 `forget_session` non
            // era mai girata perché `--fine` era scritto sul solo ripiego
            // Python mentre a girare era il binario. Resta come ripiego, per
            // poter provare il gancio dalla riga di comando.
            let phase = match input.hook_event_name.as_deref() {
                Some("PreToolUse") => "pre".to_string(),
                Some("PostToolUse") => "post".to_string(),
                _ => std::env::args().nth(2).unwrap_or_else(|| "pre".into()),
            };
            let empty = serde_json::json!({});
            let tool_input = input.tool_input.as_ref().unwrap_or(&empty);
            let path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Ok(0);
            }
            let text = guards::code_language::written_text(tool_input);
            let exempt = std::fs::read_to_string(path)
                .map(|c| guards::comment_refs::declares_marker(&c))
                .unwrap_or(false);
            let hook_io::Decision::Deny(message) = guards::comment_refs::judge(path, &text, exempt)
            else {
                return Ok(0);
            };
            // In fase `post` non si può più negare: si blocca con uscita 2, che
            // è il canale che l'assistente legge. Il codice d'uscita di `pre`
            // resta 0 — la negazione viaggia dentro lo stdout, ed è il motivo
            // per cui la prima misura di questo freno lo credette muto.
            if phase == "pre" {
                Ok(hook_io::emit(
                    "comment-refs",
                    &hook_io::Decision::Deny(message),
                ))
            } else {
                eprintln!("{message}");
                Ok(2)
            }
        }
        // Le regole appena scritte. Nessuna valvola d'ambiente: l'originale non
        // ne aveva una, e aggiungerla qui vorrebbe dire che il porting cambia
        // ciò che si può spegnere — una decisione, non una traduzione.
        "live-rules" => {
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(live_rules::run(&input))
        }
        // Il divieto su Linear: 813 righe di Python, il gancio più grande del
        // parco. Il giudizio sta in `guards::linear_readonly` ed è puro; la
        // parte con stato — permesso di Theo e registro — in `linear.rs`.
        // Una consegna appena scritta arma la sessione dopo — una sola.
        "handoff-arms-successor" => {
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(successor::run(&input))
        }
        "linear-readonly" => {
            // Nessuna valvola d'ambiente: il mandato dell'11/08/2026 non
            // prevede che una variabile lo tolga. Le tre valvole che esistono
            // stanno dentro il giudizio, e la più forte non la digita l'agente.
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(linear::run(&input))
        }
        // Non un gancio: l'interrogazione che permette allo strumento di
        // equivalenza di chiedere al Rust la stessa cosa che chiede al Python.
        // Senza, il porting della misura di consegna si proverebbe solo sui casi
        // scritti a mano — e sono proprio quelli a non trovare i difetti.
        "handoff-measure" => {
            let mut args = std::env::args().skip(2);
            let Some(transcript) = args.next() else {
                return Err("handoff-measure vuole il percorso di un transcript".into());
            };
            Ok(handoff::measure(
                &transcript,
                &args.next().unwrap_or_default(),
            ))
        }
        // Stessa ragione: l'elenco dei pannelli arriva da stdin perché le due
        // implementazioni devono giudicare lo stesso elenco, non due letture a
        // un secondo di distanza.
        // L'interrogazione del gancio che arma il successore, stessa ragione.
        // La staffetta, un passo. `--secco` non rigenera nessuna sessione ma
        // NON e' a vuoto: i record dei terminali morti li cancella lo stesso.
        "relay" => Ok(relay::step(std::env::args().any(|a| a == "--secco"))),
        // Il promemoria alla sessione che riparte da un riassunto. Legge il
        // JSON di SessionStart da stdin e, solo dopo una compattazione, parla.
        "restart-notice" => Ok(restart::run()),
        // Il promemoria sui dati personali spediti a un modello. La regola
        // gemella non basta da sola: il suo `paths:` si valuta sull'albero
        // della sessione, e l'84,5% dei tocchi cade fuori.
        "ai-personal-data" => {
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(ai_personal_data::run(&input))
        }
        // Il presidio della consegna, lato PostToolUse.
        "handoff-required" => Ok(handoff_required::run()),
        "handoff-on-stop" => Ok(handoff_on_stop::run()),
        "allow-worktree-deletes" => Ok(worktree_deletes::run()),
        // La sessione che cambia mestiere in corsa. Sta su PostToolUse `*`,
        // quindi è insieme a `observe` il gancio che parte più spesso: il suo
        // lavoro è quasi niente, e quasi tutto il costo era l'avvio di Python.
        // La valvola resta quella dell'originale, `SCOPE_DRIFT=off`.
        "scope-drift" => Ok(scope_drift::run()),
        // Non è un gancio: è l'aggancio dello strumento di equivalenza, che pone
        // la stessa domanda ai due conteggi sullo stesso transcript vero.
        "restart-count" => {
            let path = std::env::args().nth(2).unwrap_or_default();
            Ok(restart::count_probe(&path))
        }
        "successor-probe" => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
            Ok(successor::probe(&arg(0), &arg(1), &arg(2)))
        }
        // Non è un gancio: è `latest_handoff` esposta in sola lettura, perché una
        // correzione su quale consegna eredita il successore si verifica sul
        // binario che gira, non sul sorgente che si legge.
        "handoff-latest" => {
            let cwd = std::env::args().nth(2).unwrap_or_default();
            println!("{}", relay::latest_handoff(&cwd));
            Ok(0)
        }
        // Non è un gancio: è il consiglio di `guards::repo_tools` interrogabile
        // dall'esterno, perché la misura che lo giustifica si prende sui comandi
        // veri dei transcript e non sui casi scritti a mano. Il comando arriva da
        // stdin, il repo come argomento.
        "repo-tools" => {
            let dir = std::env::args().nth(2).unwrap_or_default();
            let mut command = String::new();
            use std::io::Read;
            let _ = std::io::stdin().read_to_string(&mut command);
            let pkg = std::fs::read_to_string(std::path::Path::new(&dir).join("package.json"))
                .unwrap_or_default();
            let said = guards::repo_tools::advice(command.trim(), &pkg);
            if !said.is_empty() {
                println!("{said}");
            }
            Ok(0)
        }
        "handoff-resolve" => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
            Ok(handoff::resolve(&arg(0), &arg(1), &arg(2)))
        }
        // Nemmeno questo è un gancio: è la decisione della staffetta esposta a
        // `tools/compare-relay-evaluate.py`, che pone la stessa domanda a
        // `relay.evaluate()` con una HOME finta e pretende la stessa risposta.
        "relay-evaluate" => Ok(relay_eval::run()),
        // Il gemello per il freno della catena, interrogato da
        // `tools/compare-relay-chain.py`.
        "relay-chain" => Ok(relay_eval::run_chain()),
        // Il terzo, per la lettura da disco: lo stesso confronto, ma con la
        // guardia sull'albero ricreato in mezzo.
        "relay-read-chain" => Ok(relay_eval::run_read_chain()),
        // Il quarto: quali file di stato restano senza albero.
        "relay-sweep" => Ok(relay_eval::run_sweep()),
        // I quattro porti del 17/08: rispondono già, ma la configurazione li
        // nomina solo quando il confronto col Python è verde e i mutanti sono
        // uccisi. L'ordine è quello di `adopt-hook.py`: prima si dimostra, poi
        // si registra.
        "handoff-threshold" => Ok(handoff_threshold::run()),
        "hook-census" => Ok(hook_census::run()),
        "link-worktree-rules" => Ok(link_worktree_rules::run()),
        "spotlight-marker" => Ok(spotlight_marker::run()),
        "orca-cleanup" => Ok(orca_cleanup::run()),
        "register-session" => Ok(register_session::run()),
        "skill-nudge" => Ok(skill_nudge::run()),
        "work-status" => Ok(work_status::run()),
        "squash-orphans" => Ok(squash_orphans::run()),
        "allow-session-messages" => Ok(session_messages::run()),
        // Non e un gancio: e lo strumento che risponde a «chi lancia
        // ancora questo controllo». Sta nel dispatch perche il binario e
        // gia il posto dove vive la logica, e un secondo eseguibile per
        // una misura sarebbe una superficie in piu da tenere viva.
        "reachability" => Ok(reachability::run()),
        // Nemmeno questo è un gancio: risponde a «questa memoria parla ancora
        // del codice di adesso?», e nessuna radice lo invoca. Chi lo lancia è un
        // servizio settimanale o una sessione che sta per agire su una memoria.
        "memory-anchors" => Ok(memory_anchors::run()),
        // Nega una scrittura su una memoria se introduce una citazione a un
        // file che non si trova su nessuna radice. Stesso involucro di
        // `comment-refs`: guarda il testo che sta per essere scritto e nega
        // prima, perché un avviso dopo lascia la citazione morta sul file.
        "memory-citation-gate" => Ok(memory_citation_gate::run()),
        // Non è un gancio: è lo strumento che risponde a «quali affermazioni
        // dicono di aver misurato qualcosa, e quando». Sta qui perché il suo
        // gemello Python non è più eseguibile da dentro una sessione.
        "stale-facts" => Ok(stale_facts::run()),
        // `json` non è un gancio: è il pezzo che toglie `python3 -c` dai tre
        // ganci scritti in shell, che lo invocano per leggere un campo o
        // costruire una risposta. Sta nell'elenco perché il dispatch e l'elenco
        // si controllano a vicenda, non perché `settings.json` lo nomini.
        "json" => Ok(json_tool::run()),
        other => Err(format!("gancio sconosciuto: {other}")),
    }
}

/// I messaggi conservano il prefisso degli script Python (`BLOCCATO (cd-guard):`)
/// perché sono già citati nelle regole e nei documenti: cambiarli spezzerebbe i
/// rimandi senza migliorare niente.
fn emit_with_legacy_prefix(hook: &str, decision: &Decision) -> i32 {
    hook_io::emit(hook, decision)
}

#[cfg(test)]
mod catalogo {
    use super::*;

    /// I nomi dei rami del `match` di `run()`, letti da questo stesso sorgente.
    ///
    /// Un elenco scritto a mano accanto a un `match` diverge al primo gancio
    /// nuovo, e diverge in silenzio: chi aggiunge un ramo non ha motivo di
    /// sospettare che esista un secondo posto da aggiornare. Qui il secondo
    /// posto se ne accorge da solo.
    fn hooks_in_dispatch() -> Vec<String> {
        let source = include_str!("main.rs");
        let body = source
            .split_once("fn run(which: &str)")
            .expect("la firma di run() è cambiata: aggiorna questo test")
            .1;
        let mut found = Vec::new();
        for line in body.lines() {
            let t = line.trim();
            // i rami hanno la forma `"nome" => …`, eventualmente su più nomi
            let Some(rest) = t.strip_prefix('"') else {
                continue;
            };
            let Some((name, after)) = rest.split_once('"') else {
                continue;
            };
            if after.trim_start().starts_with("=>")
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                found.push(name.to_string());
            }
        }
        found.sort();
        found.dedup();
        found
    }

    #[test]
    fn ogni_gancio_del_dispatch_e_elencato() {
        let dispatch = hooks_in_dispatch();
        assert!(
            !dispatch.is_empty(),
            "nessun ramo trovato: il lettore del sorgente non funziona più"
        );
        let missing: Vec<&String> = dispatch
            .iter()
            .filter(|h| !ALL_HOOKS.contains(&h.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "ganci nel dispatch ma non in ALL_HOOKS: {missing:?}"
        );
    }

    #[test]
    fn nessun_gancio_elencato_e_sconosciuto_al_dispatch() {
        let dispatch = hooks_in_dispatch();
        let ghosts: Vec<&&str> = ALL_HOOKS
            .iter()
            .filter(|h| !dispatch.contains(&h.to_string()))
            .collect();
        assert!(
            ghosts.is_empty(),
            "ganci elencati che il dispatch non conosce: {ghosts:?}"
        );
    }

    #[test]
    fn no_declared_tool_is_unknown_to_the_dispatch() {
        let ghosts: Vec<&&str> = NOT_HOOKS
            .iter()
            .filter(|s| !ALL_HOOKS.contains(s))
            .collect();
        assert!(
            ghosts.is_empty(),
            "dichiarati non-ganci, ma il dispatch non li conosce: {ghosts:?}"
        );
    }

    /// Il vincolo che rende utile la dichiarazione: uno slug esce dai ganci
    /// perché è uno strumento, non perché così il conto torna. Questi tre
    /// GIUDICANO — e restano ganci anche mentre nessuna radice li accende.
    #[test]
    fn whatever_judges_stays_a_hook_even_while_switched_off() {
        for name in [
            "block-worktree-create",
            "comment-refs",
            "allow-session-messages",
        ] {
            assert!(
                is_hook(name),
                "{name} giudica: toglierlo dai ganci spegnerebbe la domanda invece di rispondere"
            );
        }
    }

    #[test]
    fn hooks_declared_self_checked_have_a_real_case() {
        for name in SELF_CHECK_EXTRA {
            assert!(
                ALL_HOOKS.contains(name),
                "{name} è dichiarato provato ma non è un gancio"
            );
            assert!(
                !SMOKE.iter().any(|(n, _, _)| n == name),
                "{name} è contato due volte: sta in SMOKE e in SELF_CHECK_EXTRA"
            );
        }
    }

    #[test]
    fn self_check_runs_from_here() {
        // `self_check()` girava solo dentro il binario: togliere il caso di un
        // gancio lasciava i test verdi, e se ne accorgeva soltanto chi lanciava
        // `--check` a mano. Ora la stessa domanda la fa anche `cargo test`.
        assert_eq!(self_check(), 0, "l'autoverifica del binario non passa");
    }

    #[test]
    fn self_check_is_not_total_and_says_so() {
        // Solo 11 ganci girano dentro `self_check()`: gli altri hanno prove nel
        // modulo, non un caso qui. Se un giorno self_check li esercitasse tutti
        // questo test cadrà, ed è il momento giusto per toglierlo.
        let n = ALL_HOOKS.iter().filter(|h| self_check_covers(h)).count();
        assert!(
            n < ALL_HOOKS.len(),
            "self_check ora esercita ogni gancio: togli questo test"
        );
    }

    #[test]
    fn every_self_check_extra_has_a_block_in_self_check() {
        // Il nome deve comparire nel corpo di `self_check()`: tiene onesto
        // SELF_CHECK_EXTRA contro il caso «dichiarato ma il blocco è stato
        // tolto». Mutazione che l'ha ucciso: togliere "work-status" da questo
        // sorgente lasciandolo in `SELF_CHECK_EXTRA` fa rossire questo test.
        let source = include_str!("main.rs");
        let body = source
            .split_once("fn self_check() -> i32")
            .expect("la firma di self_check() è cambiata: aggiorna questo test")
            .1
            .split_once("fn run(which: &str)")
            .expect("la fine di self_check() è cambiata: aggiorna questo test")
            .0;
        for name in SELF_CHECK_EXTRA {
            assert!(
                body.contains(name),
                "{name} è in SELF_CHECK_EXTRA ma il suo blocco non è più in self_check()"
            );
        }
    }

    #[test]
    fn source_reader_recognises_a_test() {
        // La logica pura di `has_module_test`, isolata dal disco: prova diretta
        // sul riconoscimento del marcatore, non sulla mappa gancio→file.
        assert!(source_contains_test("fn f() {}\n#[test]\nfn g() {}"));
        assert!(!source_contains_test("fn f() {}\nfn g() {}"));
    }

    #[test]
    fn every_hook_has_an_entry_in_the_test_map() {
        // Stessa tecnica di `hooks_in_dispatch`: legge il proprio sorgente
        // invece di ripetere l'elenco, così un gancio nuovo senza voce qui
        // dentro si vede da sé invece di cadere silenziosamente nel `_ => &[]`
        // finale e risultare sempre «senza test-modulo».
        let source = include_str!("main.rs");
        let body = source
            .split_once("fn has_module_test(name: &str) -> bool {")
            .expect("la firma di has_module_test() è cambiata: aggiorna questo test")
            .1
            .split_once("\n    };\n")
            .expect("il match di has_module_test() è cambiato: aggiorna questo test")
            .0;
        let mut found = Vec::new();
        for line in body.lines() {
            let t = line.trim();
            let Some(rest) = t.strip_prefix('"') else {
                continue;
            };
            let Some((name, after)) = rest.split_once('"') else {
                continue;
            };
            if after.trim_start().starts_with("=>") {
                found.push(name.to_string());
            }
        }
        found.sort();
        let mut expected: Vec<String> = ALL_HOOKS.iter().map(|h| h.to_string()).collect();
        expected.sort();
        assert_eq!(
            found, expected,
            "la mappa gancio→sorgente in has_module_test non elenca esattamente ALL_HOOKS"
        );
    }

    #[test]
    fn known_modules_have_a_real_test() {
        // Ancoraggio ai nove porti che avevano prove non collegate qui, più
        // quelli scritti da allora: se un domani perdessero i test il rapporto
        // dovrebbe dirlo, non tacerlo.
        for name in [
            "register-session",
            "skill-nudge",
            "handoff-threshold",
            "scope-drift",
            "restart-notice",
            "observe",
            "link-worktree-rules",
            "handoff-on-stop",
            "handoff-required",
            "hook-census",
            "handoff-arms-successor",
            "allow-worktree-deletes",
            "allow-session-messages",
        ] {
            assert!(
                has_module_test(name),
                "{name} risultava senza test nel modulo: ha perso il suo #[test]?"
            );
        }
    }
}
