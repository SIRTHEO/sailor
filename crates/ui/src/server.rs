//! Il servitore locale: una pagina HTML e una risposta JSON, niente altro.
//! Solo `127.0.0.1` — questa è la macchina di Theo, non un servizio
//! pubblico — e sola lettura: nessuna rotta scrive nel deposito. Niente
//! libreria HTTP: il workspace tiene le dipendenze al minimo di proposito,
//! e servire una pagina più qualche JSON sta comodo in un `TcpListener`.

use crate::dashboard::{build_executions, ExecutionView};
use crate::gather::{gather, ledger_present, GatherError};
use crate::registry::{flow_views, FlowRegistry, FlowView};
use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const INDEX_HTML: &str = include_str!("../assets/index.html");
const READ_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ServerState {
    pub ledger_dir: PathBuf,
    pub flows: FlowRegistry,
}

#[derive(Debug, Serialize)]
pub struct DashboardPayload {
    pub generated_at: i64,
    pub ledger_dir: String,
    pub ledger_present: bool,
    pub flows: Vec<FlowView>,
    pub executions: Vec<ExecutionView>,
}

pub fn build_payload(state: &ServerState) -> Result<DashboardPayload, GatherError> {
    let now = now_secs();
    let present = ledger_present(&state.ledger_dir);
    let data = gather(&state.ledger_dir)?;
    let executions = data
        .as_ref()
        .map(|data| build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, now))
        .unwrap_or_default();
    Ok(DashboardPayload {
        generated_at: now,
        ledger_dir: state.ledger_dir.to_string_lossy().into_owned(),
        ledger_present: present,
        flows: flow_views(&state.flows),
        executions,
    })
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

/// Accetta connessioni finché il processo vive. Ogni connessione va sul suo
/// thread: un client lento non deve bloccare chi guarda la pagina.
pub fn run_forever(listener: TcpListener, state: Arc<ServerState>) -> std::io::Result<()> {
    for stream in listener.incoming() {
        let stream = stream?;
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            let _ = handle_connection(stream, &state);
        });
    }
    Ok(())
}

pub fn handle_connection(stream: TcpStream, state: &ServerState) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let request_line = read_request(&stream)?;
    let Some((method, path)) = parse_request_line(&request_line) else {
        return respond(&stream, 400, "text/plain; charset=utf-8", b"richiesta malformata");
    };
    if method != "GET" {
        return respond(
            &stream,
            405,
            "text/plain; charset=utf-8",
            b"solo GET: questa pagina non modifica nulla",
        );
    }
    match path.as_str() {
        "/" | "/index.html" => respond(&stream, 200, "text/html; charset=utf-8", INDEX_HTML.as_bytes()),
        "/api/dashboard" => match build_payload(state) {
            Ok(payload) => {
                let body = serde_json::to_vec(&payload).unwrap_or_default();
                respond(&stream, 200, "application/json; charset=utf-8", &body)
            }
            Err(error) => {
                let body = format!("{{\"error\":{}}}", serde_json::to_string(&error.to_string()).unwrap());
                respond(&stream, 500, "application/json; charset=utf-8", body.as_bytes())
            }
        },
        _ => respond(&stream, 404, "text/plain; charset=utf-8", b"non trovato"),
    }
}

/// Legge la riga di richiesta e scarta le intestazioni fino alla riga vuota:
/// chiudere senza svuotare quello che il client ha già inviato può troncare
/// la risposta su alcuni sistemi.
fn read_request(stream: &TcpStream) -> std::io::Result<String> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut header = String::new();
    loop {
        header.clear();
        let bytes = reader.read_line(&mut header)?;
        if bytes == 0 || header == "\r\n" || header == "\n" {
            break;
        }
    }
    Ok(request_line)
}

fn parse_request_line(line: &str) -> Option<(String, String)> {
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?;
    let path = target.split('?').next().unwrap_or("").to_owned();
    Some((method, path))
}

fn respond(stream: &TcpStream, status: u16, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    let mut writer = stream;
    let header = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason_phrase(status),
        body.len()
    );
    writer.write_all(header.as_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_string_is_not_part_of_the_path() {
        assert_eq!(
            parse_request_line("GET /api/dashboard?x=1 HTTP/1.1\r\n"),
            Some(("GET".to_owned(), "/api/dashboard".to_owned()))
        );
    }

    #[test]
    fn a_line_without_a_target_is_rejected() {
        assert_eq!(parse_request_line("GET\r\n"), None);
    }

    #[test]
    fn an_empty_line_is_rejected() {
        assert_eq!(parse_request_line(""), None);
    }
}
