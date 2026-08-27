//! Le decisioni di un rilascio, separate dai gesti che lo eseguono.
//!
//! IL GUASTO CHE QUESTA STRADA ESISTE PER IMPEDIRE, misurato il 21/08/2026 alle
//! 20:20: `cargo build --release` costruisce dall'**albero**, non dal commit,
//! quindi chi ricompila mette in servizio anche ogni riga non committata di
//! chiunque altro. Quella sera erano circa novecento righe non promosse, di cui
//! quattrocento **bocciate** da un verdetto un quarto d'ora prima.
//!
//! PERCHÉ UN COMANDO E NON UNA REGOLA. Il gesto sembra personale e ha effetto
//! collettivo. Se la difesa fosse «ricordarsi di compilare altrove», fra due
//! giorni qualcuno compilerebbe dall'albero e avrebbe ragione lui: è il gesto
//! che `cargo` suggerisce. Un comando rende corta la strada giusta.
//!
//! COSA C'È QUI DENTRO E COSA NO. Come ogni crate di logica di questa casa —
//! `guards`, `sweep` — qui stanno le decisioni che si possono sbagliare in
//! silenzio, senza toccare disco, ambiente o processi: **cosa** si rilascia (la
//! tabella dei bersagli), **quale commit** ha prodotto il binario in servizio
//! (il timbro), e **se si può sostituire adesso** senza troncare una
//! lavorazione. I gesti — clonare, compilare, copiare, riavviare — stanno in
//! `main.rs`, dove non c'è niente da decidere.
//!
//! PERCHÉ UN SERVIZIO RESIDENTE NON BASTA SOSTITUIRLO. Un gancio nasce a ogni
//! chiamata e prende il binario nuovo da solo; `notte` esegue per sempre
//! l'immagine caricata all'avvio. Fino al 27/08/2026 non esisteva nessuna via
//! per rilasciarlo: il binario cambiava senza che nessuno lo volesse — qualunque
//! `cargo build` riscriveva `target/release/notte` — e il comportamento non
//! cambiava quando qualcuno lo voleva, perché nessuno riavviava. La combinazione
//! peggiore delle due.

/// Un servizio residente: c'è chi va riavviato perché la sostituzione abbia
/// effetto, e chi no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Service {
    /// L'etichetta launchd predefinita, per `launchctl kickstart -k gui/<uid>/<label>`.
    ///
    /// **È un valore predefinito, non un fatto del codice.** Il nome del
    /// servizio è una proprietà dell'installazione — chi installa `notte` su
    /// un'altra macchina lo chiama come vuole — e questo crate va in un deposito
    /// pubblico. `RELEASE_SERVICE_LABEL` lo sostituisce; qui resta il nome che
    /// ha su questa macchina, perché un predefinito sbagliato è meglio di un
    /// predefinito assente solo quando è scritto che è un predefinito.
    pub label: &'static str,
    /// Dove il servizio lascia la ricevuta della lavorazione che ha in mano,
    /// relativo a `~/.claude`. Un riavvio dato lì in mezzo lascia un compito né
    /// fatto né in coda.
    pub in_progress_rel: &'static str,
}

/// Cosa si rilascia. I percorsi sono relativi alla radice della configurazione:
/// una tabella con dentro `/Users/theo` non si potrebbe provare da nessun'altra
/// parte, e le prove girano in una casa isolata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// Come si nomina sulla riga di comando.
    pub name: &'static str,
    /// Il target `cargo` da costruire.
    pub bin: &'static str,
    /// Dove sta il binario che oggi qualcuno esegue davvero.
    pub live_rel: &'static str,
    /// La stessa copia **fuori da `target/`**, dove nessuna compilazione la
    /// raggiunge. Finché chi carica nomina ancora `target/release`, le due
    /// strade convivono e il rilascio le tiene allineate; il giorno in cui
    /// punta qui, compilare smette di mettere in servizio qualcosa — per
    /// costruzione, non per sorveglianza.
    pub safe_rel: &'static str,
    /// Il file che nomina il commit da cui è stato prodotto il binario in
    /// servizio. È **l'unico posto** in cui quel dato esiste.
    pub stamp_rel: &'static str,
    /// `None` per chi rinasce a ogni chiamata.
    pub service: Option<Service>,
}

/// I bersagli che esistono. Uno solo per ora ha un servizio dietro; `hooks` c'è
/// perché il giorno in cui `release-hooks.sh` viene ritirato la tabella lo sa
/// già fare, e perché una tabella con un elemento solo non mostra dove
/// finiscono le differenze.
pub const TARGETS: &[Target] = &[
    Target {
        name: "notte",
        bin: "notte",
        live_rel: "rust/target/release/notte",
        safe_rel: "bin/notte",
        stamp_rel: "state/notte-binary-commit",
        service: Some(Service {
            label: "com.theo.notte",
            in_progress_rel: "state/plancia/coda-notte/in-corso",
        }),
    },
    Target {
        name: "hooks",
        bin: "claude-hooks",
        live_rel: "rust/target/release/claude-hooks",
        safe_rel: "bin/claude-hooks",
        // Lo stesso file che `release-hooks.sh` scrive: due timbri per lo stesso
        // binario direbbero due verità, e chi legge non saprebbe quale.
        stamp_rel: "state/hooks-binary-commit",
        service: None,
    },
];

/// Il bersaglio che porta questo nome, se esiste.
pub fn target(name: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|t| t.name == name)
}

/// I nomi ammessi, per il messaggio di chi ne ha scritto uno che non c'è.
pub fn target_names() -> String {
    TARGETS
        .iter()
        .map(|t| t.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// La prima parola della prima riga che non è un commento.
///
/// Le righe che cominciano con `#` sono commento: servono a chi deve
/// **ricostruire** un valore invece di registrarlo — è successo al primo giro
/// dei ganci, dove il binario era stato compilato dall'albero e nessuno aveva
/// scritto da quale commit. Un dato ricostruito va usato, ma va detto.
pub fn read_stamp(contents: &str) -> Option<String> {
    contents
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_string)
}

/// Una lavorazione che il servizio ha in mano adesso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    pub task: String,
    pub pid: u32,
}

/// Se si può sostituire adesso, e cosa si è visto per dirlo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Readiness {
    /// Le lavorazioni in mano a un processo vivo. Finché ce n'è una, il riavvio
    /// aspetta.
    pub busy: Vec<Busy>,
    /// Ricevute che non dicono di chi sono: nome senza suffisso numerico, cioè
    /// file arrivati lì per un'altra via. **Non bloccano** — un file estraneo
    /// che ferma ogni rilascio per sempre è un guasto peggiore di quello che
    /// eviterebbe — ma si stampano, perché nessuno le scopra dopo.
    pub unknown: Vec<String>,
}

impl Readiness {
    pub fn is_ready(&self) -> bool {
        self.busy.is_empty()
    }
}

/// Legge le ricevute di `in-corso/` e dice se il servizio è a metà di qualcosa.
///
/// Il nome di una ricevuta lo separa `notte::split_receipt_name`, che è chi lo
/// scrive: due letture dello stesso formato divergono al primo dubbio.
///
/// PERCHÉ IL PID VIVO E NON LA SOLA PRESENZA DEL FILE. Una ricevuta rimasta da
/// un processo ucciso a metà resta lì fino al recupero del prossimo avvio: se
/// bastasse la sua presenza, un rilascio non partirebbe mai più proprio nel caso
/// in cui serve di più, cioè dopo che qualcosa è andato storto.
/// UN PID RICICLATO PUÒ MENTIRE, e si accetta: se il processo è morto e il
/// sistema ha dato quel numero a qualcun altro, qui risulta occupato. Non blocca
/// per sempre — chi aspetta ha un tetto e dopo quello rimanda — e la direzione
/// dell'errore è quella giusta: un falso «occupato» costa un rilascio rimandato,
/// un falso «libero» costa una lavorazione troncata.
pub fn readiness(receipt_names: &[String], alive: &dyn Fn(u32) -> bool) -> Readiness {
    let mut out = Readiness::default();
    for name in receipt_names {
        // I file nascosti non sono ricevute e non li ha scritti il servizio:
        // `.DS_Store` nasce da solo in ogni cartella che il Finder guarda, e
        // dichiararlo «non si sa di chi è» a ogni rilascio insegna a saltare
        // quella riga — che è dove un giorno comparirà una ricevuta vera.
        if name.starts_with('.') {
            continue;
        }
        let (task, pid) = notte::split_receipt_name(name);
        match pid {
            Some(pid) if alive(pid) => out.busy.push(Busy { task, pid }),
            Some(_) => {} // orfana: il servizio la recupera al prossimo avvio
            None => out.unknown.push(name.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_skips_comments_and_blank_lines() {
        let contents = "# ricostruito a mano il 24/08\n\n  abc123 (non fidarsi)\n";
        assert_eq!(read_stamp(contents).as_deref(), Some("abc123"));
    }

    #[test]
    fn a_stamp_of_only_comments_names_nothing() {
        assert_eq!(read_stamp("# niente\n#\n\n"), None);
    }

    /// Il caso che il rilascio esiste per non fare: troncare una lavorazione.
    #[test]
    fn a_receipt_of_a_live_process_holds_back_the_restart() {
        let names = vec!["triage-voci.task.4242".to_string()];
        let ready = readiness(&names, &|pid| pid == 4242);
        assert!(!ready.is_ready());
        assert_eq!(
            ready.busy,
            vec![Busy {
                task: "triage-voci.task".to_string(),
                pid: 4242
            }]
        );
    }

    /// E il caso opposto, che è quello in cui il rilascio serve di più: dopo un
    /// processo morto a metà, la ricevuta resta ma non è più di nessuno.
    #[test]
    fn an_orphaned_receipt_holds_back_nothing() {
        let names = vec!["triage-voci.task.4242".to_string()];
        let ready = readiness(&names, &|_| false);
        assert!(ready.is_ready());
        assert!(ready.unknown.is_empty());
    }

    #[test]
    fn a_receipt_without_a_pid_is_reported_but_does_not_block() {
        let names = vec!["arrivato-per-altra-via.txt".to_string()];
        let ready = readiness(&names, &|_| true);
        assert!(ready.is_ready());
        assert_eq!(ready.unknown, vec!["arrivato-per-altra-via.txt".to_string()]);
    }

    #[test]
    fn targets_are_found_by_name() {
        assert_eq!(target("notte").map(|t| t.bin), Some("notte"));
        assert_eq!(target("hooks").map(|t| t.bin), Some("claude-hooks"));
        assert!(target("nessuno").is_none());
    }

    /// Solo chi resta residente va riavviato: dirlo di un gancio farebbe
    /// chiamare `launchctl` su un'etichetta che non esiste.
    #[test]
    fn only_a_resident_service_declares_a_restart() {
        assert!(target("notte").and_then(|t| t.service).is_some());
        assert!(target("hooks").and_then(|t| t.service).is_none());
    }

    /// L'invariante che regge tutta la difesa: la seconda copia sta dove
    /// `cargo` non scrive. Se un giorno qualcuno la fa puntare dentro
    /// `target/`, le due strade diventano la stessa e la protezione sparisce
    /// senza che nulla si rompa — cioè in silenzio.
    ///
    /// SI CHIEDE DOVE STA, NON DOVE NON STA. Fino al 27/08/2026 qui c'era
    /// scritto solo «non contiene `target/`», e un revisore indipendente ha
    /// fatto notare che una stringa vuota lo soddisfa: la prova sarebbe rimasta
    /// verde con `safe_rel` che non nomina nessun posto. Un divieto lascia
    /// passare tutto ciò a cui non aveva pensato; un obbligo no.
    #[test]
    fn the_safe_copy_lives_under_bin() {
        for candidate in TARGETS {
            assert!(
                candidate.safe_rel.starts_with("bin/") && candidate.safe_rel.len() > 4,
                "{}: la copia sicura è '{}', che non sta in bin/ — sotto target/ cargo la riscrive",
                candidate.name,
                candidate.safe_rel
            );
        }
    }

    /// Le tre specie insieme, che è la forma vera di una cartella `in-corso/`
    /// dopo qualche giorno: una viva, una orfana, un intruso del Finder. Le tre
    /// prove separate qui sopra passerebbero anche se il codice trattasse la
    /// prima ricevuta e ignorasse le altre.
    #[test]
    fn the_three_kinds_of_receipt_are_told_apart_together() {
        let names = vec![
            "vecchia.task.111".to_string(),
            ".DS_Store".to_string(),
            "viva.task.222".to_string(),
            "arrivato-per-altra-via".to_string(),
        ];
        let ready = readiness(&names, &|pid| pid == 222);
        assert!(!ready.is_ready());
        assert_eq!(ready.busy.len(), 1);
        assert_eq!(ready.busy[0].task, "viva.task");
        assert_eq!(ready.unknown, vec!["arrivato-per-altra-via".to_string()]);
    }

    /// Un timbro ricostruito a mano porta più di un commento, e non sempre in
    /// testa: quello che conta è che la prima riga *utile* vinca comunque.
    #[test]
    fn the_stamp_reads_the_first_useful_line_whatever_surrounds_it() {
        let contents = "\n   \n# perché è stato ricostruito\n\t abc123 \n# e una nota dopo\ndef456\n";
        assert_eq!(read_stamp(contents).as_deref(), Some("abc123"));
    }
}
