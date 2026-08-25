//! Il braccio della freschezza delle memorie: dove stanno le consegne, quali
//! sessioni sono ancora vive, chi scrive il rilievo dentro il documento.
//!
//! Il giudizio — cos'è una sezione operativa, quando una consegna è superata,
//! com'è fatto il blocco — sta in `guards::handoff_freshness`, che non tocca il
//! disco. Qui c'è solo il mondo: le cartelle, la lettura, la riscrittura, il
//! conteggio finale.
//!
//! LA PARENTELA, dichiarata perché non nasca un terzo meccanismo che risponde
//! alla stessa domanda: `queue-freshness` fa questo sulle voci di coda, e il
//! gesto comune — togliere e rimettere un blocco senza lasciare sedimento — è
//! uno solo, in `guards::regen_block`. Quello che cambia è il corpus e la
//! posizione: là il rilievo sta sotto il frontmatter perché un programma legge
//! `stato:`, qui sta **sopra la sezione operativa** perché è lì che guarda chi
//! sta per eseguire un piano vecchio.
//!
//! Uso:
//!     claude-hooks memory-freshness           racconta e non scrive niente
//!     claude-hooks memory-freshness --mark    scrive il rilievo nelle consegne
//!     claude-hooks memory-freshness --armed   le memorie di fatto ancora armate
//!
//! NON AGGIUNGE UN BYTE AL PROLOGO, come il suo gemello: il rilievo vive dentro
//! il documento, e fuori esce una riga sola per chi lancia il comando.
//!
//! Uscita: 0 sempre, tranne quando una scrittura viene negata — è l'unico caso
//! in cui chi lancia deve accorgersene senza leggere l'uscita a occhio.

use guards::handoff_freshness::{
    block_body, collects, is_handoff, operative_line, with_block, written, Freshness,
};
use guards::stale_facts::Date;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Le cartelle di memorie, prese da chi già le percorre.
///
/// Restano fuori due cose, e nessuna delle due per caso:
/// - **la cartella della coda**, che ha il suo rilievo con la sua etichetta e la
///   sua posizione: marcarla due volte metterebbe due avvisi diversi sullo
///   stesso testo;
/// - **le sottocartelle `archive/`**, perché archiviare è già la marcatura. Ne
///   escono due consegne sul parco del 24/08/2026 — 80 col selettore che guarda
///   solo il nome, 78 qui — e una delle due è l'unica dell'intero parco che
///   qualcuno avesse dichiarato di raccogliere.
fn memory_dirs() -> Vec<PathBuf> {
    crate::memory_anchors::memory_dirs()
        .into_iter()
        .filter(|d| d.file_name().is_some_and(|n| n == "memory"))
        .collect()
}

/// Le sessioni che risultano ancora vive, per firma di otto caratteri — oppure
/// **niente**, quando la domanda non ha avuto risposta.
///
/// `state/sessioni-vive/<firma>.json` lo scrive `register-session` all'avvio e
/// lo toglie il raccoglitore quando la sessione muore. UN FILE CHE RESTA È
/// PRUDENTE: una sessione morta senza pulizia risulta viva, e la sua consegna
/// non viene marcata. Il verso opposto — marcare il piano di chi sta lavorando
/// adesso — sarebbe il danno vero.
///
/// PER QUESTO UNA CARTELLA CHE NON RISPONDE NON È UN ELENCO VUOTO. Le due cose
/// si somigliano e portano a conclusioni opposte: senza questa distinzione, una
/// cartella di stato assente — o negata dal perimetro, o un `HOME` che non
/// arriva — fa risultare **chiusa ogni sessione del parco**, e il rilievo va a
/// scrivere «la sessione che ha scritto questa consegna è chiusa» sopra il piano
/// di chi sta lavorando. Misurato il 25/08/2026 da un revisore su una consegna
/// di una sessione **viva**: con la cartella al suo posto «0 superate», con
/// `HOME` spostata «1 superate» e il testo già scritto dentro il file.
///
/// Il rimedio non è nuovo in questa casa: `marker_sweep` risponde `None` invece
/// di una lista vuota per la stessa ragione, dal 21/08/2026.
fn live_sessions() -> Option<BTreeSet<String>> {
    let dir = home().join(".claude/state/sessioni-vive");
    let entries = std::fs::read_dir(dir).ok()?;
    Some(
        entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.strip_suffix(".json").map(str::to_string)
            })
            .collect(),
    )
}

/// Il rilievo «la sessione che ha scritto questa consegna è chiusa», oppure
/// **niente** quando la domanda non ha avuto risposta.
///
/// UN VALORE SOLO GOVERNA TUTTO CIÒ CHE SEGUE, e non è una finezza di forma. La
/// cecità tocca due cose — il rilievo che non si dà e il file che non si tocca —
/// e finché erano due espressioni separate sullo stesso fatto potevano
/// divergere: il 25/08/2026 un revisore ha rimesso il difetto **nel punto d'uso**
/// lasciando corretta la funzione che legge la cartella, e la batteria è rimasta
/// verde mentre il binario tornava a scrivere «la tua sessione è chiusa» sopra il
/// piano di chi lavorava. Da qui in poi chi vuole rimetterlo deve passare da qui.
fn session_closed(live: &Option<BTreeSet<String>>, text: &str) -> Option<bool> {
    let live = live.as_ref()?;
    Some(guards::handoff_freshness::origin_session(text).is_some_and(|s| !live.contains(&s)))
}

/// La data di oggi dall'orologio locale, come la legge `stale-facts`.
fn today() -> Option<Date> {
    let stamp = hook_io::local_time::now_local_iso8601();
    let n = |a: usize, b: usize| stamp.get(a..b).and_then(|s| s.parse::<i64>().ok());
    Date::new(n(0, 4)?, n(5, 7)?, n(8, 10)?)
}

/// Una memoria letta dal disco.
struct Memory {
    path: PathBuf,
    name: String,
    text: String,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn read_all() -> Vec<Memory> {
    let mut out = Vec::new();
    for dir in memory_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_some_and(|x| x == "md") {
                let name = file_name(&path);
                if name == "MEMORY.md" {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push(Memory { path, name, text });
                }
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Chi ha dichiarato di raccogliere chi.
///
/// Si guarda una riga alla volta perché il verbo e il rimando devono stare
/// nella stessa frase: una consegna che nomina cinque memorie in cinque righe e
/// dice «raccolgo» in una sola non le sta raccogliendo tutte.
fn collectors(memories: &[Memory]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for m in memories {
        for other in memories {
            if other.name == m.name {
                continue;
            }
            let stem = other.name.trim_end_matches(".md");
            if m.text.lines().any(|l| collects(l, stem)) {
                out.insert(other.name.clone(), m.name.clone());
            }
        }
    }
    out
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mark = args.iter().any(|a| a == "--mark");
    let show_armed = args.iter().any(|a| a == "--armed");

    let memories = read_all();
    if memories.is_empty() {
        println!("Nessuna memoria trovata.");
        return 0;
    }
    let live = live_sessions();
    let taken = collectors(&memories);
    let now = today();

    let mut handoffs = 0;
    // Le consegne saltate perché la cartella delle sessioni vive non risponde.
    // Il numero esce dal **conteggio vero**, non da un secondo controllo sullo
    // stesso fatto: un avviso che non deriva dal valore che governa il
    // comportamento è un avviso che sopravvive alla propria verità.
    let mut blind = 0;
    let mut armed_facts: Vec<String> = Vec::new();
    let mut marked = 0;
    let mut saved = 0;
    let mut refused: Vec<String> = Vec::new();

    for m in &memories {
        if operative_line(&m.text).is_none() {
            continue;
        }
        // Una memoria di fatto con una sezione operativa non si marca: si
        // nomina. Il rilievo è la cura per una consegna, che quella sezione ha
        // il diritto di averla; su una memoria di fatto la cura è toglierla, ed
        // è un giudizio di merito che nessun comando può prendere da solo.
        if !is_handoff(&m.name) {
            armed_facts.push(m.name.clone());
            continue;
        }
        handoffs += 1;

        // La data di scrittura viene dal NOME, non da `metadata.modified`: il
        // 24/08/2026 il rilievo ha scritto «questa consegna è del 24/08» su una
        // consegna del 21, perché quel giorno qualcuno l'aveva corretta e il
        // campo del frontmatter era diventato l'ora della correzione. Il
        // frontmatter resta il ripiego, e in quel caso il rilievo lo dichiara.
        // `now` è già la data di oggi letta più su: serve a dare l'anno ai nomi
        // di consegna che portano il solo giorno e mese.
        let when = written(&m.name, &m.text, now);
        let age = match (now, when) {
            (Some(now), Some(w)) => Some((now.days() - w.date().days(), w)),
            _ => None,
        };
        // Senza risposta dalla cartella delle sessioni vive questa consegna non
        // si tocca affatto: **non si arma e non si disarma**. Il secondo verso è
        // quello che sorprende, e va detto — da cieco il rilievo risulterebbe
        // «niente da segnalare», e la riscrittura andrebbe a **togliere** i
        // rilievi veri già scritti, contandoli come scritture riuscite.
        let Some(session_closed) = session_closed(&live, &m.text) else {
            blind += 1;
            continue;
        };
        let fresh = Freshness {
            age,
            session_closed,
            collector: taken.get(&m.name).cloned(),
        };
        let body = block_body(&fresh);
        if body.is_some() {
            marked += 1;
        }
        if !mark {
            continue;
        }
        let updated = with_block(&m.text, body.as_deref());
        if updated == m.text {
            continue;
        }
        match std::fs::write(&m.path, &updated) {
            Ok(()) => saved += 1,
            // UNA SCRITTURA NEGATA SI DICE, com'è già per la coda: la passata
            // che tace risponde «rilievo scritto in 0 memorie», cioè la stessa
            // riga del caso in cui non c'era niente da scrivere. È il falso
            // verde peggiore.
            Err(e) => refused.push(format!("{}: {e}", m.name)),
        }
    }

    if show_armed {
        for name in &armed_facts {
            println!("armata: {name}");
        }
    }

    println!(
        "{} memorie · {handoffs} consegne con una sezione operativa · {marked} superate \
         (sessione chiusa o raccolte da un'altra) · {} memorie di fatto ancora armate",
        memories.len(),
        armed_facts.len()
    );
    if blind > 0 {
        println!(
            "NON MISURATE: {blind} consegne saltate — la cartella delle sessioni vive non \
             risponde, e senza quella «non lo so» diventerebbe «è chiusa». Non sono state \
             marcate né smarcate."
        );
    }
    if !mark {
        println!("Nessun file toccato: `--mark` scrive il rilievo dentro le consegne.");
        return 0;
    }
    println!("Rilievo scritto dentro {saved} consegne.");
    if let Some(first) = refused.first() {
        println!(
            "SCRITTURA NEGATA su {} consegne — il parco NON è marcato. La prima: {first}",
            refused.len()
        );
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(name: &str, text: &str) -> Memory {
        Memory {
            path: PathBuf::from(name),
            name: name.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn only_a_declared_takeover_makes_a_collector() {
        // Il caso vero del 24/08/2026: su 80 consegne una sola era nominata da
        // un'altra, e le citazioni «vedi anche» sono decine. Se bastasse il
        // rimando, quelle decine diventerebbero altrettanti sorpassi finti.
        let memories = vec![
            memory(
                "consegna-b.md",
                "Raccoglie [[consegna-a]] e chiude quel filone.\nVedi anche [[consegna-c]].\n",
            ),
            memory("consegna-a.md", "# a\n"),
            memory("consegna-c.md", "# c\n"),
        ];
        let taken = collectors(&memories);
        assert_eq!(taken.get("consegna-a.md").map(String::as_str), Some("consegna-b.md"));
        assert!(!taken.contains_key("consegna-c.md"));
        // E nessuno raccoglie sé stesso, nemmeno nominandosi.
        assert!(!taken.contains_key("consegna-b.md"));
    }

    /// UNA CARTELLA CHE NON RISPONDE NON È UN ELENCO VUOTO, e le due risposte
    /// portano a conclusioni opposte: la seconda dichiara chiusa ogni sessione
    /// del parco e va a marcare il piano di chi sta lavorando adesso.
    ///
    /// Il caso gira dentro una `HOME` usa-e-getta perché la funzione legge
    /// l'ambiente: senza, proverebbe la cartella vera di questa macchina, e il
    /// verdetto cambierebbe col giorno.
    #[test]
    fn a_directory_that_does_not_answer_is_not_an_empty_list() {
        let home = crate::test_home::HomeIsolata::nuova("freschezza-sessioni-vive");
        assert!(
            live_sessions().is_none(),
            "senza la cartella la risposta è «non lo so»"
        );

        let dir = home.stato().join("sessioni-vive");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            live_sessions(),
            Some(BTreeSet::new()),
            "una cartella vuota è un elenco vuoto: nessuno è vivo, e lo sappiamo"
        );

        std::fs::write(dir.join("a28f125e.json"), "{}").unwrap();
        std::fs::write(dir.join("non-una-sessione.txt"), "x").unwrap();
        let live = live_sessions().expect("la cartella c'è");
        assert_eq!(live.len(), 1, "{live:?}");
        assert!(live.contains("a28f125e"));
    }

    /// LA DECISIONE STA IN UN PUNTO SOLO, e questo caso la prova lì. Non basta
    /// che la cartella sappia dire «non lo so»: il 25/08/2026 un revisore ha
    /// rimesso il difetto **nel punto d'uso** — un `unwrap_or_default()` sulla
    /// risposta — e la batteria è rimasta verde mentre il binario tornava a
    /// marcare la consegna di una sessione viva.
    ///
    /// Il terzo stato è quello che conta: `None` non è «non è chiusa», è «non si
    /// tocca». Chi lo confonde con `false` fa la stessa cosa in un verso solo —
    /// e da cieco arriva a **togliere** i rilievi veri già scritti.
    #[test]
    fn without_an_answer_the_relief_is_not_given_nor_denied() {
        let text = "---\nmetadata:\n  originSessionId: a28f125e-047b-41f8\n---\n\ncorpo\n";

        assert_eq!(session_closed(&None, text), None, "cieco: non si decide");

        let nobody = Some(BTreeSet::new());
        assert_eq!(
            session_closed(&nobody, text),
            Some(true),
            "cartella vuota: la sessione non c'è più, e lo sappiamo"
        );

        let alive = Some(BTreeSet::from(["a28f125e".to_string()]));
        assert_eq!(session_closed(&alive, text), Some(false));

        // Una memoria senza firma non è di nessuna sessione: non è «chiusa».
        assert_eq!(session_closed(&nobody, "# senza frontmatter\n"), Some(false));
    }
}
