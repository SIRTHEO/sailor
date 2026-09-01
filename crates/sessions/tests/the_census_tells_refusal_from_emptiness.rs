//! Il censimento dice *non lo so* invece di *niente*.
//!
//! **PERCHÉ QUESTA PROVA ESISTE, E PERCHÉ NON PUÒ INTERROGARE `ps`.** Dentro il
//! perimetro in cui gira questa batteria `ps` è negato davvero: una prova che
//! lo invocasse misurerebbe il perimetro, non il codice, e sulla macchina di
//! chi non ha quel perimetro non proverebbe più niente. Qui la macchina è un
//! finto, e ce ne sono tre: una che dice di no, una che **dice di sì e non
//! risponde** — il diniego silenzioso del guasto 12 — e una che risponde
//! davvero.
//!
//! Il guasto originale: `ps -e | wc -l` scrive `0` con **uscita 0**. Chi legge
//! un vettore vuoto non ha modo di distinguere «nessun terminale» da «non me
//! l'hanno lasciato chiedere», e le due cose portano a decisioni opposte.

use sessions::census::{Census, Machine, Refusal};

/// La macchina che rifiuta e lo dice.
struct Denied;

impl Machine for Denied {
    fn process_table(&self) -> Result<String, Refusal> {
        Err(Refusal {
            tool: "ps".to_owned(),
            reason: "operation not permitted: ps".to_owned(),
        })
    }
    fn working_directory(&self, _pid: u32) -> Option<String> {
        None
    }
    fn own_pid(&self) -> u32 {
        4242
    }
}

/// La macchina che rifiuta **senza dirlo**: uscita pulita, risposta vuota. È
/// la forma esatta che prende il diniego quando l'uscita passa da una pipe.
struct SilentlyDenied;

impl Machine for SilentlyDenied {
    fn process_table(&self) -> Result<String, Refusal> {
        Ok(String::new())
    }
    fn working_directory(&self, _pid: u32) -> Option<String> {
        None
    }
    fn own_pid(&self) -> u32 {
        4242
    }
}

/// La macchina che risponde: una tabella scritta come la scrive `ps -e -o
/// pid=,ppid=,tty=,etime=,comm=`, copiata da una vera.
struct Answering {
    table: &'static str,
}

impl Machine for Answering {
    fn process_table(&self) -> Result<String, Refusal> {
        Ok(self.table.to_owned())
    }
    fn working_directory(&self, pid: u32) -> Option<String> {
        // Solo di uno si sa: il resto resta `None`, che è «non lo so».
        (pid == 7073).then(|| "/Users/somebody/work/general".to_owned())
    }
    fn own_pid(&self) -> u32 {
        4242
    }
}

/// Una macchina viva, con due terminali, un capostipite comune e chi chiede
/// dentro la tabella.
const FIVE_LINES: &str = "\
 3354  7157 ttys001        03:32 caffeinate
 7072   886 ttys001  02-01:05:27 /usr/bin/login
 7073  7072 ttys001  02-01:05:27 /bin/zsh
 4242  7073 ttys001        00:01 sailor
32375   886 ttys002        58:21 /usr/bin/login
  886   675 ??         1-00:00:00 /Applications/Whatever.app/Contents/Frameworks/Whatever Helper.app/Contents/MacOS/Whatever Helper
  675     1 ??         1-00:00:00 /Applications/Whatever.app/Contents/MacOS/Whatever
";

/// Una macchina viva dove **nessuno** ha un terminale: la tabella c'è, chi
/// chiede c'è, e nessuna riga ha un tty.
const NO_ONE_ON_A_TERMINAL: &str = "\
    1     0 ??         9-00:00:00 /sbin/launchd
 4242     1 ??            00:01 sailor
";

#[test]
fn a_refusal_is_not_an_empty_machine() {
    match Census::of(&Denied) {
        Census::Refused(refusal) => {
            assert_eq!(refusal.tool, "ps");
            assert!(
                refusal.reason.contains("not permitted"),
                "il diniego deve portare le parole con cui è arrivato: {refusal}"
            );
        }
        other => panic!("un diniego è stato preso per una macchina vuota: {other:?}"),
    }
}

/// **IL CANARINO.** Un `ps` negato può uscire con codice 0 e uscita vuota, e
/// allora nessun errore lo tradisce. Ma chi chiede la tabella dei processi *è*
/// un processo: se non c'è, quella non è la macchina.
#[test]
fn a_silent_refusal_is_caught_by_the_canary() {
    match Census::of(&SilentlyDenied) {
        Census::Refused(refusal) => assert!(
            refusal.reason.contains("4242"),
            "il diniego silenzioso va spiegato col pid che manca: {refusal}"
        ),
        other => panic!(
            "uscita pulita e risposta vuota sono state prese per una macchina deserta: {other:?}"
        ),
    }
}

#[test]
fn a_machine_without_terminals_says_so_and_is_not_a_refusal() {
    assert_eq!(
        Census::of(&Answering {
            table: NO_ONE_ON_A_TERMINAL
        }),
        Census::NoTerminal
    );
}

#[test]
fn the_terminals_are_grouped_by_tty() {
    let census = Census::of(&Answering { table: FIVE_LINES });
    let Census::Terminals(terminals) = &census else {
        panic!("una tabella con due terminali non ha dato terminali: {census:?}");
    };
    let names: Vec<&str> = terminals.iter().map(|found| found.tty.as_str()).collect();
    assert_eq!(names, vec!["ttys001", "ttys002"]);
    assert_eq!(terminals[0].inhabitants.len(), 4, "{:?}", terminals[0]);
    assert_eq!(terminals[1].inhabitants.len(), 1);
}

/// Il capostipite si ottiene risalendo la catena dei genitori, e i due
/// terminali arrivano allo stesso. **È un'etichetta**: qui si guarda che ci
/// sia e che sia leggibile, non che sia un prodotto piuttosto che un altro.
#[test]
fn every_terminal_carries_the_label_of_who_drew_it() {
    let census = Census::of(&Answering { table: FIVE_LINES });
    assert_eq!(census.ancestor_of("ttys001"), Some("Whatever"));
    assert_eq!(census.ancestor_of("ttys002"), Some("Whatever"));
    assert_eq!(census.ancestor_of("ttys009"), None);
}

#[test]
fn each_process_carries_its_pid_its_age_its_command_and_where_it_works() {
    let census = Census::of(&Answering { table: FIVE_LINES });
    let terminals = census.seen();
    let shell = terminals[0]
        .inhabitants
        .iter()
        .find(|found| found.pid == 7073)
        .expect("la shell sta nella tabella");
    assert_eq!(shell.parent_pid, 7072);
    assert_eq!(shell.uptime, "02-01:05:27");
    assert_eq!(shell.command, "/bin/zsh");
    assert_eq!(
        shell.working_directory.as_deref(),
        Some("/Users/somebody/work/general")
    );
    let unknown = terminals[0]
        .inhabitants
        .iter()
        .find(|found| found.pid == 3354)
        .expect("caffeinate sta nella tabella");
    assert_eq!(
        unknown.working_directory, None,
        "una cartella che non si è potuta leggere resta «non lo so»"
    );
}

/// **`Terminals` NON PUÒ ESSERE VUOTO.** Se lo fosse, il tipo tornerebbe ad
/// avere due modi di dire «niente» e la distinzione che questo modulo esiste
/// per fare sparirebbe dall'interno.
#[test]
fn a_census_with_terminals_is_never_an_empty_one() {
    for machine in [NO_ONE_ON_A_TERMINAL, FIVE_LINES] {
        if let Census::Terminals(terminals) = Census::of(&Answering { table: machine }) {
            assert!(
                !terminals.is_empty(),
                "Terminals(vec![]) è un secondo modo di dire «niente»"
            );
        }
    }
}

/// Un comando con spazi dentro resta intero: `npm exec qualcosa` sono tre
/// parole e un comando solo.
#[test]
fn a_command_made_of_several_words_stays_whole() {
    let census = Census::of(&Answering {
        table: concat!(
            "   10     1 ttys003        00:05 npm exec socraticode\n",
            " 4242    10 ttys003        00:01 sailor\n"
        ),
    });
    let terminals = census.seen();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].inhabitants[0].command, "npm exec socraticode");
}
