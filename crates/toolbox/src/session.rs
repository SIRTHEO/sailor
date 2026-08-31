//! Quali motori sanno riprendere una sessione, e con quali opzioni.
//!
//! **PERCHÉ UN FILE A SÉ E NON UN CAMPO DEI DESCRITTORI.** Il 31/08/2026 un
//! altro lavoro stava estendendo `descriptors/default.json` e
//! `src/descriptor.rs` in un albero separato. Due mani sullo stesso file JSON
//! non danno un conflitto da leggere: danno un file che si carica, non dice
//! quello che una delle due credeva, e nessun compilatore lo mostra. Un file
//! separato costa una lettura in più all'avvio e non può collidere con niente.
//!
//! **E RESTA UN DATO, NON UN RAMO `if`.** Non compare nessun nome di motore in
//! questo modulo: chi ne aggiunge uno scrive una voce, e chi non ce l'ha
//! continua a funzionare ripartendo da zero. È il vincolo permanente
//! «indipendenza dal modello»: una capacità che vale su un motore solo si
//! dichiara come capacità di quel motore.
//!
//! **PERCHÉ OGNI MODO PORTA LA RIGA INTERA.** Riprendere non è sempre
//! un'opzione in più. Verificato il 31/08/2026 su questa macchina, con
//! `--help`:
//!
//! | motore | apre con un identificativo nostro | riprende | ramifica |
//! |---|---|---|---|
//! | `claude` | `--session-id <uuid>` | `--resume <id>` | `--resume <id> --fork-session` |
//! | `codex` | no | `exec resume <id>` (sottocomando) | `exec fork <id>` (sottocomando) |
//! | `gemini` | `--session-id <uuid>` | `--resume` vuole «latest» o un **indice**, non un identificativo | no |
//! | `agy` | no | `--conversation <id>` | no |
//!
//! Due dei quattro cambiano **sottocomando**, non opzione: un modello «aggiungi
//! queste opzioni» li escluderebbe entrambi.
//!
//! **COSA È SPEDITO E COSA NO, E PERCHÉ.** Spedito: `claude-code`, l'unico dei
//! quattro che chiude il giro con un identificativo che scegliamo noi.
//! `codex` sa riprendere e ramificare ma conia l'identificativo da sé: per
//! sapere quale sia bisognerebbe leggerlo dalla sua uscita, e l'unico modo che
//! `codex exec --help` offre è `--json`, che ne cambia il formato — cioè
//! romperebbe la lettura del consumo che quel descrittore già dichiara. `agy`
//! ha lo stesso problema senza nemmeno una via. `gemini` sa nascere con un
//! identificativo nostro ma non sa riprendere per identificativo. Tutti e tre
//! ripartono da zero e pagano di più: è il comportamento dichiarato, non un
//! guasto.

use serde::Deserialize;
use std::path::PathBuf;

/// Le capacità spedite dentro il binario, per la stessa ragione per cui ci
/// stanno i flussi di sistema: un file accanto al programma può mancare, e
/// allora il prodotto si comporta diversamente su macchine diverse senza che si
/// capisca perché.
const BUILT_IN: &str = include_str!("../descriptors/sessions.json");

/// Dove chi usa Sailor mette le proprie, senza ricompilare niente.
const USER_FILE: &str = "sessions.json";

/// Cosa un motore sa fare con le proprie sessioni, come sta scritto nel file.
///
/// Ogni modo è la riga di comando **intera** con cui si interroga quel motore
/// in quel modo — cioè ciò che prende il posto delle opzioni della domanda —
/// con `{session}` dove va l'identificativo.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionAbility {
    /// L'identificativo dello strumento, lo stesso dei descrittori.
    pub tool: String,
    /// Come si apre una sessione **con un identificativo scelto da noi**. Chi
    /// non lo lascia scegliere non ha questa voce: non c'è modo di ritrovare
    /// una sessione di cui non si conosce il nome.
    #[serde(default)]
    pub open: Option<Vec<String>>,
    #[serde(default)]
    pub resume: Option<Vec<String>>,
    #[serde(default)]
    pub fork: Option<Vec<String>>,
    /// Dove il motore dice **di quale sessione** ha appena parlato. Serve a chi
    /// l'identificativo se lo conia da sé, che è la maggioranza.
    #[serde(default)]
    pub id_from: Option<IdPlace>,
}

/// Dove sta scritto l'identificativo dentro ciò che il motore ha detto.
///
/// **DUE FORME E BASTA, ED È UNA COPIA DELIBERATA.** `toolbox::descriptor` ha
/// già un tipo che dice la stessa cosa per i numeri del consumo; usarlo qui
/// legherebbe questo file a quello, che un altro lavoro sta modificando nello
/// stesso giorno. Due righe di copia costano meno di un file conteso.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdPlace {
    /// Un'espressione regolare col identificativo nel primo gruppo, per i
    /// motori che parlano in chiaro.
    Pattern(String),
    /// Un cammino di chiavi, per quelli che rispondono in JSON.
    Path(Vec<String>),
}

impl IdPlace {
    fn pointer(&self) -> actions::Pointer {
        match self {
            IdPlace::Pattern(pattern) => actions::Pointer::Pattern(pattern.clone()),
            IdPlace::Path(keys) => actions::Pointer::Path(keys.clone()),
        }
    }
}

/// Le capacità che questa macchina conosce, spedite più quelle dell'utente.
#[derive(Debug, Clone, Default)]
pub struct SessionAbilities {
    entries: Vec<SessionAbility>,
}

impl SessionAbilities {
    /// Quelle spedite, più il file dell'utente se c'è.
    ///
    /// **UN FILE DELL'UTENTE SCRITTO MALE NON FERMA NIENTE**, e non è
    /// indulgenza: l'unica conseguenza di ignorarlo è che quei motori
    /// ripartono da zero, cioè si torna esattamente a come funzionava prima.
    /// Rompere l'avvio di Sailor per un JSON storto in un file facoltativo
    /// sarebbe un danno molto più grande di quello che evita.
    pub fn current() -> Self {
        let mut abilities = Self::shipped();
        if let Some(path) = user_file() {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(theirs) = serde_json::from_str::<Vec<SessionAbility>>(&text) {
                    abilities.absorb(theirs);
                }
            }
        }
        abilities
    }

    /// Solo quelle spedite dentro il binario.
    pub fn shipped() -> Self {
        Self {
            entries: serde_json::from_str(BUILT_IN)
                .expect("le capacità spedite sono un dato di questo repository"),
        }
    }

    /// Un elenco deciso da chi chiama: è così che una prova verifica la
    /// traduzione senza dipendere da cosa è spedito.
    pub fn of(entries: Vec<SessionAbility>) -> Self {
        Self { entries }
    }

    /// **L'UTENTE VINCE, VOCE PER VOCE.** Stessa regola dei descrittori: chi
    /// scrive in casa propria una voce per uno strumento che spediamo sta
    /// dicendo che sul suo motore le opzioni sono altre — magari perché ha una
    /// versione diversa — e la sua è quella giusta lì.
    fn absorb(&mut self, theirs: Vec<SessionAbility>) {
        for ability in theirs {
            self.entries.retain(|mine| mine.tool != ability.tool);
            self.entries.push(ability);
        }
    }

    /// Cosa sa fare lo strumento con questo identificativo. `None` per chi non
    /// è dichiarato, che è il caso di quasi tutti.
    pub fn for_tool(&self, id: &str) -> Option<actions::SessionRecipe> {
        let ability = self.entries.iter().find(|entry| entry.tool == id)?;
        Some(actions::SessionRecipe {
            open: ability.open.clone(),
            resume: ability.resume.clone(),
            fork: ability.fork.clone(),
            id_from: ability.id_from.as_ref().map(IdPlace::pointer),
        })
    }
}

fn user_file() -> Option<PathBuf> {
    Some(ledger::sailor_home()?.join(USER_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **QUELLO CHE SI SPEDISCE DEVE ESSERE VERO SULLA MACCHINA DOVE GIRA.**
    /// Le tre righe di `claude-code` non sono un'ipotesi: vengono da
    /// `claude --help` letto il 31/08/2026. Questa prova non può verificare
    /// l'aiuto di un binario che potrebbe non esserci, ma tiene ferma la forma
    /// — se qualcuno toglie `--fork-session` dalla voce «fork», la ramificazione
    /// diventa una ripresa e i tre passi paralleli si scriverebbero addosso.
    #[test]
    fn the_shipped_engine_declares_all_three_moves() {
        let shipped = SessionAbilities::shipped();

        let recipe = shipped.for_tool("claude-code").expect("è spedito");

        assert_eq!(
            recipe.open.as_deref(),
            Some(["-p", "--session-id", "{session}"].map(str::to_owned).as_slice())
        );
        assert_eq!(
            recipe.resume.as_deref(),
            Some(["-p", "--resume", "{session}"].map(str::to_owned).as_slice())
        );
        assert_eq!(
            recipe.fork.as_deref(),
            Some(
                ["-p", "--resume", "{session}", "--fork-session"]
                    .map(str::to_owned)
                    .as_slice()
            )
        );
    }

    /// Un motore che non è dichiarato non diventa capace per indovinamento.
    #[test]
    fn an_engine_nobody_declared_can_do_nothing() {
        assert!(SessionAbilities::shipped().for_tool("agy").is_none());
    }

    /// **CHI NON LASCIA SCEGLIERE IL NOME DEVE DIRE DOVE LO SCRIVE.** Una voce
    /// che apre una sessione senza segnaposto e senza `id_from` aprirebbe una
    /// conversazione che nessuno potrà ritrovare: il passo dopo riprenderebbe
    /// il nulla, e se ne accorgerebbe dopo aver speso. Verificato il 31/08/2026
    /// contro `codex exec --help` e contro una corsa vera: `codex` non ha
    /// nessuna opzione per imporre un identificativo, e lo stampa.
    #[test]
    fn an_engine_that_mints_its_own_name_declares_where_it_writes_it() {
        for ability in SessionAbilities::shipped().entries {
            let Some(open) = &ability.open else { continue };
            let ours = open.iter().any(|arg| arg.contains(actions::SESSION_PLACEHOLDER));
            assert!(
                ours || ability.id_from.is_some(),
                "«{}» apre una sessione che nessuno potrà ritrovare",
                ability.tool
            );
        }
    }

    /// L'espressione di `codex` deve riconoscere la sua uscita vera, non una
    /// che le somiglia: se fosse scritta male l'identificativo resterebbe
    /// ignoto per sempre, e nessuno se ne accorgerebbe. Il testo qui sotto è
    /// copiato da una corsa del 31/08/2026.
    #[test]
    fn the_shipped_pattern_finds_the_identifier_in_the_real_output() {
        let recipe = SessionAbilities::shipped()
            .for_tool("codex")
            .expect("codex è spedito");
        let pointer = recipe.id_from.expect("e dichiara dove scrive il proprio nome");
        let said = "model: gpt-5.6-sol\nprovider: openai\napproval: never\n\
                    session id: 01a057e8-f849-79c1-84f8-9de1f4f758b8\n--------\nuser\n";

        assert_eq!(
            actions::read_text(said, &pointer).as_deref(),
            Some("01a057e8-f849-79c1-84f8-9de1f4f758b8")
        );
    }

    /// Un modo non dichiarato resta assente **mentre gli altri due funzionano**:
    /// è il caso di un motore che sa riprendere e non sa ramificare, e senza
    /// questa asimmetria dovrebbe rinunciare anche a ciò che sa fare.
    #[test]
    fn a_move_that_is_not_declared_stays_absent_without_taking_the_others_with_it() {
        let abilities = SessionAbilities::of(vec![SessionAbility {
            tool: "solo-resume".to_owned(),
            open: None,
            resume: Some(vec!["--conversation".to_owned(), "{session}".to_owned()]),
            fork: None,
            id_from: None,
        }]);

        let recipe = abilities.for_tool("solo-resume").expect("è dichiarato");

        assert!(recipe.open.is_none());
        assert!(recipe.fork.is_none());
        assert_eq!(recipe.resume.expect("questo sì").len(), 2);
    }
}
