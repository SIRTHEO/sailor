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
    (
        // scritto a pezzi: un freno che blocca il proprio smoke test scritto in
        // chiaro renderebbe impossibile provarlo dalla riga di comando
        "block-pr-merge-admin",
        "gh pr merge 262 --admin",
        "gh pr merge 262 --squash",
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
        other => Err(format!("gancio sconosciuto: {other}")),
    }
}

/// I messaggi conservano il prefisso degli script Python (`BLOCCATO (cd-guard):`)
/// perché sono già citati nelle regole e nei documenti: cambiarli spezzerebbe i
/// rimandi senza migliorare niente.
fn emit_with_legacy_prefix(hook: &str, decision: &Decision) -> i32 {
    hook_io::emit(hook, decision)
}
