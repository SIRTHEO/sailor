//! Dichiara una sessione ferma su una richiesta di permesso, leggibile da fuori.
//!
//! Nasce dalla segnalazione del 21/08/2026
//! (`state/plancia/segnalazioni/2026-08-21-una-sessione-bloccata-da-un-permesso-sembra-aperta.md`):
//! un pannello aperto da un'automazione, senza presidio, che si ferma su
//! `PermissionRequest` non lo dice a nessuno — il registro di chi l'ha aperto
//! scrive comunque «avviata».
//!
//! IL DISEGNO IN TRE MOSSE. `declare()` gira sull'evento `PermissionRequest`
//! (fase `permission` di `observe`, da cablare in `settings.json`: qui non lo
//! decide nessuna sessione) e scrive un marcatore. `clear()` gira sulle fasi
//! `pre`/`post` di `observe`, GIÀ ACCESE per ogni strumento: una sessione che fa
//! qualunque cosa dopo — lo stesso strumento concesso, o un altro tentativo
//! dopo un diniego — dimostra di non essere ferma, e il marcatore sparisce.
//! `run_report()` legge da fuori con un comando solo.
//!
//! FERMA NON È LENTA. Il marcatore da solo non basta: una richiesta appena
//! arrivata e una rimasta senza risposta per un'ora hanno la stessa forma sul
//! disco. `decide_stalled` aggiunge una soglia di tempo e il filtro sulle
//! sessioni ancora vive, così chi è solo lento — o già finito — non conta.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Soglia sotto la quale un marcatore non conta come «ferma»: una richiesta
/// concessa e in esecuzione lo cancella molto prima, quasi sempre.
const DEFAULT_GRACE_SECONDS: i64 = 45;

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/Users/theo".into()))
}

fn markers_dir() -> PathBuf {
    home().join(".claude").join("state").join("permessi-in-sospeso")
}

fn live_dir() -> PathBuf {
    home().join(".claude").join("state").join("sessioni-vive")
}

/// Gli stessi primi otto caratteri con cui `register_session` nomina
/// `sessioni-vive/<sess>.json`: la stessa sessione deve trovarsi con la stessa
/// chiave nei due registri, o il filtro «è ancora viva» manca sempre.
fn short(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

fn marker_path(session_id: &str) -> PathBuf {
    markers_dir().join(format!("{}.json", short(session_id)))
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Ciò che serve a scrivere il marcatore, estratto dal payload grezzo di
/// `PermissionRequest`. `None` se manca la sessione: senza, non c'è una chiave
/// su cui dichiarare niente.
struct PendingRequest {
    session_id: String,
    tool_name: String,
    summary: String,
    cwd: String,
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn summarize(tool_name: &str, input: Option<&Value>) -> String {
    if tool_name == "Bash" {
        if let Some(cmd) = input.and_then(|v| v.get("command")).and_then(|v| v.as_str()) {
            return truncate(cmd, 300);
        }
    }
    input.map(|v| truncate(&v.to_string(), 300)).unwrap_or_default()
}

fn extract_request(raw: &str) -> Option<PendingRequest> {
    let data = serde_json::from_str::<Value>(raw).ok()?;
    let session_id = data.get("session_id").and_then(|v| v.as_str())?;
    if session_id.trim().is_empty() {
        return None;
    }
    let tool_name = data
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let cwd = data.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let summary = summarize(&tool_name, data.get("tool_input"));
    Some(PendingRequest { session_id: session_id.to_string(), tool_name, summary, cwd })
}

/// La colla pura del lato scrittura: dal payload al corpo del marcatore, senza
/// toccare il disco. Isolata per poterla provare senza una `HOME` finta.
fn marker_body(req: &PendingRequest, requested_at_iso: &str, requested_at_epoch: i64) -> Value {
    serde_json::json!({
        "session_id": req.session_id,
        "tool_name": req.tool_name,
        "summary": req.summary,
        "cwd": req.cwd,
        "requested_at": requested_at_iso,
        "requested_at_epoch": requested_at_epoch,
    })
}

/// Scrive il marcatore. Mai un errore verso il chiamante: un `PermissionRequest`
/// che non riesce a dichiararsi non deve far cadere l'evento che sblocca la
/// sessione.
pub fn declare(raw: &str) {
    let Some(req) = extract_request(raw) else { return };
    let dir = markers_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let body = marker_body(&req, &hook_io::journal::now_iso8601_seconds(), now_epoch());
    let _ = fs::write(marker_path(&req.session_id), body.to_string());
}

/// Toglie il marcatore della sessione: chiamata da `pre` e `post` di `observe`,
/// già accese su ogni strumento. Qualunque attività dopo una richiesta di
/// permesso — lo stesso strumento concesso, o un tentativo diverso dopo un
/// diniego — prova che la sessione non è ferma.
pub fn clear(raw: &str) {
    let Ok(data) = serde_json::from_str::<Value>(raw) else { return };
    let Some(session_id) = data.get("session_id").and_then(|v| v.as_str()) else { return };
    let _ = fs::remove_file(marker_path(session_id));
}

/// Una sessione dichiarata ferma: il marcatore, filtrato dalla soglia e dal
/// registro delle sessioni vive.
#[derive(Debug, Clone, PartialEq)]
pub struct StalledSession {
    pub session_id: String,
    pub tool_name: String,
    pub summary: String,
    pub cwd: String,
    pub requested_at: String,
    pub seconds_pending: i64,
}

/// Il giudizio puro: quali marcatori contano come «ferma», adesso. Riceve i
/// corpi già letti dal disco e l'insieme delle sessioni ancora vive (i primi
/// otto caratteri, come li scrive `register_session`), non tocca niente da sé.
///
/// Un marcatore la cui sessione non è più nel registro delle vive non conta:
/// o la sessione è finita da sola, o si è chiusa mentre era ferma — in
/// entrambi i casi non serve più segnalare un pannello che non c'è più.
pub fn decide_stalled(
    markers: &[Value],
    alive_short_ids: &BTreeSet<String>,
    now: i64,
    grace_seconds: i64,
) -> Vec<StalledSession> {
    let mut out = Vec::new();
    for m in markers {
        let Some(session_id) = m.get("session_id").and_then(|v| v.as_str()) else { continue };
        if !alive_short_ids.contains(&short(session_id)) {
            continue;
        }
        let requested_at_epoch = m.get("requested_at_epoch").and_then(|v| v.as_i64()).unwrap_or(now);
        let elapsed = now - requested_at_epoch;
        if elapsed < grace_seconds {
            continue;
        }
        out.push(StalledSession {
            session_id: session_id.to_string(),
            tool_name: m.get("tool_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            summary: m.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            cwd: m.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            requested_at: m.get("requested_at").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            seconds_pending: elapsed,
        });
    }
    out
}

fn alive_short_ids() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(live_dir()) {
        for e in entries.flatten() {
            if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                out.insert(stem.to_string());
            }
        }
    }
    out
}

fn read_markers() -> Vec<Value> {
    let Ok(entries) = fs::read_dir(markers_dir()) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<Value>(&s).ok())
        .collect()
}

fn grace_seconds() -> i64 {
    std::env::var("PERMISSION_STALL_GRACE_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GRACE_SECONDS)
}

/// Il comando da riga di comando che legge da fuori: un elenco JSON delle
/// sessioni ferme, o `[]`. Non è un gancio — nessun evento lo accende — e per
/// questo sta in `NOT_HOOKS`.
pub fn run_report() -> i32 {
    let stalled = decide_stalled(&read_markers(), &alive_short_ids(), now_epoch(), grace_seconds());
    let body: Vec<Value> = stalled
        .iter()
        .map(|s| {
            serde_json::json!({
                "session_id": s.session_id,
                "tool_name": s.tool_name,
                "summary": s.summary,
                "cwd": s.cwd,
                "requested_at": s.requested_at,
                "seconds_pending": s.seconds_pending,
            })
        })
        .collect();
    println!("{}", Value::Array(body));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(tool_name: &str, session_id: &str, input: Value, cwd: &str) -> String {
        serde_json::json!({
            "tool_name": tool_name,
            "tool_input": input,
            "session_id": session_id,
            "cwd": cwd,
        })
        .to_string()
    }

    /// Caso nominale: un `PermissionRequest` con sessione riconoscibile produce
    /// una richiesta da dichiarare, col comando nel riassunto.
    ///
    /// MUTANTE: sostituendo `data.get("session_id")` con `data.get("session")`
    /// (il nome sbagliato) il test diventa ROSSO perché `extract_request`
    /// risponde `None` anche su un payload valido. Ripristinato, torna VERDE.
    #[test]
    fn a_request_with_a_session_is_extracted() {
        let raw = payload("Bash", "abc12345-def", serde_json::json!({"command": "npm install"}), "/tmp/x");
        let req = extract_request(&raw).expect("doveva estrarre");
        assert_eq!(req.session_id, "abc12345-def");
        assert_eq!(req.summary, "npm install");
    }

    /// Senza sessione non c'è chiave: niente da dichiarare, non un valore
    /// inventato.
    #[test]
    fn a_missing_session_id_extracts_nothing() {
        let raw = serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "ls"}}).to_string();
        assert!(extract_request(&raw).is_none());
        assert!(extract_request("{non valido").is_none());
    }

    /// Il caso che il difetto segnalato descrive: una richiesta rimasta sola
    /// oltre la soglia, con la sessione ancora viva, risulta ferma.
    #[test]
    fn a_marker_older_than_the_grace_period_with_a_live_session_is_stalled() {
        let markers = vec![marker_body(
            &PendingRequest {
                session_id: "d9fed018-9899-4f24-887b-bc519474d83d".to_string(),
                tool_name: "Bash".to_string(),
                summary: "npm install".to_string(),
                cwd: "/tmp".to_string(),
            },
            "2026-08-21T14:10:00Z",
            1000,
        )];
        let alive: BTreeSet<String> = ["d9fed018".to_string()].into_iter().collect();
        let stalled = decide_stalled(&markers, &alive, 1200, 45);
        assert_eq!(stalled.len(), 1, "una richiesta ferma da 200s deve comparire");
        assert_eq!(stalled[0].seconds_pending, 200);
    }

    /// IL CASO CHE DEVE FALLIRE: una sessione che lavora — il marcatore sparisce
    /// non appena qualcosa succede dopo — non risulta mai ferma, nemmeno molto
    /// dopo la richiesta.
    ///
    /// MUTANTE: se `clear()` non cancellasse il file (es. `let _ =
    /// fs::remove_file(...)` sostituito con un `return` prima della cancellazione),
    /// questo test diventa ROSSO perché il marcatore risulterebbe ancora ferma.
    /// Ripristinato, torna VERDE.
    #[test]
    fn a_session_that_moves_on_is_never_reported_stalled() {
        let home = crate::test_home::HomeIsolata::nuova("permission-stall-moves-on");
        std::fs::create_dir_all(home.stato().join("sessioni-vive")).unwrap();
        std::fs::write(home.stato().join("sessioni-vive/aaaaaaaa.json"), "{}").unwrap();

        let raw = payload("Bash", "aaaaaaaa-1111-2222-3333-444444444444", serde_json::json!({"command": "cargo build"}), "/tmp");
        declare(&raw);
        assert!(marker_path("aaaaaaaa-1111-2222-3333-444444444444").exists(), "declare doveva scrivere il marcatore");

        // La stessa sessione completa uno strumento: la richiesta è stata
        // risolta, di qualunque tipo fosse la risposta.
        let completed = serde_json::json!({"session_id": "aaaaaaaa-1111-2222-3333-444444444444"}).to_string();
        clear(&completed);

        let markers = read_markers();
        let alive: BTreeSet<String> = ["aaaaaaaa".to_string()].into_iter().collect();
        // Molto oltre la soglia: se il marcatore fosse ancora lì, risulterebbe ferma.
        let stalled = decide_stalled(&markers, &alive, now_epoch() + 10_000, 45);
        assert!(stalled.is_empty(), "una sessione che ha lavorato non deve risultare ferma");
    }

    /// Una sessione non più viva non conta, anche col marcatore ancora sul
    /// disco: o ha chiuso da sola, o si è chiusa mentre era ferma — in
    /// entrambi i casi non c'è più un pannello da segnalare.
    #[test]
    fn a_marker_for_a_dead_session_does_not_count() {
        let markers = vec![marker_body(
            &PendingRequest {
                session_id: "ffffffff-0000-0000-0000-000000000000".to_string(),
                tool_name: "Bash".to_string(),
                summary: "x".to_string(),
                cwd: "/tmp".to_string(),
            },
            "2026-08-21T14:10:00Z",
            0,
        )];
        let alive: BTreeSet<String> = BTreeSet::new();
        assert!(decide_stalled(&markers, &alive, 10_000, 45).is_empty());
    }

    /// Sotto la soglia — una richiesta appena arrivata — non conta come ferma:
    /// è la differenza fra «ferma» e «solo appena chiesta».
    #[test]
    fn a_fresh_marker_under_the_grace_period_does_not_count() {
        let markers = vec![marker_body(
            &PendingRequest {
                session_id: "eeeeeeee-0000-0000-0000-000000000000".to_string(),
                tool_name: "Bash".to_string(),
                summary: "x".to_string(),
                cwd: "/tmp".to_string(),
            },
            "2026-08-21T14:10:00Z",
            1000,
        )];
        let alive: BTreeSet<String> = ["eeeeeeee".to_string()].into_iter().collect();
        assert!(decide_stalled(&markers, &alive, 1010, 45).is_empty());
    }
}
