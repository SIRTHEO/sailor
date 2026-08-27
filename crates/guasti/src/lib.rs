//! I guasti li sceglie il codice, non chi l'ha scritto.
//!
//! Una prova di guasto vale quanto vale l'elenco dei guasti, e un elenco
//! scritto da chi ha scritto il codice eredita i suoi punti ciechi: il
//! 27/08/2026 diciassette guasti scelti a mano sono usciti tutti rossi, e il
//! buco stava su una riga che non era in elenco. Qui l'elenco lo ricava il
//! programma dal **codice modificato** — il diff — e nessuno decide cosa
//! guardare.
//!
//! Il gemello per Rust esiste già ed è `cargo mutants` (`tools/cargo-mutants.sh`):
//! quello legge l'albero sintattico di rustc e vale solo lì. Questo lavora sul
//! testo, quindi risponde su TypeScript, JavaScript, Python e Rust insieme, ed
//! è l'unico che sa restringersi alle righe che una lavorazione ha toccato.
//!
//! QUESTO FILE E I SUOI MODULI NON TOCCANO NÉ DISCO NÉ PROCESSI: leggono
//! stringhe e restituiscono decisioni, così le prove le controllano senza
//! copiare alberi né lanciare batterie. Copia, esecuzione e ripristino stanno
//! in `main.rs`.

pub mod diff;
pub mod mutations;
pub mod report;
pub mod source;

/// Un guasto: la sostituzione di un pezzo di file con un altro testo.
///
/// L'ancoraggio è in byte sul file intero e non in riga-colonna, perché lo
/// stesso meccanismo deve applicare sia il cambio di un operatore sia lo
/// svuotamento di un corpo di funzione lungo trenta righe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// Percorso del file nel deposito, con le barre in avanti.
    pub file: String,
    /// Riga 1-based dove il guasto comincia: serve solo al rapporto.
    pub line: usize,
    /// Byte d'inizio dentro il file.
    pub offset: usize,
    /// Byte sostituiti.
    pub length: usize,
    /// Cosa dichiara di rompere, in una riga.
    pub label: String,
    /// Il testo che c'era: se non combacia, il guasto non si applica.
    pub before: String,
    /// Il testo che ci va.
    pub after: String,
}

impl Fault {
    /// Il nome con cui il guasto compare nel rapporto.
    pub fn name(&self) -> String {
        format!("{}:{} {}", self.file, self.line, self.label)
    }
}

/// L'esito di un guasto, dopo che la batteria ha detto la sua.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// La batteria è diventata rossa: il controllo controlla davvero.
    Killed,
    /// La batteria è rimasta verde: nessuna prova se ne accorge.
    Survived,
    /// Il codice guastato non compila: il guasto non dice niente su nessuna
    /// prova, e non va contato né da una parte né dall'altra.
    NotViable,
    /// Il testo da sostituire non c'era più: il guasto non ha morso.
    NotApplied,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Killed => "ucciso",
            Verdict::Survived => "SOPRAVVISSUTO",
            Verdict::NotViable => "non vitale",
            Verdict::NotApplied => "non applicato",
        }
    }
}

/// Il verdetto, dai due esiti che l'ambiente restituisce.
///
/// `build_ok` è `None` quando non è stato chiesto nessun comando di
/// compilazione: allora un guasto che non compila si presenta come ucciso —
/// la batteria fallisce comunque — e il rapporto non può distinguerlo. È il
/// motivo per cui `--build` vale la pena su un progetto tipato.
pub fn classify(applied: bool, build_ok: Option<bool>, test_ok: bool) -> Verdict {
    if !applied {
        return Verdict::NotApplied;
    }
    if build_ok == Some(false) {
        return Verdict::NotViable;
    }
    if test_ok {
        Verdict::Survived
    } else {
        Verdict::Killed
    }
}

/// Applica un guasto al sorgente. `None` se l'ancora non combacia più.
pub fn apply(source: &str, fault: &Fault) -> Option<String> {
    let end = fault.offset.checked_add(fault.length)?;
    if end > source.len() {
        return None;
    }
    if !source.is_char_boundary(fault.offset) || !source.is_char_boundary(end) {
        return None;
    }
    if &source[fault.offset..end] != fault.before {
        return None;
    }
    let mut out = String::with_capacity(source.len() + fault.after.len());
    out.push_str(&source[..fault.offset]);
    out.push_str(&fault.after);
    out.push_str(&source[end..]);
    // Un guasto che non cambia niente non è un guasto: sarebbe un verde
    // garantito, e quel verde verrebbe letto come una lacuna delle prove.
    if out == source {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fault(offset: usize, before: &str, after: &str) -> Fault {
        Fault {
            file: "a.ts".into(),
            line: 1,
            offset,
            length: before.len(),
            label: "prova".into(),
            before: before.into(),
            after: after.into(),
        }
    }

    #[test]
    fn a_fault_replaces_exactly_its_anchor() {
        let source = "if (a === b) return 1;";
        let applied = apply(source, &fault(6, "===", "!==")).expect("l'ancora combacia");
        assert_eq!(applied, "if (a !== b) return 1;");
    }

    #[test]
    fn a_fault_whose_anchor_moved_does_not_apply() {
        let source = "if (a === b) return 1;";
        assert_eq!(apply(source, &fault(6, "!==", "===")), None);
    }

    /// Un guasto che riscrive il testo con se stesso lascerebbe la batteria
    /// verde per costruzione, e quel verde si leggerebbe come una lacuna.
    #[test]
    fn a_fault_that_changes_nothing_is_refused() {
        let source = "if (a === b) return 1;";
        assert_eq!(apply(source, &fault(6, "===", "===")), None);
    }

    #[test]
    fn a_fault_past_the_end_does_not_apply() {
        let source = "abc";
        assert_eq!(apply(source, &fault(2, "cdef", "x")), None);
    }

    /// L'ancora a metà di un carattere multibyte non deve far cadere il
    /// programma: i commenti di questa casa sono pieni di accenti.
    #[test]
    fn a_fault_inside_a_multibyte_character_does_not_apply() {
        let source = "// è così";
        assert_eq!(apply(source, &fault(4, "\u{a8}", "x")), None);
    }

    #[test]
    fn the_verdicts_follow_the_two_outcomes() {
        assert_eq!(classify(false, None, true), Verdict::NotApplied);
        assert_eq!(classify(true, Some(false), false), Verdict::NotViable);
        assert_eq!(classify(true, Some(true), true), Verdict::Survived);
        assert_eq!(classify(true, Some(true), false), Verdict::Killed);
        // Senza comando di compilazione, il non vitale si nasconde fra gli
        // uccisi: è la ragione per cui `--build` esiste.
        assert_eq!(classify(true, None, false), Verdict::Killed);
    }
}
