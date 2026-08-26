//! Il giudizio puro del freno che nega l'uscita a chi lascia un filo scoperto.
//!
//! Gemello di `handoff_on_stop`, sull'altro lato dello stesso problema: quel
//! gancio arma chi arriva dopo, questo nega a chi se ne va adesso. Nasce dalla
//! stessa segnalazione di `claude-hooks::uncovered_thread` (il filo Sailor
//! rimasto senza nessuno per 2h40 il 25/08/2026), ma agisce PRIMA che il filo
//! sparisca: qui la sessione che sta per fermarsi è ancora quella che lo
//! tiene, ed è l'ultimo istante in cui può scegliere di non lasciarlo.
//!
//! UN SOLO TENTATIVO, non un tetto come `STOP_CAP`. Quel gemello pretende un
//! gesto meccanico preciso — scrivere la consegna — e ha senso insistere: il
//! gesto può essere fatto in ogni momento. Qui si chiede di accorgersi e
//! scegliere, e il motivo del rinvio (`fuori-orario`, `troppe-sessioni`,
//! `albero-affollato`, `seconda-generazione`) di solito non cambia entro i
//! pochi minuti di un turno in più: insistere ripeterebbe la stessa domanda
//! senza offrire una risposta diversa. Misurato il 25/08/2026 su
//! `state/ganci.jsonl`: lo stesso motivo di rinvio ricorre fino a 226 volte in
//! un giorno, concentrato su poche sessioni che ritentano — un tetto più alto
//! qui moltiplicherebbe le forzature sulla stessa manciata di sessioni, non la
//! qualità della scelta.

/// Quante forzature prima di lasciare andare comunque, **sullo stesso filo**.
pub const EXIT_CAP: u32 = 1;

/// Quante forzature prima di lasciare andare comunque, **in tutta la sessione**.
///
/// Il numero viene dal registro del 26/08/2026, non dal gusto: le sessioni
/// fermate da questo freno lo erano 10, 5, 5, 3 e 1 volta. Con due, le prime
/// tre si sarebbero fermate a due invece di dieci e cinque, e l'ultima non
/// avrebbe visto differenza. Sopra il due non c'è nessun caso in cui la
/// forzatura successiva abbia prodotto la raccolta del filo: ha prodotto altro
/// lavoro, che è il costo che questo freno dovrebbe evitare.
pub const SESSION_EXIT_CAP: u32 = 2;

/// Cosa fa il freno, una volta noti i fatti.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Nessun filo proprio scoperto, o già dentro una catena indotta: si
    /// lascia fermare in silenzio.
    Pass,
    /// Il tetto è già stato raggiunto: si lascia fermare, ma si registra.
    Surrender,
    /// Non ci si ferma: il messaggio va su stderr e la sessione continua.
    Block(String),
}

/// I fatti che il chiamante ha già raccolto dal disco.
pub struct Facts {
    /// Vero se, in questo stesso Stop, un marcatore di `uncovered_thread`
    /// esiste per QUESTA sessione — cioè il suo successore non si è armato per
    /// un motivo che lascia il filo scoperto (vedi
    /// `claude_hooks::uncovered_thread::uncovers`, già applicato da chi scrive
    /// il marcatore: qui non si riclassifica il motivo, si legge se il
    /// marcatore c'è).
    pub own_thread_uncovered: bool,
    /// Vero quando lo stop è già dentro un blocco indotto da un gancio: anti-loop
    /// primario, come nel gemello — altrimenti una catena di Stop ravvicinati
    /// senza un turno vero in mezzo si riblocherebbe da sola.
    pub stop_hook_active: bool,
    /// Quante volte questo freno ha già negato **per questo stesso filo**,
    /// prima di ora. Si azzera quando il filo cambia, ed è voluto: un filo
    /// nuovo merita il suo tentativo.
    ///
    /// Fino al 26/08/2026 il commento qui diceva «per questa sessione», e non
    /// era vero: è quella confusione che ha lasciato passare il difetto sotto.
    pub blocks_so_far: u32,
    /// Quante volte ha negato **a questa sessione**, per qualunque filo. Non si
    /// azzera mai.
    ///
    /// PERCHÉ SERVE UN SECONDO CONTATORE. Con il solo tetto per filo, il 26/08
    /// il registro contava una sessione fermata **dieci** volte e altre due
    /// cinque, a fronte di un tetto dichiarato di uno: ogni volta che il
    /// marcatore cambiava, il conteggio ripartiva da zero e il tetto non
    /// mordeva mai. Un freno che si può azzerare cambiando ciò che sorveglia
    /// non è un tetto, è un suggerimento.
    pub blocks_in_session: u32,
    /// Il motivo del rinvio, per il messaggio.
    pub reason: String,
}

pub fn decide(f: &Facts) -> Decision {
    if f.stop_hook_active || !f.own_thread_uncovered {
        return Decision::Pass;
    }
    if f.blocks_so_far >= EXIT_CAP || f.blocks_in_session >= SESSION_EXIT_CAP {
        return Decision::Surrender;
    }
    Decision::Block(message(f))
}

/// Il testo che la sessione legge su stderr.
///
/// L'AZIONE IN TESTA, NON LA DIAGNOSI — come nel gemello, che apre con
/// «Aggiorna la consegna e continua a lavorare», non con la percentuale di
/// contesto. Qui l'azione si biforca subito in due strade, e nessuna delle
/// due è indicata come «più semplice»: `Decision::Settle` scatta a qualunque
/// livello di contesto, e sopra il gradino di blocco (96%) continuare a
/// lavorare consuma proprio il margine che il gancio gemello vuole
/// preservare (`LOCKOUT_GROWTH_FRACTION`). Trovato in revisione il
/// 25/08/2026: la prima stesura diceva «è la via più semplice» davanti a
/// «continua», ed è precisamente l'incentivo sbagliato nel momento in cui
/// questo freno conta di più.
pub fn message(f: &Facts) -> String {
    format!(
        "Raccogli questo filo prima di fermarti: il successore non si e' armato \
         ({}), e questa sessione e' l'unica che lo tiene ancora. Continua tu il \
         lavoro, oppure — se hai un motivo per non raccoglierlo — dillo in una \
         riga e fermati: la prossima volta questo gancio lascia andare comunque, \
         una sola forzatura.",
        f.reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(uncovered: bool) -> Facts {
        Facts {
            own_thread_uncovered: uncovered,
            stop_hook_active: false,
            blocks_so_far: 0,
            blocks_in_session: 0,
            reason: "fuori-orario".into(),
        }
    }

    #[test]
    fn without_own_uncovered_thread_it_lets_stop() {
        // MUTANTE: tolto il controllo su `own_thread_uncovered`, questo caso
        // va in rosso da solo — nessun altro fatto vale `false` qui.
        assert_eq!(decide(&facts(false)), Decision::Pass);
    }

    #[test]
    fn an_own_uncovered_thread_denies_the_first_time() {
        match decide(&facts(true)) {
            Decision::Block(m) => {
                assert!(m.starts_with("Raccogli questo filo"), "l'azione in testa: {m}");
                assert!(m.contains("fuori-orario"), "il motivo si nomina: {m}");
                assert!(m.contains("dillo in una riga"), "{m}");
            }
            other => panic!("atteso un blocco, non {other:?}"),
        }
    }

    #[test]
    fn the_message_does_not_rank_continuing_as_the_easier_path() {
        // MUTANTE: reintrodotta la clausola «e' la via piu' semplice» davanti
        // a «continua», questo caso muore — era esattamente l'incentivo
        // sbagliato quando il contesto e' gia' vicino al gradino di blocco.
        let Decision::Block(m) = decide(&facts(true)) else {
            panic!("atteso un blocco");
        };
        assert!(
            !m.to_lowercase().contains("piu' semplice") && !m.to_lowercase().contains("più semplice"),
            "nessuna delle due strade e' indicata come piu' semplice: {m}"
        );
    }

    #[test]
    fn after_one_forcing_the_brake_surrenders() {
        // MUTANTE: tolto `>=` (o il confronto intero), questo caso resta
        // verde su tutto il resto — nessun altro tocca il tetto.
        let mut f = facts(true);
        f.blocks_so_far = EXIT_CAP;
        assert_eq!(decide(&f), Decision::Surrender);
        f.blocks_so_far = EXIT_CAP - 1;
        assert!(matches!(decide(&f), Decision::Block(_)), "una forzatura c'e' ancora");
    }

    /// IL CASO VERO DEL 26/08/2026, che il tetto per filo non prendeva.
    ///
    /// Il contatore per filo si azzera quando il marcatore cambia — e cambia a
    /// ogni giro, perché il motivo del rinvio è ricalcolato. Così una sessione
    /// è stata fermata dieci volte con un tetto dichiarato di uno. Qui si
    /// riproduce: filo sempre nuovo (`blocks_so_far` a zero ogni volta), e il
    /// freno deve comunque arrendersi quando la sessione ha già pagato il suo.
    #[test]
    fn a_thread_that_keeps_changing_still_hits_the_session_ceiling() {
        let mut f = facts(true);
        f.blocks_so_far = 0; // filo nuovo: il tetto per filo non morde mai
        for already in 0..SESSION_EXIT_CAP {
            f.blocks_in_session = already;
            assert!(
                matches!(decide(&f), Decision::Block(_)),
                "alla forzatura {already} la sessione ha ancora credito"
            );
        }
        f.blocks_in_session = SESSION_EXIT_CAP;
        assert_eq!(
            decide(&f),
            Decision::Surrender,
            "oltre il tetto di sessione il freno deve lasciare andare, anche con un filo nuovo"
        );
        // E molto oltre: è il caso da dieci blocchi che ha aperto la voce.
        f.blocks_in_session = 10;
        assert_eq!(decide(&f), Decision::Surrender);
    }

    /// I due tetti sono indipendenti: quello per filo continua a valere da
    /// solo. Senza questo, alzare `SESSION_EXIT_CAP` spegnerebbe in silenzio il
    /// tetto originale e nessun caso se ne accorgerebbe.
    #[test]
    fn the_per_thread_ceiling_still_bites_on_its_own() {
        let mut f = facts(true);
        f.blocks_in_session = 0;
        f.blocks_so_far = EXIT_CAP;
        assert_eq!(decide(&f), Decision::Surrender);
    }

    #[test]
    fn inside_an_induced_chain_it_does_not_reblock() {
        // MUTANTE: tolto il ramo `stop_hook_active`, un doppio Stop
        // ravvicinato senza turno vero in mezzo si ribloccherebbe.
        let mut f = facts(true);
        f.stop_hook_active = true;
        assert_eq!(decide(&f), Decision::Pass);
    }

    #[test]
    fn the_cap_is_exactly_one() {
        // Decisione ancorata a un numero, non a un aggettivo: se qualcuno
        // alzasse `EXIT_CAP` senza saperlo, questo caso lo dice.
        assert_eq!(EXIT_CAP, 1);
    }
}
