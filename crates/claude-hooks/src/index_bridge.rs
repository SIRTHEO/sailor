//! `claude-hooks indice` — l'indice semantico di casa, da riga di comando.
//!
//! PERCHÉ ESISTE. SocratiCode parla **solo** il protocollo MCP su standard
//! input: Claude lo usa come server, ma un motore esterno no. Il 26/08/2026
//! Codex, collegato al server, ha provato a chiamarlo e si è visto annullare
//! ogni chiamata dal meccanismo dei permessi, in modalità non interattiva.
//! Senza indice, quello stesso giorno, aveva proposto compiti che puntavano a
//! due cartelle inesistenti.
//!
//! La shell invece la sanno usare tutti. Questo comando fa da ponte: apre il
//! server, parla il protocollo al posto suo, e stampa il risultato in testo.
//! Da qui lo usano Codex, Gemini, il ciclo notturno e chiunque altro, senza
//! dipendere da un meccanismo di permessi che ognuno implementa a modo suo.
//!
//! IL PROTOCOLLO, IN TRE BATTUTE. `initialize`, la notifica
//! `notifications/initialized`, e `tools/call`. Le risposte arrivano una per
//! riga; si tiene quella con l'identificativo giusto e si ignora il resto,
//! perché il server intercala notifiche non richieste.
//!
//! FALLISCE RUMOROSO, AL CONTRARIO DEI GANCI. Un gancio nel dubbio tace,
//! perché rompere l'avvio costa più del guasto che segnala. Questo è uno
//! strumento che qualcuno ha invocato apposta: se non trova l'indice o il
//! server non risponde, lo deve dire, e uscire diverso da zero.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// I posti dove cercare il server, oltre al percorso ereditato: sotto
/// `launchd` quel percorso è minimo, e il 26/08 è già costato una notte
/// intera di compiti falliti per un motore che c'era ma non si trovava.
const SERVER_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"];

const DEFAULT_TIMEOUT_SECS: u64 = 90;

/// Quanto si aspetta la risposta prima di dire che il server non risponde.
fn timeout() -> Duration {
    Duration::from_secs(
        std::env::var("INDICE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT_SECS),
    )
}

fn server_path() -> Option<String> {
    if let Ok(explicit) = std::env::var("INDICE_SERVER_BIN") {
        if !explicit.is_empty() {
            return Some(explicit);
        }
    }
    let from_path = std::env::var("PATH").unwrap_or_default();
    let dirs = from_path
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
        .chain(SERVER_DIRS.iter().map(|d| d.to_string()));
    for dir in dirs {
        let candidate = format!("{}/socraticode", dir.trim_end_matches('/'));
        if std::fs::metadata(&candidate).map(|m| m.is_file()).unwrap_or(false) {
            return Some(candidate);
        }
    }
    None
}

/// Il testo leggibile dentro la risposta di uno strumento MCP.
///
/// Il risultato è `{"content":[{"type":"text","text":"…"}]}`: si concatenano
/// i pezzi di testo e si ignora il resto, che per questi strumenti non c'è.
/// Separata dalla parte che parla col processo, così le prove la esercitano
/// senza avviare niente.
pub fn text_from_result(value: &serde_json::Value) -> Option<String> {
    if let Some(err) = value.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("errore senza messaggio");
        return Some(format!("l'indice ha risposto con un errore: {msg}"));
    }
    let content = value.get("result")?.get("content")?.as_array()?;
    let mut out = String::new();
    for piece in content {
        if let Some(t) = piece.get("text").and_then(|t| t.as_str()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(t);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Traduce gli argomenti della riga di comando nella chiamata da mandare.
///
/// Tre forme, e nient'altro, perché sono le tre domande che un motore
/// esterno si pone davvero: dove vive una cosa, cosa fa un simbolo, cosa
/// contiene un progetto.
pub fn build_call(args: &[String]) -> Result<(String, serde_json::Value), String> {
    let (verb, rest) = args.split_first().ok_or_else(usage)?;
    match verb.as_str() {
        "cerca" => {
            let query = rest.join(" ");
            if query.trim().is_empty() {
                return Err("«cerca» vuole una domanda: indice cerca <cosa cerchi>".to_string());
            }
            Ok((
                "codebase_search".to_string(),
                serde_json::json!({
                    "query": query,
                    "projectPath": project_path(),
                    "limit": how_many(),
                }),
            ))
        }
        "simbolo" => {
            let name = rest.join(" ");
            if name.trim().is_empty() {
                return Err("«simbolo» vuole un nome: indice simbolo <nome>".to_string());
            }
            Ok((
                "codebase_symbol".to_string(),
                serde_json::json!({ "name": name, "projectPath": project_path() }),
            ))
        }
        "progetti" => Ok(("codebase_about".to_string(), serde_json::json!({}))),
        other => Err(format!("non conosco «{other}».\n{}", usage())),
    }
}

fn usage() -> String {
    "uso:\n  \
     claude-hooks indice cerca <cosa cerchi>   — dove vive, esiste già, chi lo usa\n  \
     claude-hooks indice simbolo <nome>        — cosa fa questa funzione, dove sta\n  \
     claude-hooks indice progetti              — cosa è indicizzato\n\n\
     Il progetto si sceglie con INDICE_PROGETTO (default: ~/.claude)."
        .to_string()
}

fn project_path() -> String {
    std::env::var("INDICE_PROGETTO").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/.claude")
    })
}

fn how_many() -> u64 {
    std::env::var("INDICE_QUANTI")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8)
}

/// Parla col server e restituisce il testo della risposta.
fn ask(server: &str, tool: &str, arguments: serde_json::Value) -> Result<String, String> {
    let mut child = Command::new(server)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("l'indice non è partito ({server}): {e}"))?;

    {
        let stdin = child.stdin.as_mut().ok_or("l'indice non accetta input")?;
        let hello = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "claude-hooks-indice", "version": "1"}
            }
        });
        let ready = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments}
        });
        for msg in [hello, ready, call] {
            writeln!(stdin, "{msg}")
                .map_err(|e| format!("non riesco a parlare con l'indice: {e}"))?;
        }
        stdin.flush().ok();
    }

    let stdout = child.stdout.take().ok_or("l'indice non risponde")?;
    let deadline = Instant::now() + timeout();
    let mut answer: Option<String> = None;
    for line in BufReader::new(stdout).lines() {
        if Instant::now() > deadline {
            break;
        }
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue; // il server intercala righe che non sono risposte
        };
        if value.get("id").and_then(|i| i.as_u64()) == Some(2) {
            answer = text_from_result(&value);
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();

    answer.ok_or_else(|| {
        format!(
            "l'indice non ha risposto entro {}s, o ha risposto qualcosa che non so leggere.\n\
             Se il progetto non è indicizzato, indicizzalo prima: è la causa più frequente.",
            timeout().as_secs()
        )
    })
}

/// Le risposte con cui il server dice «non ho niente da darti».
///
/// Arrivano come testo normale dentro una risposta riuscita, quindi senza
/// questo riconoscimento il ponte le stampava e usciva **zero**: chi le
/// riceveva da uno script le contava come successo. Il 26/08/2026 è successo
/// esattamente questo — il freno su `grep` mandava all'indice, l'indice
/// rispondeva «No symbol graph found» e nessuno sapeva perché.
fn looks_like_nothing_came_back(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("no symbol graph found")
        || t.contains("not indexed")
        || t.contains("no results found")
        || t.contains("docker not available")
}

/// Vero se il socket di Docker si lascia aprire da qui.
///
/// Serve a distinguere due guasti che il server racconta con la stessa frase:
/// l'indice davvero mancante (si costruisce) e l'indice irraggiungibile perché
/// il perimetro di questo processo non arriva al socket (si riesegue fuori).
/// Senza la distinzione, il consiglio è sbagliato in metà dei casi.
fn docker_socket_is_reachable() -> bool {
    for socket in [
        "/var/run/docker.sock",
        // Docker Desktop su macOS mette il socket dell'utente qui.
        &format!(
            "{}/.docker/run/docker.sock",
            std::env::var("HOME").unwrap_or_default()
        ),
    ] {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return true;
        }
    }
    false
}

/// Il consiglio da dare quando il server non ha restituito niente.
///
/// Separato dal resto perché è la parte che qualcuno legge davvero, e le prove
/// la esercitano senza avviare nessun processo.
pub fn advice_when_empty(docker_reachable: bool) -> String {
    if docker_reachable {
        "L'indice risponde ma su questo progetto non ha dati: va costruito.\n  \
         codebase_index (poi codebase_graph_build) sul percorso del repo,\n  \
         oppure INDICE_PROGETTO=<repo> per interrogarne un altro."
            .to_string()
    } else {
        "Il socket di Docker non si apre da questo processo, e senza Docker\n\
         l'indice non ha né ricerca né grafo — la risposta «non trovato» qui\n\
         NON vuol dire che il progetto sia da indicizzare.\n  \
         Da una sessione Claude: usa gli strumenti MCP, che non passano da qui.\n  \
         Da uno script nel perimetro ristretto: rieseguilo fuori dal perimetro.\n  \
         Se Docker è davvero fermo: avvia Docker Desktop."
            .to_string()
    }
}

pub fn run(args: &[String]) -> i32 {
    let (tool, arguments) = match build_call(args) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            return 2;
        }
    };
    let Some(server) = server_path() else {
        eprintln!(
            "l'indice non è installato, o non è in nessuno dei posti guardati.\n\
             Cercato «socraticode» nel percorso e in: {}",
            SERVER_DIRS.join(", ")
        );
        return 2;
    };
    match ask(&server, &tool, arguments) {
        Ok(text) => {
            println!("{text}");
            if looks_like_nothing_came_back(&text) {
                eprintln!("\n{}", advice_when_empty(docker_socket_is_reachable()));
                return 3;
            }
            0
        }
        Err(msg) => {
            eprintln!("{msg}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Le frasi con cui il server dice «niente», raccolte dal vivo il
    /// 26/08/2026 — non inventate: sono quelle che il ponte ha stampato
    /// uscendo zero.
    #[test]
    fn the_server_saying_nothing_is_recognised_as_a_failure() {
        for said in [
            "No symbol graph found. Run codebase_graph_build (or codebase_index) first.",
            "Infrastructure: ❌ Docker not available",
            "Project not indexed yet.",
            "No results found for this query.",
        ] {
            assert!(
                looks_like_nothing_came_back(said),
                "«{said}» è un guasto travestito da risposta, va riconosciuto"
            );
        }
    }

    /// Il contro-caso, senza il quale la prova sopra si accontenterebbe di un
    /// riconoscimento che dice sempre sì — e ogni risposta buona diventerebbe
    /// un guasto.
    #[test]
    fn a_real_answer_is_not_mistaken_for_a_failure() {
        for said in [
            "Symbol: looks_like_a_name_lookup (function)\nDefined: crates/guards/src/socraticode_gate.rs:1012",
            "Infrastructure: ✅ All services running (Docker, Qdrant, ollama embeddings)",
            "3 results:\n  crates/notte/src/main.rs:88",
        ] {
            assert!(
                !looks_like_nothing_came_back(said),
                "«{said}» è una risposta buona, non va scambiata per un guasto"
            );
        }
    }

    /// I due consigli devono essere DIVERSI, e ciascuno dire la sua causa:
    /// è tutto il valore della distinzione. Se dicessero la stessa cosa,
    /// metà di chi legge andrebbe a indicizzare un progetto già indicizzato.
    #[test]
    fn the_advice_names_the_cause_it_actually_found() {
        let with_docker = advice_when_empty(true);
        let without_docker = advice_when_empty(false);
        assert_ne!(with_docker, without_docker);
        assert!(with_docker.contains("codebase_index"));
        assert!(!with_docker.contains("socket"));
        assert!(without_docker.contains("socket"));
        assert!(
            without_docker.contains("NON vuol dire"),
            "il consiglio senza Docker deve smentire la lettura sbagliata, non solo suggerire"
        );
    }

    #[test]
    fn a_search_carries_the_whole_question() {
        let (tool, a) =
            build_call(&args(&["cerca", "dove", "si", "decide", "la", "notte"])).unwrap();
        assert_eq!(tool, "codebase_search");
        assert_eq!(a["query"], "dove si decide la notte");
    }

    #[test]
    fn a_search_without_a_question_is_refused() {
        let err = build_call(&args(&["cerca"])).expect_err("senza domanda non si cerca");
        assert!(err.contains("vuole una domanda"), "{err}");
    }

    #[test]
    fn a_symbol_lookup_uses_the_right_tool() {
        let (tool, a) = build_call(&args(&["simbolo", "decide"])).unwrap();
        assert_eq!(tool, "codebase_symbol");
        assert_eq!(a["name"], "decide");
    }

    #[test]
    fn an_unknown_verb_says_what_exists() {
        let err = build_call(&args(&["vola"])).expect_err("verbo inventato");
        assert!(err.contains("non conosco «vola»"), "{err}");
        assert!(err.contains("cerca"), "deve elencare cosa esiste: {err}");
    }

    #[test]
    fn no_arguments_prints_the_usage() {
        let err = build_call(&[]).expect_err("senza argomenti");
        assert!(err.contains("uso:"), "{err}");
    }

    /// Il caso normale: il testo si estrae e i pezzi si concatenano.
    #[test]
    fn the_text_of_an_answer_is_extracted() {
        let v = serde_json::json!({
            "result": {"content": [
                {"type": "text", "text": "prima"},
                {"type": "text", "text": "seconda"}
            ]}
        });
        assert_eq!(text_from_result(&v).unwrap(), "prima\nseconda");
    }

    /// Un errore del server non deve sembrare una risposta vuota: chi legge
    /// deve poter distinguere «non ho trovato niente» da «l'indice è rotto».
    #[test]
    fn a_server_error_is_reported_as_such() {
        let v = serde_json::json!({"error": {"code": -32602, "message": "progetto sconosciuto"}});
        let t = text_from_result(&v).unwrap();
        assert!(t.contains("errore"), "{t}");
        assert!(t.contains("progetto sconosciuto"), "{t}");
    }

    /// Una risposta senza testo non è una risposta: chi chiama deve poterlo
    /// dire, invece di stampare una riga vuota e uscire zero.
    #[test]
    fn an_empty_answer_is_not_an_answer() {
        assert!(text_from_result(&serde_json::json!({"result": {"content": []}})).is_none());
        assert!(text_from_result(&serde_json::json!({"result": {}})).is_none());
    }
}
