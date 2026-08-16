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

mod linear;

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
    if failures == 0 {
        println!("{} ganci, tutti rispondono come devono", SMOKE.len());
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
        // Il divieto su Linear: 813 righe di Python, il gancio più grande del
        // parco. Il giudizio sta in `guards::linear_readonly` ed è puro; la
        // parte con stato — permesso di Theo e registro — in `linear.rs`.
        "linear-readonly" => {
            // Nessuna valvola d'ambiente: il mandato dell'11/08/2026 non
            // prevede che una variabile lo tolga. Le tre valvole che esistono
            // stanno dentro il giudizio, e la più forte non la digita l'agente.
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(linear::run(&input))
        }
        other => Err(format!("gancio sconosciuto: {other}")),
    }
}

/// I messaggi conservano il prefisso degli script Python (`BLOCCATO (cd-guard):`)
/// perché sono già citati nelle regole e nei documenti: cambiarli spezzerebbe i
/// rimandi senza migliorare niente.
fn emit_with_legacy_prefix(hook: &str, decision: &Decision) -> i32 {
    hook_io::emit(hook, decision)
}
