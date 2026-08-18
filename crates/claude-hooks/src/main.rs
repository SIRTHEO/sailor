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
mod linear;
mod live_rules;
mod preflight;
mod handoff_required;
mod handoff_on_stop;
mod worktree_deletes;
mod relay;
mod restart;
mod scope_drift;
#[cfg(test)]
mod test_home;
mod successor;
mod relay_eval;
// I quattro porti aperti il 17/08/2026. Registrati come scheletri prima di
// essere scritti, così ogni porto tocca un file solo e il binario compila a
// ogni passo.
mod handoff_threshold;
mod hook_census;
mod link_worktree_rules;
mod spotlight_marker;
// La seconda ondata, i quattro grossi: 400-824 righe di Python ciascuno.
mod orca_cleanup;
mod register_session;
mod skill_nudge;
mod work_status;
mod json_tool;
mod session_messages;

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
        for name in ALL_HOOKS {
            let covered = if is_covered(name) { "provato" } else { "senza caso" };
            println!("{name}\t{covered}");
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
];

/// Ganci con un caso di prova in `self_check`, oltre a quelli di SMOKE: qui
/// stanno quelli che non giudicano un comando e hanno un controllo scritto a
/// parte, più sotto.
const COVERED_APART: &[&str] = &[
    // I due del 18/08: nessun caso in `self_check`, ma un confronto
    // d'equivalenza col Python a parte — 28 casi e 3 mutanti uccisi per il
    // primo, 126 casi e 2 mutanti per il secondo.
    "allow-session-messages",
    "json",
    "code-language",
    "comment-refs",
    "duplication",
    "orca-cleanup",
    "spotlight-marker",
    "work-status",
];

fn is_covered(name: &str) -> bool {
    SMOKE.iter().any(|(n, _, _)| *n == name) || COVERED_APART.contains(&name)
}

const SMOKE: &[(&str, &str, &str)] = &[
    ("cd-guard", "cd /repo && git status", "git -C /repo status"),
    (
        "block-worktree-create",
        "git worktree add /home/someone/orca/workspaces/suite/x",
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
    let english = guards::code_language::judge(
        "/x/a.test.ts",
        "it('rejects future dates', () => {})",
        true,
    );
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
    // E' il caso vero di `a-client/media-link-recovery`, e smontare quella copia
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
        // mentre 17 ganci su 22 non avevano nessun caso. `adopt-hook.py` si
        // fida di questa riga prima di far puntare la configurazione al
        // binario, quindi la riga deve dire la copertura, non il totale dei
        // provati (17/08/2026).
        let covered: Vec<&str> = ALL_HOOKS.iter().copied().filter(|h| is_covered(h)).collect();
        let uncovered: Vec<&str> = ALL_HOOKS
            .iter()
            .copied()
            .filter(|h| !is_covered(h))
            .collect();
        println!(
            "{} ganci su {} controllati, e rispondono come devono",
            covered.len(),
            ALL_HOOKS.len()
        );
        if !uncovered.is_empty() {
            println!("senza caso di prova: {}", uncovered.join(", "));
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
                    let Some(message) = guards::code_language::judge(&target, &body, exists)
                    else {
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
                hook_io::Decision::Deny(m) => Ok(hook_io::emit("code-language", &hook_io::Decision::Deny(m))),
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
            let hook_io::Decision::Deny(message) =
                guards::comment_refs::judge(path, &text, exempt)
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
            Ok(handoff::measure(&transcript, &args.next().unwrap_or_default()))
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
        "allow-session-messages" => Ok(session_messages::run()),
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
    fn i_ganci_dichiarati_provati_hanno_davvero_un_caso() {
        for name in COVERED_APART {
            assert!(
                ALL_HOOKS.contains(name),
                "{name} è dichiarato provato ma non è un gancio"
            );
            assert!(
                !SMOKE.iter().any(|(n, _, _)| n == name),
                "{name} è contato due volte: sta in SMOKE e in COVERED_APART"
            );
        }
    }

    #[test]
    fn l_autoverifica_passa_da_qui() {
        // `self_check()` girava solo dentro il binario: togliere il caso di un
        // gancio lasciava i test verdi, e se ne accorgeva soltanto chi lanciava
        // `--check` a mano. Ora la stessa domanda la fa anche `cargo test`.
        assert_eq!(self_check(), 0, "l'autoverifica del binario non passa");
    }

    #[test]
    fn la_copertura_non_e_totale_e_lo_si_dice() {
        // Se un giorno saranno tutti coperti questo test cadrà, ed è il momento
        // giusto per toglierlo: finché non succede, difende la riga onesta.
        let covered = ALL_HOOKS.iter().filter(|h| is_covered(h)).count();
        assert!(
            covered < ALL_HOOKS.len(),
            "copertura totale raggiunta: togli questo test e il ramo «senza caso di prova»"
        );
    }
}
