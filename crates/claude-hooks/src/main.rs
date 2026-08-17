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
mod successor;
mod relay_eval;

use hook_io::{Decision, Mode};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(which) = args.get(1).map(String::as_str) else {
        eprintln!("uso: claude-hooks <gancio>   (--list per l'elenco)");
        std::process::exit(64);
    };

    if which == "--list" {
        for (name, _, _) in SMOKE {
            println!("{name}");
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
const SMOKE: &[(&str, &str, &str)] = &[
    ("cd-guard", "cd /repo && git status", "git -C /repo status"),
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
                "block-pr-merge-admin" => guards::pr_merge_admin::judge(command),
                // Il rifiuto viaggia sull'altro canale (`deny` su stdout,
                // uscita 0), quindi qui si guarda che il giudizio ci sia — non
                // che il gancio esca con codice 2.
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
            let blocked = matches!(decision, Decision::Block(_));
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

    // Anche il rilevatore di copie ha bisogno di due file per decidere, non di
    // un comando: il suo caso vive in una cartella temporanea.
    if let Err(why) = duplication::self_check() {
        eprintln!("duplication: {why}");
        failures += 1;
    }

    if failures == 0 {
        println!("{} ganci, tutti rispondono come devono", SMOKE.len() + 2);
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
            if path.is_empty() {
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
        "successor-probe" => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
            Ok(successor::probe(&arg(0), &arg(1), &arg(2)))
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
        other => Err(format!("gancio sconosciuto: {other}")),
    }
}

/// I messaggi conservano il prefisso degli script Python (`BLOCCATO (cd-guard):`)
/// perché sono già citati nelle regole e nei documenti: cambiarli spezzerebbe i
/// rimandi senza migliorare niente.
fn emit_with_legacy_prefix(hook: &str, decision: &Decision) -> i32 {
    hook_io::emit(hook, decision)
}
