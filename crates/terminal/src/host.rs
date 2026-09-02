//! The host: the process that owns the pseudo-terminals, so that a window can
//! close and reopen while the sessions inside keep running.
//!
//! One unix socket, one request per connection, JSON on one line each way.
//! Output is not polled: a client that attaches holds its connection and
//! receives every chunk as it comes, framed in binary because the bytes of a
//! terminal are not text.

use crate::inbox;
use crate::session::{Opening, Summary, Terminals};
use crate::{Ending, Output, Routed, Workspace};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// The shape of the conversation. A client built against another number is
/// refused by name rather than misread.
pub const PROTOCOL: u32 = 1;

/// How much of what a terminal printed is kept for whoever attaches late.
///
/// A declared limit, not a policy: past it the oldest bytes go, and a pane
/// attached late starts mid-line. Half a megabyte is hours of an agent's
/// output and a few seconds of a build's.
pub const BACKLOG_LIMIT: usize = 512 * 1024;

/// Where the host answers, inside the store's home, beside the letterboxes.
pub fn address_in(store: &Path) -> PathBuf {
    inbox::mailroom(store).join("host.sock")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Hello,
    Open {
        workspace_root: String,
        program: Option<String>,
        args: Vec<String>,
        environment: Vec<(String, String)>,
        rows: u16,
        columns: u16,
        /// The profile the program runs under, as the opener knows it.
        #[serde(default)]
        profile: Option<String>,
    },
    Submit {
        id: String,
        line: String,
    },
    Press {
        id: String,
        bytes: Vec<u8>,
    },
    Resize {
        id: String,
        rows: u16,
        columns: u16,
    },
    Close {
        id: String,
    },
    List,
    Backlog {
        id: String,
    },
    Attach {
        id: String,
    },
}

/// Where a submitted line went, in the two forms the window tells apart.
///
/// `rule` is the id of the route that decided: whoever watches must be able
/// to reach the line of JSON that diverted their command, not only the flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Submitted {
    Command,
    Flow {
        flow: String,
        text: String,
        rule: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    Hello {
        protocol: u32,
        pid: u32,
    },
    Opened {
        summary: Summary,
    },
    Submitted {
        submitted: Submitted,
    },
    Done,
    Listed {
        terminals: Vec<Summary>,
    },
    /// `length` raw bytes follow the line: what the terminal printed from
    /// offset `at` up to `upto`.
    Backlog {
        at: u64,
        upto: u64,
        length: usize,
        ended: Option<String>,
    },
    /// Frames follow the line, until the terminal ends or the client leaves.
    Attached,
    Refused {
        why: String,
    },
}

/// One piece of what a terminal printed, or its end.
///
/// `at` is the offset of the first byte since the terminal opened, so a pane
/// that read the backlog can tell which live pieces it already has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Chunk { at: u64, bytes: Vec<u8> },
    Ended { status: String },
}

/// What a terminal printed, up to a limit, with the offset of its first byte.
pub struct Backlog {
    kept: Vec<u8>,
    start: u64,
    limit: usize,
}

impl Backlog {
    pub fn new(limit: usize) -> Backlog {
        Backlog {
            kept: Vec::new(),
            start: 0,
            limit,
        }
    }

    pub fn append(&mut self, bytes: &[u8]) {
        self.kept.extend_from_slice(bytes);
        if self.kept.len() > self.limit {
            let extra = self.kept.len() - self.limit;
            self.kept.drain(..extra);
            self.start += extra as u64;
        }
    }

    /// The offset of the first byte still kept.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// The offset just past the last byte seen.
    pub fn end(&self) -> u64 {
        self.start + self.kept.len() as u64
    }

    pub fn bytes(&self) -> &[u8] {
        &self.kept
    }
}

/// What the host knows of one terminal's output: the backlog, whether it has
/// ended, and who is listening right now.
struct Relay {
    state: Mutex<RelayState>,
}

struct RelayState {
    backlog: Backlog,
    ended: Option<String>,
    watchers: Vec<Sender<Frame>>,
}

impl Relay {
    fn new() -> Relay {
        Relay {
            state: Mutex::new(RelayState {
                backlog: Backlog::new(BACKLOG_LIMIT),
                ended: None,
                watchers: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RelayState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// From now on: the backlog is asked for apart, and the offsets are what
    /// let the two be joined without a gap or a repeat.
    fn attach(&self) -> (Receiver<Frame>, Option<String>) {
        let (sender, receiver) = mpsc::channel();
        let mut state = self.lock();
        let ended = state.ended.clone();
        if ended.is_none() {
            state.watchers.push(sender);
        }
        (receiver, ended)
    }

    fn snapshot(&self) -> (u64, Vec<u8>, u64, Option<String>) {
        let state = self.lock();
        (
            state.backlog.start(),
            state.backlog.bytes().to_vec(),
            state.backlog.end(),
            state.ended.clone(),
        )
    }
}

impl Output for Relay {
    fn chunk(&self, bytes: &[u8]) {
        let mut state = self.lock();
        let at = state.backlog.end();
        state.backlog.append(bytes);
        let frame = Frame::Chunk {
            at,
            bytes: bytes.to_vec(),
        };
        state
            .watchers
            .retain(|watcher| watcher.send(frame.clone()).is_ok());
    }

    fn ended(&self, ending: Ending) {
        let mut state = self.lock();
        let status = ending.to_string();
        state.ended = Some(status.clone());
        for watcher in state.watchers.drain(..) {
            let _ = watcher.send(Frame::Ended {
                status: status.clone(),
            });
        }
    }
}

/// The terminals this process owns, and what each has printed.
pub struct Host {
    terminals: Terminals,
    relays: Mutex<HashMap<String, Arc<Relay>>>,
}

impl Host {
    pub fn new(terminals: Terminals) -> Host {
        Host {
            terminals,
            relays: Mutex::new(HashMap::new()),
        }
    }

    pub fn terminals(&self) -> &Terminals {
        &self.terminals
    }

    fn relay_of(&self, id: &str) -> Option<Arc<Relay>> {
        self.relays
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(id)
            .cloned()
    }

    fn unknown(&self, id: &str) -> String {
        let open = self.terminals.list();
        if open.is_empty() {
            return format!("no terminal called «{id}»: none is open");
        }
        let names: Vec<&str> = open.iter().map(|row| row.id.as_str()).collect();
        format!(
            "no terminal called «{id}»; open are: {}",
            names.join(", ")
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn open(
        &self,
        workspace_root: &str,
        program: Option<String>,
        args: Vec<String>,
        environment: Vec<(String, String)>,
        rows: u16,
        columns: u16,
        profile: Option<String>,
    ) -> Result<Summary, String> {
        let workspace = Workspace::open(workspace_root)
            .map_err(|error| format!("the workspace «{workspace_root}» does not open: {error}"))?;
        let mut opening = Opening {
            size: crate::Size { rows, columns },
            profile,
            ..Opening::default()
        };
        if let Some(program) = program.filter(|program| !program.trim().is_empty()) {
            opening.program = program.into();
        }
        opening.args = args.into_iter().map(Into::into).collect();
        opening.environment.extend(environment);

        let opened = self
            .terminals
            .open(workspace, &opening, |id| {
                let relay = Arc::new(Relay::new());
                self.relays
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(id.to_owned(), Arc::clone(&relay));
                relay as Arc<dyn Output>
            })
            .map_err(|error| error.to_string())?;
        Ok(opened.summary())
    }

    fn answer(&self, request: Request, mut stream: UnixStream) -> io::Result<()> {
        let answer = match request {
            Request::Hello => Answer::Hello {
                protocol: PROTOCOL,
                pid: std::process::id(),
            },
            Request::Open {
                workspace_root,
                program,
                args,
                environment,
                rows,
                columns,
                profile,
            } => match self.open(&workspace_root, program, args, environment, rows, columns, profile) {
                Ok(summary) => Answer::Opened { summary },
                Err(why) => Answer::Refused { why },
            },
            Request::Submit { id, line } => match self.terminals.find(&id) {
                None => Answer::Refused {
                    why: self.unknown(&id),
                },
                Some(terminal) => match terminal.submit(&line) {
                    Ok(Routed::Command { .. }) => Answer::Submitted {
                        submitted: Submitted::Command,
                    },
                    Ok(Routed::Flow { route, flow, text }) => Answer::Submitted {
                        submitted: Submitted::Flow {
                            flow,
                            text,
                            rule: route,
                        },
                    },
                    Err(error) => Answer::Refused {
                        why: error.to_string(),
                    },
                },
            },
            Request::Press { id, bytes } => match self.terminals.find(&id) {
                None => Answer::Refused {
                    why: self.unknown(&id),
                },
                Some(terminal) => match terminal.press(&bytes) {
                    Ok(()) => Answer::Done,
                    Err(error) => Answer::Refused {
                        why: error.to_string(),
                    },
                },
            },
            Request::Resize { id, rows, columns } => match self.terminals.find(&id) {
                None => Answer::Refused {
                    why: self.unknown(&id),
                },
                Some(terminal) => match terminal.resize(crate::Size { rows, columns }) {
                    Ok(()) => Answer::Done,
                    Err(error) => Answer::Refused {
                        why: error.to_string(),
                    },
                },
            },
            Request::Close { id } => match self.terminals.close(&id) {
                None => Answer::Refused {
                    why: self.unknown(&id),
                },
                Some(Ok(())) => {
                    self.relays
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(&id);
                    Answer::Done
                }
                Some(Err(error)) => Answer::Refused {
                    why: error.to_string(),
                },
            },
            Request::List => Answer::Listed {
                terminals: self.terminals.list(),
            },
            Request::Backlog { id } => match self.relay_of(&id) {
                None => Answer::Refused {
                    why: self.unknown(&id),
                },
                Some(relay) => {
                    let (at, bytes, upto, ended) = relay.snapshot();
                    write_line(
                        &mut stream,
                        &Answer::Backlog {
                            at,
                            upto,
                            length: bytes.len(),
                            ended,
                        },
                    )?;
                    stream.write_all(&bytes)?;
                    return stream.flush();
                }
            },
            Request::Attach { id } => match self.relay_of(&id) {
                None => Answer::Refused {
                    why: self.unknown(&id),
                },
                Some(relay) => {
                    let (frames, ended) = relay.attach();
                    write_line(&mut stream, &Answer::Attached)?;
                    if let Some(status) = ended {
                        return write_frame(&mut stream, &Frame::Ended { status });
                    }
                    // The connection ends with the terminal, or with the
                    // client: a write that fails is the client gone, and the
                    // dropped receiver takes this watcher off the relay.
                    for frame in frames {
                        write_frame(&mut stream, &frame)?;
                    }
                    return Ok(());
                }
            },
        };
        write_line(&mut stream, &answer)
    }
}

/// Answers at `address` until the process ends. Never returns on its own.
///
/// One thread per connection: an attached client holds its connection for
/// as long as it watches, and the others must not queue behind it.
pub fn serve(host: Arc<Host>, address: &Path) -> io::Result<()> {
    let listener = inbox::bind_unless_answered(address)?;
    for arriving in listener.incoming() {
        let Ok(stream) = arriving else { continue };
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(reading) => reading,
                Err(_) => return,
            });
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                return;
            }
            let request: Request = match serde_json::from_str(&line) {
                Ok(request) => request,
                Err(error) => {
                    let _ = write_line(
                        &mut &stream,
                        &Answer::Refused {
                            why: format!("not a request this host understands: {error}"),
                        },
                    );
                    return;
                }
            };
            let _ = host.answer(request, stream);
        });
    }
    Ok(())
}

fn write_line(into: &mut impl Write, answer: &Answer) -> io::Result<()> {
    let mut text = serde_json::to_string(answer).map_err(io::Error::other)?;
    text.push('\n');
    into.write_all(text.as_bytes())?;
    into.flush()
}

const CHUNK_FRAME: u8 = 1;
const ENDED_FRAME: u8 = 2;

/// One frame on the wire: a kind, the offset, the length, the payload.
fn write_frame(into: &mut impl Write, frame: &Frame) -> io::Result<()> {
    let (kind, at, payload): (u8, u64, &[u8]) = match frame {
        Frame::Chunk { at, bytes } => (CHUNK_FRAME, *at, bytes),
        Frame::Ended { status } => (ENDED_FRAME, 0, status.as_bytes()),
    };
    let length = u32::try_from(payload.len()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "a frame longer than the wire allows")
    })?;
    into.write_all(&[kind])?;
    into.write_all(&at.to_be_bytes())?;
    into.write_all(&length.to_be_bytes())?;
    into.write_all(payload)?;
    into.flush()
}

/// The next frame, or nothing when the far side closed cleanly.
fn read_frame(from: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut kind = [0u8; 1];
    match from.read_exact(&mut kind) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let mut at = [0u8; 8];
    from.read_exact(&mut at)?;
    let mut length = [0u8; 4];
    from.read_exact(&mut length)?;
    let mut payload = vec![0u8; u32::from_be_bytes(length) as usize];
    from.read_exact(&mut payload)?;
    match kind[0] {
        CHUNK_FRAME => Ok(Some(Frame::Chunk {
            at: u64::from_be_bytes(at),
            bytes: payload,
        })),
        ENDED_FRAME => Ok(Some(Frame::Ended {
            status: String::from_utf8_lossy(&payload).into_owned(),
        })),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a frame of kind {other}, which this client does not know"),
        )),
    }
}

/// What a pane receives when it attaches late: the backlog, and where it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub at: u64,
    pub bytes: Vec<u8>,
    pub upto: u64,
    pub ended: Option<String>,
}

/// The window's side of the conversation.
///
/// One connection per call and nothing kept between them: the host may have
/// been restarted between two calls, and a client holding a stale connection
/// would read a refusal where a fresh one gets an answer.
#[derive(Debug, Clone)]
pub struct Client {
    address: PathBuf,
}

impl Client {
    pub fn at(address: PathBuf) -> Client {
        Client { address }
    }

    pub fn in_store(store: &Path) -> Client {
        Client::at(address_in(store))
    }

    pub fn address(&self) -> &Path {
        &self.address
    }

    fn call(&self, request: &Request) -> io::Result<(BufReader<UnixStream>, Answer)> {
        let mut stream = UnixStream::connect(&self.address)?;
        let mut text = serde_json::to_string(request).map_err(io::Error::other)?;
        text.push('\n');
        stream.write_all(text.as_bytes())?;
        stream.flush()?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the host closed without answering",
            ));
        }
        let answer = serde_json::from_str(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        Ok((reader, answer))
    }

    fn ask(&self, request: &Request) -> Result<Answer, String> {
        let (_, answer) = self.call(request).map_err(|error| error.to_string())?;
        match answer {
            Answer::Refused { why } => Err(why),
            other => Ok(other),
        }
    }

    /// Whether anybody answers, and with which protocol. An `io::Error` here
    /// is «no host»; a refusal is a host that did not understand.
    pub fn hello(&self) -> io::Result<(u32, u32)> {
        let (_, answer) = self.call(&Request::Hello)?;
        match answer {
            Answer::Hello { protocol, pid } => Ok((protocol, pid)),
            Answer::Refused { why } => Err(io::Error::other(why)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("a greeting answered with {other:?}"),
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &self,
        workspace_root: &str,
        program: Option<String>,
        args: Vec<String>,
        environment: Vec<(String, String)>,
        rows: u16,
        columns: u16,
        profile: Option<String>,
    ) -> Result<Summary, String> {
        match self.ask(&Request::Open {
            workspace_root: workspace_root.to_owned(),
            program,
            args,
            environment,
            rows,
            columns,
            profile,
        })? {
            Answer::Opened { summary } => Ok(summary),
            other => Err(format!("opening answered with {other:?}")),
        }
    }

    pub fn submit(&self, id: &str, line: &str) -> Result<Submitted, String> {
        match self.ask(&Request::Submit {
            id: id.to_owned(),
            line: line.to_owned(),
        })? {
            Answer::Submitted { submitted } => Ok(submitted),
            other => Err(format!("submitting answered with {other:?}")),
        }
    }

    pub fn press(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        self.done(&Request::Press {
            id: id.to_owned(),
            bytes: bytes.to_vec(),
        })
    }

    pub fn resize(&self, id: &str, rows: u16, columns: u16) -> Result<(), String> {
        self.done(&Request::Resize {
            id: id.to_owned(),
            rows,
            columns,
        })
    }

    pub fn close(&self, id: &str) -> Result<(), String> {
        self.done(&Request::Close { id: id.to_owned() })
    }

    fn done(&self, request: &Request) -> Result<(), String> {
        match self.ask(request)? {
            Answer::Done => Ok(()),
            other => Err(format!("{request:?} answered with {other:?}")),
        }
    }

    pub fn list(&self) -> Result<Vec<Summary>, String> {
        match self.ask(&Request::List)? {
            Answer::Listed { terminals } => Ok(terminals),
            other => Err(format!("listing answered with {other:?}")),
        }
    }

    pub fn backlog(&self, id: &str) -> Result<Snapshot, String> {
        let (mut reader, answer) = self
            .call(&Request::Backlog { id: id.to_owned() })
            .map_err(|error| error.to_string())?;
        match answer {
            Answer::Backlog {
                at,
                upto,
                length,
                ended,
            } => {
                let mut bytes = vec![0u8; length];
                reader
                    .read_exact(&mut bytes)
                    .map_err(|error| format!("the backlog was cut short: {error}"))?;
                Ok(Snapshot {
                    at,
                    bytes,
                    upto,
                    ended,
                })
            }
            Answer::Refused { why } => Err(why),
            other => Err(format!("the backlog answered with {other:?}")),
        }
    }

    /// Follows a terminal's output until it ends. Blocks: run it on a thread.
    pub fn attach(&self, id: &str, mut on_frame: impl FnMut(Frame)) -> Result<(), String> {
        let (mut reader, answer) = self
            .call(&Request::Attach { id: id.to_owned() })
            .map_err(|error| error.to_string())?;
        match answer {
            Answer::Attached => {}
            Answer::Refused { why } => return Err(why),
            other => return Err(format!("attaching answered with {other:?}")),
        }
        loop {
            match read_frame(&mut reader).map_err(|error| error.to_string())? {
                None => return Ok(()),
                Some(Frame::Ended { status }) => {
                    on_frame(Frame::Ended { status });
                    return Ok(());
                }
                Some(frame) => on_frame(frame),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The backlog keeps the tail and says where the tail starts: a pane that
    /// attaches late gets what is left, and the offset tells it what it lost.
    #[test]
    fn a_backlog_keeps_the_tail_and_knows_where_it_starts() {
        let mut backlog = Backlog::new(8);
        backlog.append(b"abcdef");
        assert_eq!(backlog.start(), 0);
        assert_eq!(backlog.end(), 6);
        backlog.append(b"ghij");
        assert_eq!(backlog.bytes(), b"cdefghij");
        assert_eq!(backlog.start(), 2);
        assert_eq!(backlog.end(), 10);
        // A chunk bigger than the whole limit leaves only its own tail.
        backlog.append(b"0123456789");
        assert_eq!(backlog.bytes(), b"23456789");
        assert_eq!(backlog.end(), 20);
    }

    /// A frame survives the wire byte for byte, offset included: it is what
    /// lets a pane join the backlog and the live output without a seam.
    #[test]
    fn a_frame_comes_back_as_it_went() {
        let mut wire = Vec::new();
        write_frame(
            &mut wire,
            &Frame::Chunk {
                at: 4_000_000_000,
                bytes: vec![0xC3, 0xA8, 0x00, 0xFF],
            },
        )
        .expect("write");
        write_frame(
            &mut wire,
            &Frame::Ended {
                status: "exited with 7".to_owned(),
            },
        )
        .expect("write");
        let mut from = wire.as_slice();
        assert_eq!(
            read_frame(&mut from).expect("read"),
            Some(Frame::Chunk {
                at: 4_000_000_000,
                bytes: vec![0xC3, 0xA8, 0x00, 0xFF],
            })
        );
        assert_eq!(
            read_frame(&mut from).expect("read"),
            Some(Frame::Ended {
                status: "exited with 7".to_owned(),
            })
        );
        assert_eq!(read_frame(&mut from).expect("read"), None);
    }

    /// The request and the answer are one line of JSON each, tagged: a client
    /// in another language reads `op` and `answer` and nothing else.
    #[test]
    fn requests_and_answers_are_tagged_lines() {
        let request = serde_json::to_value(Request::Press {
            id: "x-1".to_owned(),
            bytes: vec![3],
        })
        .expect("serialise");
        assert_eq!(request["op"], "press");
        assert_eq!(request["bytes"], serde_json::json!([3]));
        let answer = serde_json::to_value(Answer::Refused {
            why: "no".to_owned(),
        })
        .expect("serialise");
        assert_eq!(answer["answer"], "refused");
        let submitted = serde_json::to_value(Submitted::Flow {
            flow: "dispatch-the-work".to_owned(),
            text: "trova i residui".to_owned(),
            rule: "marked-request".to_owned(),
        })
        .expect("serialise");
        assert_eq!(
            submitted,
            serde_json::json!({
                "kind": "flow",
                "flow": "dispatch-the-work",
                "text": "trova i residui",
                "rule": "marked-request"
            })
        );
    }
}
