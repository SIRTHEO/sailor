//! Il censimento della macchina: quali terminali ci sono **adesso**, chi ci
//! vive dentro, e da dove sono stati aperti.
//!
//! **LA DISTINZIONE CHE QUESTO MODULO ESISTE PER FARE.** Dentro un perimetro
//! ristretto `ps` è negato, e il diniego è silenzioso appena si incanala
//! l'uscita: `ps -e | wc -l` risponde `0` con **uscita 0** e nessun errore. È il
//! guasto 12 con un altro strumento. Chi riceve un elenco vuoto non ha modo di
//! sapere se la macchina è deserta o se non gli hanno lasciato guardare, e le
//! due cose portano a due decisioni opposte.
//!
//! Per questo il risultato non è un vettore: è [`Census`], che ha tre stati e
//! non se ne può ignorare uno. `Terminals` porta almeno un terminale,
//! `NoTerminal` dice che abbiamo guardato e non c'era niente, `Refused` dice
//! che non abbiamo guardato.
//!
//! **IL CANARINO.** Un `ps` negato può anche uscire pulito con l'uscita vuota,
//! e allora nessun codice d'errore lo tradisce. Ma chi chiede la tabella dei
//! processi **è** un processo: se nella tabella non c'è il proprio pid, quella
//! tabella non è la macchina. È l'unico controllo che regge anche quando il
//! diniego non dice il proprio nome.
//!
//! **NESSUN PRODOTTO DECIDE QUI DENTRO.** L'ancora è `(tty, albero,
//! capostipite)`: il tty è un oggetto del kernel, l'albero è una cartella, e il
//! capostipite è **un'etichetta** — si stampa e si registra, non si interroga.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

/// Perché non ci è stato permesso guardare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// A chi era stata fatta la domanda.
    pub tool: String,
    /// Cosa ha risposto, con le parole che ha usato: un diniego riconosciuto
    /// per il suo testo si racconta, uno dedotto no.
    pub reason: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.tool, self.reason)
    }
}

/// Un processo che vive su un terminale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inhabitant {
    pub pid: u32,
    pub parent_pid: u32,
    pub tty: String,
    /// Da quanto vive, come lo scrive `ps`: non lo si converte, perché un
    /// formato riscritto è un formato che può divergere dalla sorgente.
    pub uptime: String,
    pub command: String,
    /// `None` vuol dire **non lo sappiamo**, non «nessuna».
    pub working_directory: Option<String>,
}

/// Un terminale, con dentro tutto quello che ci gira.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Terminal {
    pub tty: String,
    /// Chi ha disegnato la finestra, risalendo la catena dei genitori.
    /// **Etichetta**: nessuna decisione la legge.
    pub ancestor: Option<String>,
    pub inhabitants: Vec<Inhabitant>,
}

/// Cosa c'è sulla macchina, in tre stati che non si confondono.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Census {
    /// Almeno un terminale. **Mai vuoto**: lo garantisce [`Census::of`], e una
    /// prova lo tiene fermo.
    Terminals(Vec<Terminal>),
    /// Abbiamo guardato, e nessun processo ha un terminale.
    NoTerminal,
    /// Non abbiamo guardato.
    Refused(Refusal),
}

impl Census {
    /// I terminali visti, o niente se non li abbiamo visti. Chi chiama questa
    /// invece di leggere le varianti **sta buttando via la distinzione**, e per
    /// questo si chiama così: chiedere «quelli che ho visto» dice da sé che
    /// esiste un caso in cui non se ne è visto nessuno per un'altra ragione.
    pub fn seen(&self) -> &[Terminal] {
        match self {
            Self::Terminals(terminals) => terminals,
            Self::NoTerminal | Self::Refused(_) => &[],
        }
    }

    /// Il capostipite di un tty, se il censimento ne ha uno da dire.
    pub fn ancestor_of(&self, tty: &str) -> Option<&str> {
        self.seen()
            .iter()
            .find(|terminal| terminal.tty == tty)
            .and_then(|terminal| terminal.ancestor.as_deref())
    }
}

/// Chi risponde alle domande sulla macchina.
///
/// **È UN TRATTO PERCHÉ IL DINIEGO SI DEVE POTER PROVARE.** Dentro il perimetro
/// in cui girano le prove `ps` è negato davvero, quindi una prova che lo invoca
/// misura il perimetro, non il codice. Con un finto che risponde «non ti è
/// permesso» si prova la strada che quel diniego percorre, e con un finto che
/// risponde una tabella vera si prova la lettura — senza che le due prove
/// dipendano da dove girano.
pub trait Machine {
    /// `pid ppid tty etime comm`, una riga per processo, senza intestazione.
    fn process_table(&self) -> Result<String, Refusal>;
    /// La cartella di lavoro di un pid. `None` è «non lo so».
    fn working_directory(&self, pid: u32) -> Option<String>;
    /// Il pid di chi sta chiedendo: il canarino.
    fn own_pid(&self) -> u32;
}

/// Una riga della tabella dei processi, prima di sapere se ha un terminale.
#[derive(Debug, Clone)]
struct Row {
    pid: u32,
    parent_pid: u32,
    tty: String,
    uptime: String,
    command: String,
}

/// Il tty che `ps` scrive quando un processo non ne ha uno.
const NO_TTY: &str = "??";

/// Legge la tabella e la spezza in righe. Quello che non si legge si salta:
/// un'intestazione o una riga monca non deve far cadere il censimento.
fn parse_table(text: &str) -> Vec<Row> {
    text.lines().filter_map(parse_row).collect()
}

fn parse_row(line: &str) -> Option<Row> {
    let mut fields = line.split_whitespace();
    let pid = fields.next()?.parse().ok()?;
    let parent_pid = fields.next()?.parse().ok()?;
    let tty = fields.next()?.to_owned();
    let uptime = fields.next()?.to_owned();
    // Il comando può contenere spazi (`npm exec qualcosa`): è tutto il resto
    // della riga, non il campo successivo.
    let command = fields.collect::<Vec<_>>().join(" ");
    if command.is_empty() {
        return None;
    }
    Some(Row {
        pid,
        parent_pid,
        tty,
        uptime,
        command,
    })
}

/// L'etichetta di un comando.
///
/// **UNA CONVENZIONE DEL SISTEMA, NON LA CONOSCENZA DI UN PRODOTTO.** Su macOS
/// un'applicazione è una cartella `<Nome>.app`, e il binario che ci sta dentro
/// ha un percorso lungo che non dice niente a chi legge. Si prende il primo
/// `.app` del percorso, che è l'applicazione ospite; gli involucri interni ne
/// hanno di propri più in fondo. Nessun nome è scritto qui dentro.
pub fn label_for(command: &str) -> String {
    if let Some((head, _)) = command.split_once(".app/") {
        let name = head.rsplit('/').next().unwrap_or(head);
        if !name.is_empty() {
            return name.to_owned();
        }
    }
    command.rsplit('/').next().unwrap_or(command).to_owned()
}

/// Il capostipite di un processo: si risale finché c'è un genitore da risalire.
/// Torna anche **quanti gradini** sono stati saliti, e quel numero serve.
///
/// Ci si ferma sotto `launchd` (pid 1) perché il capostipite di tutto sarebbe
/// sempre lui, e un'etichetta uguale per tutti non etichetta niente.
fn ancestry_of(start: u32, table: &BTreeMap<u32, Row>) -> Option<(usize, String)> {
    let mut current = start;
    let mut steps = 0;
    let mut walked = BTreeSet::new();
    walked.insert(current);
    while let Some(row) = table.get(&current) {
        let parent = row.parent_pid;
        if parent <= 1 || !table.contains_key(&parent) || !walked.insert(parent) {
            break;
        }
        current = parent;
        steps += 1;
    }
    table
        .get(&current)
        .map(|row| (steps, label_for(&row.command)))
}

/// Il capostipite di un terminale: **la catena più lunga fra quelle dei suoi
/// abitanti**, non la prima che capita.
///
/// **PERCHÉ NON LA PRIMA.** Una salita si ferma anche quando il genitore non è
/// nella tabella — un processo riadottato, o uno che è appena morto — e allora
/// il processo stesso diventa il proprio capostipite: l'etichetta di un
/// terminale aperto da un'applicazione risultava `caffeinate`, che è il primo
/// pid del gruppo e non ha risalito niente. Chi è salito più in alto ha visto
/// di più, e la sua etichetta è quella vera.
fn ancestor_of_terminal(rows: &[&Row], table: &BTreeMap<u32, Row>) -> Option<String> {
    rows.iter()
        .filter_map(|row| ancestry_of(row.pid, table))
        .max_by_key(|(steps, _)| *steps)
        .map(|(_, found)| found)
}

impl Census {
    /// Il censimento, fatto sulla macchina che risponde.
    pub fn of(machine: &dyn Machine) -> Census {
        let text = match machine.process_table() {
            Ok(text) => text,
            Err(refusal) => return Census::Refused(refusal),
        };
        let rows = parse_table(&text);
        let own = machine.own_pid();
        if !rows.iter().any(|row| row.pid == own) {
            return Census::Refused(Refusal {
                tool: "ps".to_owned(),
                reason: format!(
                    "la tabella dei processi non contiene il pid di chi l'ha chiesta ({own}): \
                     {} righe lette. Una macchina senza chi la interroga non è una macchina \
                     vuota, è una risposta che non abbiamo ricevuto",
                    rows.len()
                ),
            });
        }

        let table: BTreeMap<u32, Row> = rows.iter().map(|row| (row.pid, row.clone())).collect();
        let mut grouped: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
        for row in rows.iter().filter(|row| row.tty != NO_TTY) {
            grouped.entry(row.tty.clone()).or_default().push(row);
        }
        if grouped.is_empty() {
            return Census::NoTerminal;
        }

        let terminals = grouped
            .into_iter()
            .map(|(tty, rows)| Terminal {
                ancestor: ancestor_of_terminal(&rows, &table),
                inhabitants: rows
                    .into_iter()
                    .map(|row| Inhabitant {
                        pid: row.pid,
                        parent_pid: row.parent_pid,
                        tty: row.tty.clone(),
                        uptime: row.uptime.clone(),
                        command: row.command.clone(),
                        working_directory: machine.working_directory(row.pid),
                    })
                    .collect(),
                tty,
            })
            .collect();
        Census::Terminals(terminals)
    }
}

/// La macchina vera, interrogata con gli strumenti che ci sono su ogni Unix.
pub struct LocalMachine;

impl Machine for LocalMachine {
    fn process_table(&self) -> Result<String, Refusal> {
        read_from("ps", &["-e", "-o", "pid=,ppid=,tty=,etime=,comm="])
    }

    fn working_directory(&self, pid: u32) -> Option<String> {
        let text = read_from("lsof", &["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"]).ok()?;
        // `-Fn` scrive un campo per riga, con la lettera del campo davanti:
        // `p<pid>`, `fcwd`, `n<percorso>`.
        text.lines()
            .find_map(|line| line.strip_prefix('n'))
            .map(str::to_owned)
    }

    fn own_pid(&self) -> u32 {
        std::process::id()
    }
}

/// **NIENTE PIPE, MAI.** L'uscita si cattura direttamente, così un diniego
/// arriva con il suo testo e il suo codice invece di sparire nel comando
/// successivo. `ps -e | wc -l` risponde `0` con uscita `0`; `Command::output`
/// risponde con l'errore che c'è.
fn read_from(tool: &str, args: &[&str]) -> Result<String, Refusal> {
    let output = Command::new(tool).args(args).output().map_err(|error| Refusal {
        tool: tool.to_owned(),
        reason: format!("non è partito: {error}"),
    })?;
    let complaint = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !output.status.success() {
        return Err(Refusal {
            tool: tool.to_owned(),
            reason: format!(
                "uscito con {}{}",
                output.status,
                if complaint.is_empty() {
                    String::new()
                } else {
                    format!(": {complaint}")
                }
            ),
        });
    }
    if !complaint.is_empty() {
        return Err(Refusal {
            tool: tool.to_owned(),
            reason: complaint,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
