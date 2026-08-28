//! Il riconoscimento di un documento di consegna, e l'impronta di chi l'ha già
//! armato una volta.
//!
//! IL PRESIDIO CHE APRIVA UN PANNELLO SUCCESSORE È STATO TOLTO IL 28/08/2026:
//! la staffetta ne aveva già abbandonato la funzione dal 19/08/2026 (347
//! scatti, tutti fermi). Quello che resta qui non decide più se aprire
//! niente — serve ad altri due meccanismi ancora vivi:
//! - `is_handoff_doc` (e ciò che le serve) la usa `relay.rs`, la staffetta,
//!   per riconoscere una consegna appena scritta;
//! - `armed_fingerprint`/`FingerprintOwner`/`recalculate_fingerprint_owner`
//!   le usa `marker_sweep.rs`, che ripulisce i marcatori rimasti sul disco
//!   da prima di questo cambio.

use regex::Regex;
use std::sync::OnceLock;

/// I marcatori che Orca antepone al titolo di un pannello con un agente dentro.
///
/// Usati da `handoff_on_stop::successor_alive` per distinguere una scheda
/// aperta-ma-vuota da un successore che sta davvero lavorando.
pub const AGENT_MARKS: &[char] = &['✳', '◑', '◐', '⏳'];

fn name_patterns() -> &'static [Regex; 2] {
    static RE: OnceLock<[Regex; 2]> = OnceLock::new();
    RE.get_or_init(|| {
        [
            Regex::new(r"/consegna-[^/]+\.md$").unwrap(),
            Regex::new(r"/handoff-[^/]+\.md$").unwrap(),
        ]
    })
}

fn in_memory() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Il Python usa `(?!MEMORY\.md$)`, che il crate `regex` non ha: il lookahead
    // negativo è sostituito dal controllo esplicito in `is_handoff_doc`, dove è
    // anche più leggibile di quanto fosse nella regex.
    RE.get_or_init(|| Regex::new(r"/memory/[^/]*\d{2}-\d{2}-\d{4}[^/]*\.md$").unwrap())
}

/// Sta in `memory/` e non è l'indice — senza pretendere niente dal nome.
fn in_memory_dir() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/memory/[^/]+\.md$").unwrap())
}

/// Le due intestazioni che la skill `handoff` prescrive nel corpo.
///
/// Sono il criterio che non passa dal nome del file, e servono perché il nome
/// non dice più niente: il comando `/handoff` ordina di AGGIORNARE la consegna
/// aperta sullo stesso filone (`commands/handoff.md:38`) e di scriverla in
/// `<memory-dir>/<slug>.md` (`:44`) — uno slug tematico, senza prefisso e senza
/// data. Le sezioni invece le prescrive una per una (`:57-65`), e sono ciò che
/// distingue una consegna da una nota di progetto.
fn body_marks() -> &'static [Regex; 2] {
    static RE: OnceLock<[Regex; 2]> = OnceLock::new();
    RE.get_or_init(|| {
        [
            Regex::new(r"(?mi)^#+\s*stato\b").unwrap(),
            Regex::new(r"(?mi)^#+\s*prossim[io]\s+pass").unwrap(),
        ]
    })
}

/// Il corpo ha la forma di una consegna: **entrambe** le sezioni, non una.
///
/// Entrambe perché «Stato» da solo compare in mezze note di progetto, mentre la
/// coppia no: misurata il 17/08/2026 su 415 documenti in `memory/`, seleziona 18
/// file — le consegne note, più quella che quel giorno era rimasta a terra — e
/// **zero** falsi positivi sui 187 senza `type: project`.
pub fn body_says_handoff(text: &str) -> bool {
    body_marks().iter().all(|re| re.is_match(text))
}

/// Il blocco di frontmatter: da `---` al `---` che lo chiude.
///
/// Il frontmatter si guarda solo in testa — `type: project` citato a metà corpo,
/// in una consegna che parla di consegne, non deve valere come dichiarazione —
/// ma «in testa» è il blocco, non una finestra di byte. Qui c'erano i primi 400,
/// ereditati dal Python, e il 17/08/2026 sono bastati a rendere il gancio cieco
/// una seconda volta sullo stesso documento: cresciuto a 32 KB, con una
/// `description` più lunga, `type: project` è finito al byte **419** e la
/// consegna ha smesso di essere riconosciuta un'ora dopo che l'avevamo fatta
/// riconoscere. Una soglia più alta avrebbe solo spostato il giorno del guasto.
///
/// Il tetto resta contro un file che comincia con `---` e non lo chiude più:
/// senza, un documento senza frontmatter farebbe scorrere tutto il testo.
fn front(text: &str) -> &str {
    const CAP: usize = 4096;
    let Some(rest) = text.strip_prefix("---") else {
        return "";
    };
    let mut fine = CAP.min(rest.len());
    while fine > 0 && !rest.is_char_boundary(fine) {
        fine -= 1;
    }
    let head = &rest[..fine];
    match head.find("\n---") {
        Some(chiusura) => &head[..chiusura],
        None => head,
    }
}

/// Il nome dice già che è una consegna?
pub fn name_says_handoff(path: &str) -> bool {
    name_patterns().iter().any(|re| re.is_match(path))
}

/// Sta in `memory/`, ha una data nel nome, e non è l'indice?
///
/// La data serve: `orca-usciere-tab.md` è `type: project` ma descrive uno
/// strumento, non una sessione da riprendere, e col solo controllo sul
/// frontmatter armava una sessione per sbaglio.
/// L'esclusione esplicita di `MEMORY.md` è **ridondante** finché la regex
/// pretende una data nel nome, e `MEMORY.md` non ne ha: verificato per mutazione
/// il 17/08/2026, toglierla non cambia una sola risposta su 482 casi. Resta
/// perché il Python la porta (ha lo stesso doppione, come lookahead negativo) e
/// perché è la difesa che regge se un domani la data smette di essere richiesta —
/// l'indice che arma una scheda a ogni aggiornamento è metà del difetto delle
/// due schede di quel giorno.
pub fn is_dated_memory(path: &str) -> bool {
    if !in_memory().is_match(path) {
        return false;
    }
    !path.ends_with("/memory/MEMORY.md")
}

/// È un documento di consegna? `text` è il contenuto del file.
///
/// Il nome da solo non basta e il frontmatter da solo nemmeno. La skill
/// `handoff` di questo progetto scrive un file dal nome libero dentro `memory/`,
/// dove finisce ogni sorta di memoria: solo quelle marcate `type: project` sono
/// consegne. Misurato il 13/08/2026 subito dopo aver acceso il gancio — su una
/// consegna vera rispondeva «non è una consegna», quindi era acceso e cieco.
///
/// LA DATA NEL NOME NON È PIÙ L'UNICA PORTA, dal 17/08/2026. Chiedere una data a
/// un file che la skill battezza con uno slug tematico rendeva il gancio cieco
/// sul caso che la skill stessa rende normale — aggiornare la consegna aperta.
/// Quel giorno una sessione all'88% del budget ha consegnato in
/// `sezioni-cv-campi-separati-o-testo.md` e non è partito niente: il gancio è
/// uscito prima di ogni freno, senza lasciare una riga di registro. Sul disco di
/// quel giorno erano invisibili 202 dei 227 documenti `type: project`.
///
/// Nel dubbio il gancio deve tacere: una consegna non raccolta costa
/// un'apertura a mano, una scheda aperta a sproposito costa fiducia — e a quel
/// punto lo si spegne del tutto. Per questo la porta nuova non allarga a tutto
/// `type: project`: pretende anche la forma del corpo.
pub fn is_handoff_doc(path: &str, text: Option<&str>) -> bool {
    if name_says_handoff(path) {
        return true;
    }
    if !in_memory_dir().is_match(path) || path.ends_with("/memory/MEMORY.md") {
        return false;
    }
    let Some(text) = text else { return false };
    if !front(text).contains("type: project") {
        return false;
    }
    is_dated_memory(path) || body_says_handoff(text)
}

/// L'impronta del marcatore «per questa sessione un successore c'è già».
///
/// Il percorso è uscito dalla chiave il 17/08/2026: il titolo del freno diceva
/// già «per sessione», ma l'impronta univa sessione e percorso, quindi due
/// documenti diversi nello stesso turno armavano due successori. Chi consegna
/// scrive la consegna e poi la riga d'indice — misurate due schede aperte a un
/// minuto di distanza.
pub fn armed_fingerprint(path: &str, session: &str) -> String {
    let key = if session.is_empty() { path } else { session };
    let digest = crate::duplication::sha1(key.as_bytes());
    digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .chars()
        .take(16)
        .collect()
}

/// A chi appartiene un'impronta, quando il nome del marcatore non porta
/// l'identificativo e va ricalcolata in avanti. Decisione del capitano,
/// 21/08/2026 15:55: i marcatori nati prima che si scrivesse anche la
/// sessione non hanno altra via per sapere di chi sono.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerprintOwner {
    /// L'impronta combacia con quella di una sessione viva adesso.
    Alive,
    /// Nessuna sessione viva la riproduce, e l'elenco si è potuto leggere:
    /// non appartiene a nessun vivo.
    Orphan,
    /// L'elenco delle sessioni vive non si è potuto leggere: non si sa, e
    /// un «non si sa» non si tratta come un elenco vuoto.
    Unknown,
}

/// PURA: ricalcola l'impronta per ogni sessione viva data da fuori, e dice a
/// chi appartiene il marcatore. `live_full_ids: None` è il terzo esito —
/// l'elenco non letto, non l'elenco letto e vuoto.
pub fn recalculate_fingerprint_owner(
    marker_hex: &str,
    marker_path: &str,
    live_full_ids: Option<&[String]>,
) -> FingerprintOwner {
    let Some(ids) = live_full_ids else {
        return FingerprintOwner::Unknown;
    };
    if ids
        .iter()
        .any(|id| armed_fingerprint(marker_path, id) == marker_hex)
    {
        FingerprintOwner::Alive
    } else {
        FingerprintOwner::Orphan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_nome_riconosce_le_due_forme_classiche() {
        assert!(name_says_handoff("/x/consegna-ganci-2026.md"));
        assert!(name_says_handoff("/x/handoff-sandbox.md"));
        assert!(!name_says_handoff("/x/consegna.txt"));
        // Senza barra davanti non è un percorso: il Python ancora al `/`.
        assert!(!name_says_handoff("consegna-x.md"));
    }

    #[test]
    fn in_memory_vuole_la_data_e_non_e_l_indice() {
        assert!(is_dated_memory("/p/memory/consegna-ganci-17-08-2026.md"));
        // `orca-usciere-tab.md` è `type: project` ma non è una sessione da
        // riprendere: senza la data nel nome armava una scheda per sbaglio.
        assert!(!is_dated_memory("/p/memory/orca-usciere-tab.md"));
        assert!(!is_dated_memory("/p/memory/MEMORY.md"));
    }

    #[test]
    fn il_frontmatter_decide_solo_dentro_memory() {
        // Nome giusto: il contenuto non serve nemmeno.
        assert!(is_handoff_doc("/x/consegna-a.md", None));
        // In memory con la data: serve `type: project`.
        let p = "/p/memory/note-17-08-2026.md";
        assert!(is_handoff_doc(p, Some("---\ntype: project\n---")));
        assert!(!is_handoff_doc(p, Some("---\ntype: reference\n---")));
        // File illeggibile: non si arma niente.
        assert!(!is_handoff_doc(p, None));
    }

    /// Il corpo di una consegna come la scrive `/handoff`, ridotto all'osso.
    fn handoff_body(extra: &str) -> String {
        format!("---\nname: x\nmetadata:\n  type: project\n---\n\n## Stato\n\nfatto.\n\n## Prossimi passi\n\n1. riprendere.\n{extra}")
    }

    #[test]
    fn il_corpo_riconosce_la_consegna_che_il_nome_non_dichiara() {
        // Il caso vero del 17/08/2026: slug tematico, nessuna data, `type:
        // project`, e le sezioni che la skill prescrive. Prima usciva `False`.
        let p = "/p/memory/sezioni-cv-campi-separati-o-testo.md";
        assert!(is_handoff_doc(p, Some(&handoff_body(""))));
    }

    #[test]
    fn una_nota_di_progetto_non_arma_niente() {
        // La difesa che la data garantiva prima, ora la fa la forma del corpo:
        // `type: project` senza le due sezioni resta una nota, non una consegna.
        let p = "/p/memory/orca-usciere-tab.md";
        assert!(!is_handoff_doc(
            p,
            Some("---\ntype: project\n---\n\n## Come funziona\n\nchiude le tab.\n")
        ));
        // Una sezione sola non basta: «Stato» da solo sta in mezze note.
        assert!(!is_handoff_doc(
            p,
            Some("---\ntype: project\n---\n\n## Stato\n\nva bene.\n")
        ));
    }

    #[test]
    fn il_frontmatter_vale_solo_in_testa() {
        // Una consegna che parla di consegne cita `type: project` nel corpo: se
        // valesse ovunque, ogni documento sui ganci diventerebbe una consegna.
        let p = "/p/memory/note-sui-ganci.md";
        let corpo = format!("{}\n\n## Stato\n\nx\n\n## Prossimi passi\n\ny\n", "-".repeat(420));
        assert!(!is_handoff_doc(p, Some(&format!("---\ntype: reference\n---{corpo}\ntype: project\n"))));
    }

    #[test]
    fn un_frontmatter_lungo_resta_frontmatter() {
        // Il caso vero del 17/08/2026: la `description` cresce, `type: project`
        // scivola al byte 419, e con la finestra fissa da 400 la consegna
        // smetteva di essere riconosciuta — un'ora dopo averla fatta riconoscere.
        let p = "/p/memory/sezioni-cv-campi-separati-o-testo.md";
        let testo = format!(
            "---\nname: x\ndescription: \"{}\"\nmetadata:\n  node_type: memory\n  type: project\n---\n\n## Stato\n\nx\n\n## Prossimi passi\n\ny\n",
            "parole ".repeat(60)
        );
        assert!(testo.find("type: project").unwrap() > 400, "il caso non riproduce");
        assert!(is_handoff_doc(p, Some(&testo)));
    }

    #[test]
    fn senza_frontmatter_non_si_dichiara_niente() {
        let p = "/p/memory/nudo.md";
        // Nessun `---` in apertura: non c'è nessun blocco da leggere.
        assert!(!is_handoff_doc(p, Some("type: project\n\n## Stato\n\n## Prossimi passi\n")));
        // Aperto e mai chiuso: si legge fino al tetto e non si scorre il file.
        let mai_chiuso = format!("---\n{}\ntype: project\n", "x\n".repeat(4000));
        assert!(!is_handoff_doc(p, Some(&mai_chiuso)));
    }

    #[test]
    fn fuori_da_memory_il_corpo_non_conta() {
        // Le sezioni giuste in un file qualunque del repo non aprono niente.
        assert!(!is_handoff_doc("/repo/docs/piano.md", Some(&handoff_body(""))));
    }

    #[test]
    fn l_indice_non_e_mai_una_consegna() {
        // MEMORY.md porta `type: project` in ogni riga che cita, e senza questa
        // esclusione ogni aggiornamento dell'indice aprirebbe una scheda — che è
        // metà del difetto delle due schede del 17/08/2026.
        assert!(!is_handoff_doc(
            "/p/memory/MEMORY.md",
            Some("type: project")
        ));
    }

    #[test]
    fn l_impronta_dipende_dalla_sessione_e_non_dal_file() {
        // Il difetto del 17/08: due documenti nello stesso turno, due schede.
        let a = armed_fingerprint("/x/consegna.md", "sessione-A");
        let b = armed_fingerprint("/x/MEMORY.md", "sessione-A");
        assert_eq!(a, b);
        assert_ne!(a, armed_fingerprint("/x/consegna.md", "sessione-B"));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn senza_sessione_si_ricade_sul_percorso() {
        // Un payload senza identificativo torna al comportamento stretto, che è
        // il verso giusto in cui sbagliare.
        let a = armed_fingerprint("/x/consegna.md", "");
        assert_ne!(a, armed_fingerprint("/x/altra.md", ""));
    }

    // --- recalculate_fingerprint_owner: la prova a due bracci del capitano,
    // 21/08/2026 15:55 — un marcatore di una sessione viva sopravvive, uno di
    // una morta no, e un elenco non leggibile protegge tutti e due.

    #[test]
    fn arm_one_a_live_session_reproduces_its_own_fingerprint() {
        let live = "11112222-3333-4444-5555-666677778888".to_string();
        let hex = armed_fingerprint("/x/consegna.md", &live);
        assert_eq!(
            recalculate_fingerprint_owner(&hex, "/x/consegna.md", Some(&[live])),
            FingerprintOwner::Alive
        );
    }

    #[test]
    fn arm_two_no_live_session_reproduces_a_dead_ones_fingerprint() {
        let dead = "99998888-7777-6666-5555-444433332222";
        let hex = armed_fingerprint("/x/consegna.md", dead);
        let live = vec!["11112222-3333-4444-5555-666677778888".to_string()];
        assert_eq!(
            recalculate_fingerprint_owner(&hex, "/x/consegna.md", Some(&live)),
            FingerprintOwner::Orphan
        );
    }

    #[test]
    fn an_unreadable_list_protects_even_what_looks_orphaned() {
        // Lo stesso caso del secondo braccio, ma l'elenco non si è potuto
        // leggere: il terzo esito vince su qualunque impronta.
        let dead = "99998888-7777-6666-5555-444433332222";
        let hex = armed_fingerprint("/x/consegna.md", dead);
        assert_eq!(
            recalculate_fingerprint_owner(&hex, "/x/consegna.md", None),
            FingerprintOwner::Unknown
        );
    }

}
