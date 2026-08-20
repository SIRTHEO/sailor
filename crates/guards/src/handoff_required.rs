//! La consegna prima della compattazione: cosa si decide, dati i fatti.
//!
//! Porto del giudizio di `skills/hooks/handoff-required.py`. Qui non si legge
//! niente e non si scrive niente: entrano i fatti già raccolti, esce una
//! decisione. L'involucro — stdin, i marcatori, il registro — sta in
//! `claude-hooks::handoff_required`.
//!
//! MANDATO (Theo, 11/08/2026, testuale): «non voglio vedere piu' la
//! compattazione ma voglio che venga sfruttato handoff in automatico».
//!
//! DUE SOGLIE, NON UNA. Avviso al 78% del budget di qualità del modello, obbligo
//! al 90%. Sono frazioni del budget di **qualità**, non della finestra tecnica:
//! la degradazione arriva molto prima del limite, e a punti diversi per modello.
//!
//! LA GARANZIA SI SODDISFA UNA VOLTA. Il blocco serve a ottenere che la consegna
//! sia scritta, non a fermare la sessione per sempre: scritta quella, passa
//! tutto. E dopo `BLOCK_CAP` rifiuti consecutivi il gancio si arrende comunque —
//! un presidio che manda in stallo una sessione fa più danno della compattazione
//! che voleva evitare, e lo stallo, a differenza della compattazione, non lascia
//! niente su disco.

use crate::handoff::{percent, thousands, Thresholds, HANDOFF_TOOLS};

/// Dopo tanti rifiuti consecutivi il gancio si arrende e lascia passare.
pub const BLOCK_CAP: u32 = 6;

/// Sopra questa frazione del budget la consegna già scritta smette di essere
/// un lasciapassare permanente. Decisione di Theo, 20/08/2026: fra l'obbligo
/// (90%) e la compattazione automatica nessuno sorvegliava una sessione che
/// aveva consegnato e continuava a lavorare — misurato su 450 sessioni, sale
/// indisturbata dai 450k fino ai 550k.
pub const LOCKOUT_FRACTION: f64 = 0.96;

/// Il contesto è oltre il gradino di blocco: la garanzia della consegna non
/// vale più come lasciapassare permanente.
pub fn over_lockout(used: u64, budget: u64) -> bool {
    used >= (budget as f64 * LOCKOUT_FRACTION) as u64
}

/// Cosa fa il gancio, una volta noti i fatti.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Non c'è niente da dire: si esce muti.
    Silent,
    /// Sopra l'avviso e sotto l'obbligo, e non si era ancora parlato.
    Warn(String),
    /// Sopra l'obbligo, ma questo strumento serve a consegnare.
    Pass,
    /// Sopra l'obbligo e oltre il tetto dei rifiuti: si lascia passare, dicendolo.
    Surrender(String),
    /// Sopra l'obbligo: lo strumento non passa.
    Block(String),
}

/// I fatti che il gancio ha già raccolto dal disco e dal payload.
pub struct Facts<'a> {
    pub tool: &'a str,
    /// Il modello e le soglie, dal transcript.
    pub thresholds: &'a Thresholds,
    /// I token in contesto. Zero significa «non misurabile»: si tace.
    pub used: u64,
    /// La consegna c'è **ed è ancora quella del lavoro in corso**.
    pub handoff_valid: bool,
    /// Si era già avvisato per questa sessione.
    pub already_warned: bool,
    /// Quanti rifiuti sono già stati opposti.
    pub blocks_so_far: u32,
}

pub fn decide(f: &Facts) -> Decision {
    // Zero non è «vuoto», è «non misurabile»: senza misura non si giudica.
    if f.used == 0 || f.used < f.thresholds.warn {
        return Decision::Silent;
    }
    // Consegna già fatta e ancora attuale: garanzia soddisfatta, si lavora —
    // ma non oltre il gradino di blocco: lì la consegna non è più un
    // lasciapassare permanente (vedi `LOCKOUT_FRACTION`).
    if f.handoff_valid && !over_lockout(f.used, f.thresholds.budget) {
        return Decision::Silent;
    }
    let pct = percent(f.used, f.thresholds.budget);
    let model = &f.thresholds.model;
    let budget = f.thresholds.budget;

    if f.used < f.thresholds.require {
        // Un avviso ripetuto è rumore: parla una volta sola, poi tace.
        if f.already_warned {
            return Decision::Silent;
        }
        return Decision::Warn(format!(
            "CONTESTO AL {pct}% del budget di qualita' di {model} (~{} token: \
             oltre, il modello degrada). Consegna adesso, finche' c'e' contesto \
             per ragionare: invoca la skill `handoff` (scrive un memory file che \
             la prossima sessione ricarica da sola). La compattazione comprime \
             quello che c'e'; la consegna sceglie quello che serve. Al 90% del \
             budget questo gancio rifiuta gli strumenti che non servono a \
             consegnare, finche' l'handoff non e' scritto.",
            thousands(budget)
        ));
    }

    if HANDOFF_TOOLS.contains(&f.tool) {
        return Decision::Pass;
    }

    // `blocks_so_far` è il conteggio **prima** di questo rifiuto, come
    // nell'originale: il confronto col tetto guarda quanti se ne sono già
    // opposti, e il messaggio della resa cita quel numero.
    if f.blocks_so_far >= BLOCK_CAP {
        return Decision::Surrender(format!(
            "CONTESTO AL {pct}%: il gancio ha chiesto {} volte la consegna e ora \
             lascia passare. Se stai per essere compattato senza consegna, il \
             lavoro lo ritrova solo chi rilegge il transcript.",
            f.blocks_so_far
        ));
    }

    // Qui solo perché la consegna c'è già ma il contesto è sopra il gradino
    // di blocco: non manca la consegna, va aggiornata. Via d'uscita in testa,
    // leggibile anche da chi non può chiedere a Theo.
    if f.handoff_valid {
        return Decision::Block(format!(
            "Aggiorna la consegna e chiudi il turno: invoca di nuovo la skill \
             `handoff`, poi le due righe Stato / Procedo con. CONTESTO AL {pct}% \
             del budget di qualita' di {model} ({} token, budget ~{}), oltre il \
             gradino di blocco (96%): sopra questa soglia la consegna gia' \
             scritta non e' piu' un lasciapassare permanente — decisione di \
             Theo del 20/08/2026. `{}` non serve a consegnare, quindi non \
             passa. Passano: Skill, Read, Write, Edit, Grep, Glob, SendMessage \
             e gli strumenti delle lavorazioni.",
            thousands(f.used),
            thousands(budget),
            f.tool
        ));
    }

    Decision::Block(format!(
        "CONTESTO AL {pct}% del budget di qualita' di {model} ({} token, budget \
         ~{}). Prima che il modello degradi va fatta la consegna — mandato di \
         Theo dell'11/08/2026.\n\n\
         Adesso, in quest'ordine:\n\
         1. Invoca la skill `handoff`: scrive un memory file di consegna + il \
         puntatore in MEMORY.md, che la prossima sessione ricarica da sola. Cita \
         specifiche, piani e commit per percorso invece di ricopiarli.\n\
         2. Chiudi il turno con le due righe: Stato / Procedo con.\n\n\
         Fatto l'handoff il gancio si sblocca e si torna a lavorare: il blocco \
         serve solo a garantire che la consegna sia scritta una volta.\n\
         `{}` non serve a consegnare, quindi non passa. Passano: Skill, Read, \
         Write, Edit, Grep, Glob, SendMessage e gli strumenti delle lavorazioni.\n\
         Se il lavoro puo' chiudersi adesso, chiudilo: una consegna scritta per \
         rimandare l'ultimo passo costa due volte.",
        thousands(f.used),
        thousands(budget),
        f.tool
    ))
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

    fn fatti<'a>(t: &'a Thresholds, used: u64, tool: &'a str) -> Facts<'a> {
        Facts {
            tool,
            thresholds: t,
            used,
            handoff_valid: false,
            already_warned: false,
            blocks_so_far: 0,
        }
    }

    #[test]
    fn sotto_l_avviso_non_si_dice_niente() {
        let t = soglie();
        assert_eq!(decide(&fatti(&t, 100_000, "Bash")), Decision::Silent);
        // Zero non è «contesto vuoto», è «non misurabile».
        assert_eq!(decide(&fatti(&t, 0, "Bash")), Decision::Silent);
    }

    #[test]
    fn fra_le_due_soglie_si_avvisa_una_volta_sola() {
        let t = soglie();
        let d = decide(&fatti(&t, 400_000, "Bash"));
        match d {
            Decision::Warn(m) => {
                assert!(m.contains("CONTESTO AL 80%"), "{m}");
                assert!(m.contains("~500,000 token"), "budget without a thousands separator: {m}");
            }
            altro => panic!("atteso un avviso, non {altro:?}"),
        }
        let mut f = fatti(&t, 400_000, "Bash");
        f.already_warned = true;
        assert_eq!(decide(&f), Decision::Silent, "un avviso ripetuto è rumore");
    }

    #[test]
    fn a_valid_handoff_disarms_below_the_lockout_step() {
        // Sotto il gradino (96%, qui 480_000) la consegna scritta basta ancora.
        let t = soglie();
        for used in [400_000, 460_000, 479_999] {
            let mut f = fatti(&t, used, "Bash");
            f.handoff_valid = true;
            assert_eq!(decide(&f), Decision::Silent, "used={used}");
        }
    }

    #[test]
    fn above_the_lockout_step_a_valid_handoff_is_not_enough_anymore() {
        // Il difetto misurato su 450 sessioni: `handoff_valid` era un
        // lasciapassare a QUALUNQUE livello di contesto, e la sessione saliva
        // indisturbata fino alla compattazione. Qui il presidio torna a
        // negare, e la via d'uscita sta in testa al messaggio.
        let t = soglie();
        let mut f = fatti(&t, 480_000, "Bash");
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
        let mut f = fatti(&t, 479_999, "Bash");
        f.handoff_valid = true;
        assert_eq!(decide(&f), Decision::Silent, "un token sotto: ancora disarmata");
        f.used = 480_000;
        assert!(matches!(decide(&f), Decision::Block(_)), "sul gradino esatto: nega");
    }

    #[test]
    fn without_a_handoff_above_the_lockout_step_it_denies_as_before() {
        // Nessuna regressione sul caso che già funzionava: senza consegna, il
        // messaggio resta quello originale, non quello della consegna da
        // aggiornare.
        let t = soglie();
        match decide(&fatti(&t, 480_000, "Bash")) {
            Decision::Block(m) => {
                assert!(!m.starts_with("Aggiorna la consegna"), "{m}");
                assert!(m.contains("Prima che il modello degradi va fatta la consegna"), "{m}");
            }
            other => panic!("atteso un blocco, non {other:?}"),
        }
    }

    #[test]
    fn sopra_l_obbligo_passa_solo_cio_che_serve_a_consegnare() {
        let t = soglie();
        assert_eq!(decide(&fatti(&t, 480_000, "Skill")), Decision::Pass);
        assert_eq!(decide(&fatti(&t, 480_000, "Read")), Decision::Pass);
        assert!(matches!(decide(&fatti(&t, 480_000, "Bash")), Decision::Block(_)));
        // Il blocco NON deve mai colpire la consegna stessa: sarebbe una trappola.
        assert_eq!(decide(&fatti(&t, 480_000, "Write")), Decision::Pass);
    }

    #[test]
    fn dopo_il_tetto_dei_rifiuti_il_gancio_si_arrende() {
        let t = soglie();
        let mut f = fatti(&t, 480_000, "Bash");
        f.blocks_so_far = BLOCK_CAP - 1;
        assert!(matches!(decide(&f), Decision::Block(_)), "un rifiuto ancora c'è");
        f.blocks_so_far = BLOCK_CAP;
        match decide(&f) {
            Decision::Surrender(m) => assert!(m.contains("ha chiesto 6 volte"), "{m}"),
            altro => panic!("atteso una resa, non {altro:?}"),
        }
    }

    #[test]
    fn il_motivo_del_blocco_nomina_lo_strumento_e_i_numeri() {
        let t = soglie();
        match decide(&fatti(&t, 480_000, "WebFetch")) {
            Decision::Block(m) => {
                assert!(m.contains("`WebFetch` non serve a consegnare"), "{m}");
                assert!(m.contains("480,000 token"), "le migliaia con la virgola: {m}");
                assert!(m.contains("budget ~500,000"), "{m}");
            }
            altro => panic!("atteso un blocco, non {altro:?}"),
        }
    }

    #[test]
    fn le_migliaia_si_raggruppano_come_in_python() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(180_000), "180,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
