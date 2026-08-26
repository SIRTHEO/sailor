//! Il cruscotto: cosa sta facendo il sistema, calcolato e non raccontato.
//!
//! PERCHÉ ESISTE. Il 26/08/2026 Theo ha chiesto «come vedo che stai
//! lavorando?» e la risposta onesta era: da nessuna parte. Il comando che le
//! memorie citano non esiste più fra le ricette del progetto, e i verbi del
//! binario raccontano un pezzo per uno — i costi, lo stato del lavoro — senza
//! che nessuno li metta insieme.
//!
//! Lo stesso giorno il difetto si era già presentato due volte in altra forma:
//! il ciclo notturno scriveva il rapporto nel file del giorno prima, e la ronda
//! produceva mandati senza lasciare traccia nel registro. In tutti e tre i casi
//! il sistema lavorava e nessuno poteva vederlo — ed è la differenza fra un
//! sistema autonomo e uno che sembra spento.
//!
//! PURO: qui dentro non si legge un file né si lancia un processo. Chi raccoglie
//! i dati sta nell'involucro, e questa parte si prova passandole i fatti a mano
//! — compreso il caso che conta di più, quello in cui non c'è niente da
//! mostrare.

/// Un servizio residente e se sta girando davvero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub running: bool,
    /// Cosa ha deciso l'ultima volta, se lo dice. Vuoto quando non si sa.
    pub last_decision: String,
}

/// Ciò che il sistema ha prodotto in giornata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Produced {
    pub tasks_done: usize,
    pub tasks_green: usize,
    pub commits: usize,
    pub entries_closed: usize,
}

/// Ciò che aspetta qualcuno, con da quanto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub what: String,
    pub for_whom: String,
    pub hours_open: u32,
}

/// Uno scarto fra ciò che è in servizio e ciò che dovrebbe esserlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub what: String,
    pub live: String,
    pub expected: String,
}

#[derive(Debug, Clone, Default)]
pub struct Board {
    pub services: Vec<Service>,
    pub produced: Produced,
    pub waiting: Vec<Waiting>,
    pub drifts: Vec<Drift>,
}

/// LA REGOLA DI QUESTO CRUSCOTTO: una riga esiste solo se porta un numero che
/// poteva venire diverso. Niente «tutto a posto» quando non si è misurato
/// niente — è la frase che ha fatto credere per giorni che la ronda fosse
/// morta mentre lavorava.
pub fn render(b: &Board) -> String {
    let mut out = String::new();

    out.push_str("SAILOR — cosa sta facendo\n\n");

    out.push_str("IN SERVIZIO ORA\n");
    if b.services.is_empty() {
        out.push_str("  nessun servizio misurato: il cruscotto non ha potuto leggerli\n");
    }
    for s in &b.services {
        let mark = if s.running { "gira" } else { "fermo" };
        out.push_str(&format!("  {:<22} {}", s.name, mark));
        if !s.last_decision.is_empty() {
            out.push_str(&format!(" — {}", truncate(&s.last_decision, 44)));
        }
        out.push('\n');
    }

    out.push_str("\nOGGI HA PRODOTTO\n");
    let p = &b.produced;
    if p.tasks_done == 0 && p.commits == 0 && p.entries_closed == 0 {
        out.push_str("  niente di misurabile — e questo è un dato, non un silenzio\n");
    } else {
        if p.tasks_done > 0 {
            out.push_str(&format!(
                "  {} compiti eseguiti, di cui {} verdi\n",
                p.tasks_done, p.tasks_green
            ));
        }
        if p.commits > 0 {
            out.push_str(&format!("  {} commit\n", p.commits));
        }
        if p.entries_closed > 0 {
            out.push_str(&format!("  {} voci di coda chiuse\n", p.entries_closed));
        }
    }

    out.push_str("\nASPETTA QUALCUNO\n");
    if b.waiting.is_empty() {
        out.push_str("  niente in attesa\n");
    } else {
        // PRIMA IL TOTALE PER DESTINATARIO, POI POCHE RIGHE. Alla prima
        // lettura vera questa sezione ne ha stampate 71: un elenco che non
        // entra in una schermata insegna a saltarlo, ed è lo stesso modo in
        // cui l'indice delle memorie ha smesso di leggersi per intero.
        // Il numero per destinatario è la cosa che si guarda; i titoli
        // servono solo a sapere da dove ricominciare.
        let mut buckets: Vec<(String, usize, u32)> = Vec::new();
        for w in &b.waiting {
            match buckets.iter_mut().find(|(name, _, _)| *name == w.for_whom) {
                Some((_, n, oldest)) => {
                    *n += 1;
                    *oldest = (*oldest).max(w.hours_open);
                }
                None => buckets.push((w.for_whom.clone(), 1, w.hours_open)),
            }
        }
        buckets.sort_by(|a, b| b.1.cmp(&a.1));
        for (name, n, oldest) in buckets.iter().take(6) {
            out.push_str(&format!(
                "  {:<26} {:>3} voci · la più vecchia da {} h\n",
                truncate(name, 26),
                n,
                oldest
            ));
        }
        out.push_str(&format!("  ({} voci aperte in tutto)\n", b.waiting.len()));

        out.push_str("\n  le più vecchie:\n");
        for w in b.waiting.iter().take(3) {
            out.push_str(&format!(
                "    {:<42} {} h\n",
                truncate(&w.what, 42),
                w.hours_open
            ));
        }
    }

    if !b.drifts.is_empty() {
        out.push_str("\nNON COMBACIA\n");
        for d in &b.drifts {
            out.push_str(&format!(
                "  {}: in servizio {} · atteso {}\n",
                truncate(&d.what, 24),
                truncate(&d.live, 16),
                truncate(&d.expected, 16)
            ));
        }
    }

    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        Board {
            services: vec![Service {
                name: "notte".into(),
                running: true,
                last_decision: "salto: carico alto".into(),
            }],
            produced: Produced { tasks_done: 5, tasks_green: 5, commits: 6, entries_closed: 1 },
            waiting: vec![Waiting {
                what: "ronda: versione nuova di Claude Code".into(),
                for_whom: "sessione generale".into(),
                hours_open: 28,
            }],
            drifts: vec![Drift {
                what: "binario dei ganci".into(),
                live: "06481d1".into(),
                expected: "1acc3c4".into(),
            }],
        }
    }

    #[test]
    fn every_section_carries_its_numbers() {
        let out = render(&board());
        assert!(out.contains("notte"));
        assert!(out.contains("5 compiti eseguiti, di cui 5 verdi"));
        assert!(out.contains("6 commit"));
        assert!(out.contains("28 h"), "l'attesa si misura in ore, o non è un'attesa");
        assert!(out.contains("06481d1") && out.contains("1acc3c4"));
    }

    /// IL CASO CHE CONTA DI PIÙ, e per cui questo modulo esiste: quando non c'è
    /// niente da mostrare deve **dirlo**, non tacere. Un cruscotto vuoto che
    /// stampa «tutto a posto» è ciò che il 26/08 ha fatto credere che la ronda
    /// fosse morta mentre stava lavorando.
    #[test]
    fn an_empty_board_says_so_instead_of_reassuring() {
        let out = render(&Board::default());
        assert!(
            out.contains("non ha potuto leggerli"),
            "senza servizi misurati va detto che non si è misurato: {out}"
        );
        assert!(
            out.contains("è un dato, non un silenzio"),
            "zero produzione è un'informazione, non un vuoto da nascondere: {out}"
        );
        assert!(
            !out.to_lowercase().contains("tutto a posto"),
            "mai rassicurare senza aver misurato: {out}"
        );
    }

    /// Senza scarti la sezione sparisce del tutto: una sezione vuota che
    /// compare a ogni lettura insegna a saltarla, e il giorno che porta una
    /// riga nessuno la legge.
    #[test]
    fn the_drift_section_appears_only_when_there_is_drift() {
        let mut b = board();
        b.drifts.clear();
        assert!(!render(&b).contains("NON COMBACIA"));
        assert!(render(&board()).contains("NON COMBACIA"));
    }

    /// Una riga lunga non sfonda la colonna: il cruscotto si legge in un
    /// terminale, e una voce di coda ha un titolo lungo quanto vuole.
    #[test]
    fn a_long_entry_is_cut_to_the_column() {
        let mut b = board();
        b.waiting[0].what = "x".repeat(200);
        b.services[0].last_decision = "y".repeat(200);
        let out = render(&b);
        let longest = out.lines().map(|l| l.chars().count()).max().unwrap_or(0);
        assert!(longest <= 80, "riga da {longest} caratteri: sfonda il terminale");
    }

    /// LA CODA VERA È LUNGA, E UN CRUSCOTTO LUNGO NON SI LEGGE. Alla prima
    /// lettura sui dati veri questa sezione ha stampato **71 righe**: si
    /// riassume per destinatario e si mostrano poche voci, o diventa la
    /// schermata che si salta — lo stesso modo in cui l'indice delle memorie
    /// ha smesso di leggersi per intero.
    #[test]
    fn a_long_queue_is_summarised_not_listed() {
        let mut b = board();
        b.waiting = (0..71)
            .map(|i| Waiting {
                what: format!("voce-numero-{i}"),
                for_whom: if i % 3 == 0 { "Theo".into() } else { "un builder".into() },
                hours_open: 47 - (i as u32 % 40),
            })
            .collect();
        let out = render(&b);
        assert!(out.lines().count() < 30, "il cruscotto non entra in una schermata");
        assert!(out.contains("(71 voci aperte in tutto)"), "il totale va detto: {out}");
        assert!(out.contains("Theo"), "il destinatario più carico si nomina");
        assert!(
            out.contains("voci · la più vecchia da"),
            "per ogni destinatario servono quante e da quanto: {out}"
        );
    }
}
