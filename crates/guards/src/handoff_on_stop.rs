//! La consegna anche quando il turno finisce in sola prosa: cosa si decide.
//!
//! Porto del giudizio di `skills/hooks/handoff-on-stop.py`. Qui non si legge
//! niente e non si scrive niente: entrano i fatti già raccolti, esce una
//! decisione. L'involucro — stdin, i marcatori, il registro, e soprattutto gli
//! effetti su Orca — resta per ora nel Python.
//!
//! PERCHÉ ESISTE, ed è diverso dal gemello. `handoff_required` sta su
//! PostToolUse, che scatta solo dopo uno strumento: se a contesto pieno il turno
//! finisce in sola prosa, quel presidio non parte mai. Lo Stop è l'unico evento
//! che scatta a fine turno anche senza strumenti, ed è l'unico che può impedire
//! alla sessione di fermarsi.
//!
//! I DUE MOTIVI SONO INDIPENDENTI, e il secondo non è ridondante: ogni
//! compattazione azzera la finestra, quindi più una sessione è lunga meno è
//! probabile che il contesto arrivi alla soglia. Misurato il 14/08/2026 sulla
//! sessione `e1fa3600` — settima ripartenza, 129k token in contesto, obbligo a
//! 450k: col solo contesto il presidio non sarebbe partito mai.
//!
//! ANTI-LOOP, DOPPIA DIFESA. Un presidio sullo Stop che blocca sempre incastra
//! la sessione per sempre, che è peggio della compattazione che evita:
//! `stop_hook_active` ferma la catena al primo giro, e dopo `STOP_CAP` forzature
//! senza consegna il gancio si arrende comunque.

use crate::handoff::{percent, thousands, Thresholds};
use crate::handoff_required::over_lockout;

/// Dopo tante forzature senza consegna il gancio si arrende e lascia fermare.
pub const STOP_CAP: u32 = 3;

/// Il tetto di ripartenze predefinito, quando l'ambiente non ne impone un altro.
///
/// L'originale lo legge da `RIPARTENZA_TETTO_COMPATT`, **lo stesso nome che usa
/// `restart-notice`**: un tetto solo letto da due ganci, altrimenti si alza in un
/// posto e resta basso nell'altro. Qui arriva come dato, così il giudizio non
/// dipende dall'ambiente e i casi possono variarlo.
pub const RESTART_CAP_DEFAULT: u32 = 5;

/// Cosa fa il gancio, una volta noti i fatti.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Ci si può fermare: il gancio esce muto.
    Pass,
    /// Consegna fatta e attuale. Ci si ferma, ma prima l'involucro arma chi
    /// prosegue e poi si congeda — in quest'ordine, perché il congedo pretende
    /// un successore vivo e nell'ordine inverso non scatterebbe mai.
    Settle,
    /// Sopra soglia, ma si è già forzato abbastanza: si lascia fermare, e lo si
    /// scrive nel registro invece di tacere.
    Surrender,
    /// Non ci si ferma: il messaggio va su stderr e la sessione continua.
    Block(String),
}

/// I fatti che il gancio ha già raccolto dal payload e dal disco.
pub struct Facts<'a> {
    /// Vero quando lo stop è già dentro un blocco indotto da un gancio.
    pub stop_hook_active: bool,
    /// L'evento arriva da un subagent, non dalla conversazione principale.
    ///
    /// Oggi questo gancio è registrato sul solo `Stop`, che per i subagent non
    /// scatta (loro hanno `SubagentStop`), quindi il campo è sempre falso in
    /// servizio. Sta qui lo stesso perché il giorno che qualcuno lo registra
    /// anche sull'altro evento — che porta `agent_id` come tutti — il presidio
    /// comincerebbe a forzare la fine di ogni subagent, e nessuna prova se ne
    /// accorgerebbe.
    pub in_subagent: bool,
    /// Senza trascrizione non si misura niente, quindi non si giudica.
    ///
    /// È una cintura sul contratto, non un freno: i fatti che derivano da una
    /// trascrizione assente portano a `Pass` da soli, quindi il confronto col
    /// Python non può distinguere questo ramo. A tenerlo vivo è un caso della
    /// batteria con fatti volutamente incoerenti.
    pub has_transcript: bool,
    /// Il modello e le soglie, dalla trascrizione.
    pub thresholds: &'a Thresholds,
    /// I token in contesto. Zero significa «non misurabile».
    pub used: u64,
    /// La consegna c'è **ed è ancora quella del lavoro in corso**.
    pub handoff_valid: bool,
    /// Quante volte la sessione è ripartita da un riassunto.
    pub restarts: u32,
    /// Il tetto oltre il quale le ripartenze da sole obbligano alla consegna.
    pub restart_cap: u32,
    /// Quante forzature sono già state opposte, **prima** di questa.
    pub stop_blocks_so_far: u32,
}

pub fn decide(f: &Facts) -> Decision {
    // Un subagent non consegna per la madre: si lascia fermare. Vale come per
    // il gemello su PostToolUse — il payload porta la sessione e il transcript
    // della madre, quindi la misura è di un altro e il destinatario è sbagliato.
    if f.in_subagent {
        return Decision::Pass;
    }
    // Anti-loop primario: già dentro un blocco-stop indotto, non si riblocca.
    // Così si forza un giro per catena, non un ciclo.
    if f.stop_hook_active {
        return Decision::Pass;
    }
    if !f.has_transcript {
        return Decision::Pass;
    }
    // Garanzia soddisfatta: ci si ferma. Ma una consegna descrive il lavoro di
    // quel momento, quindi `handoff_valid` è già falso se dopo la consegna la
    // sessione è ripartita — il chiamante lo sa, qui si prende il verdetto.
    // Non oltre il gradino di blocco, però: lì la consegna già scritta non
    // basta più (`over_lockout`, gemello di `handoff_required`).
    if f.handoff_valid && !over_lockout(f.used, f.thresholds.budget) {
        return Decision::Settle;
    }

    // Zero non è «contesto vuoto», è «non misurabile»: senza misura questo
    // motivo tace, e resta in piedi solo quello delle ripartenze.
    let over_context = f.used > 0 && f.used >= f.thresholds.require;
    if !over_context && f.restarts < f.restart_cap {
        return Decision::Pass;
    }

    // Anti-loop secondario. Il conteggio è quello **prima** di questa forzatura,
    // come nell'originale.
    if f.stop_blocks_so_far >= STOP_CAP {
        return Decision::Surrender;
    }

    Decision::Block(message(f))
}

/// Il testo che la sessione legge su stderr.
///
/// DUE TESTE, e non sono intercambiabili: a chi è alla settima ripartenza non si
/// può dire «il contesto è pieno», perché non lo è — ed è esattamente il punto
/// per cui il secondo motivo esiste. Quando valgono entrambi vince quella delle
/// ripartenze, che è la ragione meno visibile a chi legge.
pub fn message(f: &Facts) -> String {
    // `decide` arriva qui con `handoff_valid` vero solo se il contesto è
    // sopra il gradino di blocco (altrimenti si è già fermata con `Settle`):
    // non manca la consegna, va aggiornata. Via d'uscita in testa, non in
    // coda — deve leggerla anche un agente senza nessuno a cui chiedere.
    if f.handoff_valid {
        let pct = percent(f.used, f.thresholds.budget);
        return format!(
            "Aggiorna la consegna e chiudi il turno: invoca di nuovo la skill \
             `handoff`, poi fermati. CONTESTO AL {pct}% del budget di qualita' \
             di {} ({} token, budget ~{}), oltre il gradino di blocco (96%): \
             sopra questa soglia la consegna gia' scritta non e' piu' un \
             lasciapassare permanente — decisione di Theo del 20/08/2026. Il \
             prossimo stop passa liscio solo dopo la nuova consegna.",
            f.thresholds.model,
            thousands(f.used),
            thousands(f.thresholds.budget)
        );
    }
    let head = if f.restarts >= f.restart_cap {
        format!(
            "SESSIONE ALLA {}ª RIPARTENZA (tetto {}) e stai per fermarti senza \
             aver consegnato. Il contesto adesso e' basso proprio perche' hai \
             gia' compattato: aspettare che si riempia non succedera' mai.",
            f.restarts, f.restart_cap
        )
    } else {
        // L'originale scrive `round(...) if used else 0`, ma `percent(0, b)` vale
        // già 0: un ramo che nessun caso può distinguere è codice morto
        // travestito da cautela, quindi qui non c'è.
        let pct = percent(f.used, f.thresholds.budget);
        format!(
            "CONTESTO AL {pct}% del budget di qualita' di {} ({} token, budget \
             ~{}) e stai per fermarti senza aver consegnato.",
            f.thresholds.model,
            thousands(f.used),
            thousands(f.thresholds.budget)
        )
    };
    format!(
        "{head} Prima di chiudere il turno invoca la skill `handoff`: scrive un \
         memory file di consegna + il puntatore in MEMORY.md, che la prossima \
         sessione ricarica da sola. La compattazione comprime quello che c'e'; \
         la consegna sceglie quello che serve, e adesso c'e' ancora contesto per \
         ragionare su cosa metterci. Fatto l'handoff, il prossimo stop passa \
         liscio."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soglie() -> Thresholds {
        Thresholds {
            model: "claude-opus-5".into(),
            budget: 500_000,
            warn: 390_000,
            require: 450_000,
        }
    }

    fn fatti(t: &Thresholds, used: u64) -> Facts<'_> {
        Facts {
            stop_hook_active: false,
            in_subagent: false,
            has_transcript: true,
            thresholds: t,
            used,
            handoff_valid: false,
            restarts: 0,
            restart_cap: RESTART_CAP_DEFAULT,
            stop_blocks_so_far: 0,
        }
    }

    #[test]
    fn a_subagent_is_never_held_back_from_stopping() {
        // MUTANTE: tolto il ramo `in_subagent`, questo caso va in rosso da solo.
        // Differenziale a variabile unica: gli stessi fatti che forzano la madre
        // lasciano fermare il subagent.
        let t = soglie();
        let mut f = fatti(&t, 480_000);
        f.in_subagent = true;
        assert_eq!(decide(&f), Decision::Pass);
        f.restarts = RESTART_CAP_DEFAULT + 2;
        assert_eq!(decide(&f), Decision::Pass, "nemmeno per le ripartenze");
        f.in_subagent = false;
        assert!(matches!(decide(&f), Decision::Block(_)));
    }

    #[test]
    fn sotto_entrambe_le_soglie_ci_si_ferma() {
        let t = soglie();
        assert_eq!(decide(&fatti(&t, 100_000)), Decision::Pass);
        let mut f = fatti(&t, 100_000);
        f.restarts = RESTART_CAP_DEFAULT - 1;
        assert_eq!(decide(&f), Decision::Pass, "una ripartenza sotto il tetto");
    }

    #[test]
    fn sopra_l_obbligo_di_contesto_non_ci_si_ferma() {
        let t = soglie();
        match decide(&fatti(&t, 480_000)) {
            Decision::Block(m) => {
                assert!(m.contains("CONTESTO AL 96%"), "{m}");
                assert!(m.contains("480,000 token"), "le migliaia con la virgola: {m}");
                assert!(m.contains("budget ~500,000"), "{m}");
                assert!(m.contains("claude-opus-5"), "il modello si nomina: {m}");
                assert!(m.contains("`handoff`"), "la skill si nomina: {m}");
                assert!(!m.to_lowercase().contains("mattpocock"), "la skill sbagliata: {m}");
            }
            altro => panic!("atteso un blocco, non {altro:?}"),
        }
    }

    #[test]
    fn le_ripartenze_bloccano_anche_a_contesto_basso() {
        // È il caso di `e1fa3600`: 129k token contro un obbligo a 450k. Col solo
        // motivo del contesto questo Block non esisterebbe.
        let t = soglie();
        let mut f = fatti(&t, 129_000);
        f.restarts = RESTART_CAP_DEFAULT;
        match decide(&f) {
            Decision::Block(m) => {
                assert!(m.contains("SESSIONE ALLA 5ª RIPARTENZA"), "{m}");
                assert!(m.contains("(tetto 5)"), "{m}");
                assert!(
                    !m.contains("CONTESTO AL"),
                    "a chi ha compattato non si dice che il contesto e' pieno: {m}"
                );
                assert!(m.contains("`handoff`"), "{m}");
            }
            altro => panic!("atteso un blocco, non {altro:?}"),
        }
    }

    #[test]
    fn con_entrambi_i_motivi_vince_la_testa_delle_ripartenze() {
        // MUTANTE: invertendo le due teste questo caso resta verde su tutto il
        // resto — è l'unico che distingue l'ordine.
        let t = soglie();
        let mut f = fatti(&t, 480_000);
        f.restarts = RESTART_CAP_DEFAULT + 2;
        match decide(&f) {
            Decision::Block(m) => {
                assert!(m.contains("SESSIONE ALLA 7ª RIPARTENZA"), "{m}");
                assert!(!m.contains("CONTESTO AL"), "{m}");
            }
            altro => panic!("atteso un blocco, non {altro:?}"),
        }
    }

    #[test]
    fn il_tetto_citato_e_quello_passato_non_una_costante() {
        // MUTANTE: scrivendo `5` nel testo invece del campo, questo caso muore.
        let t = soglie();
        let mut f = fatti(&t, 100_000);
        f.restart_cap = 9;
        f.restarts = 9;
        match decide(&f) {
            Decision::Block(m) => assert!(m.contains("(tetto 9)"), "{m}"),
            altro => panic!("atteso un blocco, non {altro:?}"),
        }
        // E sotto quel tetto più alto non si blocca più: 5 non basta.
        f.restarts = 5;
        assert_eq!(decide(&f), Decision::Pass);
    }

    #[test]
    fn una_consegna_valida_manda_a_congedarsi_non_a_tacere() {
        // MUTANTE: rispondere `Pass` qui lascerebbe il presidio verde e inerte —
        // nessuno armerebbe il successore e nessuno chiuderebbe la scheda. È
        // esattamente il modo in cui il congedo è stato rotto il 16/08/2026.
        // I due valori restano sotto il gradino di blocco (96%, qui 480_000):
        // sopra, la consegna valida smette di bastare da sola.
        let t = soglie();
        for used in [100_000, 470_000] {
            let mut f = fatti(&t, used);
            f.handoff_valid = true;
            assert_eq!(decide(&f), Decision::Settle, "used={used}");
        }
        let mut f = fatti(&t, 100_000);
        f.handoff_valid = true;
        f.restarts = RESTART_CAP_DEFAULT + 3;
        assert_eq!(decide(&f), Decision::Settle, "consegnata anche se ripartita");
    }

    #[test]
    fn above_the_lockout_step_a_valid_handoff_does_not_settle_anymore() {
        // Il difetto misurato su 450 sessioni: fra 450k (obbligo) e 550k
        // (compattazione) una sessione con `handoff_valid` saliva indisturbata.
        // Sopra il gradino il gemello di `handoff_required` torna a bloccare
        // anche lo Stop, e la via d'uscita sta in testa al messaggio.
        let t = soglie();
        let mut f = fatti(&t, 480_000);
        f.handoff_valid = true;
        match decide(&f) {
            Decision::Block(m) => {
                assert!(
                    m.starts_with("Aggiorna la consegna e chiudi il turno"),
                    "la via d'uscita va in testa, non in coda: {m}"
                );
                assert!(m.contains("`handoff`"), "{m}");
                assert!(m.contains("480,000 token"), "{m}");
            }
            other => panic!("atteso un blocco anche a consegna fatta, non {other:?}"),
        }
    }

    #[test]
    fn the_lockout_step_triggers_on_the_exact_threshold_not_after() {
        let t = soglie();
        let mut f = fatti(&t, 479_999);
        f.handoff_valid = true;
        assert_eq!(decide(&f), Decision::Settle, "un token sotto: ci si ferma ancora");
        f.used = 480_000;
        assert!(matches!(decide(&f), Decision::Block(_)), "sul gradino esatto: nega");
    }

    #[test]
    fn without_a_handoff_above_the_lockout_step_it_denies_as_before() {
        // Nessuna regressione: senza consegna, il messaggio resta quello
        // originale (contesto o ripartenze), non quello della consegna da
        // aggiornare.
        let t = soglie();
        match decide(&fatti(&t, 480_000)) {
            Decision::Block(m) => {
                assert!(!m.starts_with("Aggiorna la consegna"), "{m}");
                assert!(m.contains("CONTESTO AL 96%"), "{m}");
            }
            other => panic!("atteso un blocco, non {other:?}"),
        }
    }

    #[test]
    fn dentro_una_catena_indotta_non_si_riblocca() {
        let t = soglie();
        let mut f = fatti(&t, 480_000);
        f.stop_hook_active = true;
        assert_eq!(decide(&f), Decision::Pass);
        // E vale anche per il motivo delle ripartenze, non solo per il contesto.
        f.used = 100_000;
        f.restarts = RESTART_CAP_DEFAULT + 4;
        assert_eq!(decide(&f), Decision::Pass);
    }

    #[test]
    fn senza_trascrizione_non_si_giudica() {
        let t = soglie();
        let mut f = fatti(&t, 480_000);
        f.has_transcript = false;
        assert_eq!(decide(&f), Decision::Pass);
    }

    #[test]
    fn dopo_il_tetto_delle_forzature_il_gancio_si_arrende() {
        let t = soglie();
        let mut f = fatti(&t, 480_000);
        f.stop_blocks_so_far = STOP_CAP - 1;
        assert!(matches!(decide(&f), Decision::Block(_)), "una forzatura ancora c'è");
        f.stop_blocks_so_far = STOP_CAP;
        assert_eq!(decide(&f), Decision::Surrender);
    }

    #[test]
    fn l_obbligo_scatta_sulla_soglia_esatta_non_dopo() {
        // MUTANTE: `>=` diventa `>` e senza questo caso resta verde — nessun
        // altro tocca il confine.
        let t = soglie();
        assert_eq!(decide(&fatti(&t, t.require - 1)), Decision::Pass);
        assert!(matches!(decide(&fatti(&t, t.require)), Decision::Block(_)));
    }

    #[test]
    fn una_misura_assente_non_blocca_nemmeno_con_soglie_degeneri() {
        // Il ramo `used > 0` è invisibile finché l'obbligo è positivo: con
        // `require = 0` anche «non misurabile» supererebbe la soglia, e il
        // presidio bloccherebbe una sessione di cui non sa niente. Caso
        // costruito apposta, perché sul disco non ci passa nessuno.
        let degeneri = Thresholds {
            model: "ignoto".into(),
            budget: 0,
            warn: 0,
            require: 0,
        };
        let mut f = fatti(&degeneri, 0);
        f.thresholds = &degeneri;
        assert_eq!(decide(&f), Decision::Pass);
    }

    #[test]
    fn zero_token_non_e_contesto_vuoto() {
        // Una trascrizione senza `usage` non misura niente: il motivo del
        // contesto tace, e resta in piedi solo quello delle ripartenze.
        let t = soglie();
        assert_eq!(decide(&fatti(&t, 0)), Decision::Pass);
        let mut f = fatti(&t, 0);
        f.restarts = RESTART_CAP_DEFAULT;
        assert!(matches!(decide(&f), Decision::Block(_)));
    }
}
