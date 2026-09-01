//! Con quale identità un processo esterno è partito.
//!
//! **PERCHÉ UN TIPO E NON UNA STRINGA.** Fino al 01/09/2026 la riga di una
//! chiamata portava un campo di testo che valeva `<riga di comando>/<profilo>`
//! quando un profilo era in forza, e la stringa vuota in tutti gli altri casi.
//! Gli altri casi erano cinque, e non sono la stessa cosa: un comando che non è
//! un motore conosciuto; nessun profilo in forza, cioè la casa di chi ha aperto
//! il terminale, che è un'identità vera e nominabile; uno stato che nomina un
//! profilo sparito; una riga di comando che la casa non la sposta con una
//! variabile; e un passo consegnato a un agente vivo, di cui Sailor non sa
//! niente. **Cinque fatti diversi, un solo vuoto.**
//!
//! È la stessa regola che questo albero applica già al costo e ai token: *ciò
//! che non è una misura non diventa uno zero*. Qui: **ciò che non è un'assenza
//! non diventa una stringa vuota.**
//!
//! **E C'ERA UN SESTO CASO, CHE ERA UNA BUGIA.** Un passo che scrive da sé la
//! variabile di casa vince — è la decisione, e non si cambia — ma la riga
//! continuava a nominare il profilo attivo. Il registro diceva un'identità e il
//! processo ne aveva usata un'altra, proprio nel caso in cui qualcuno l'aveva
//! cambiata apposta: cioè esattamente quello che una diagnostica o un controllo
//! di sicurezza esiste per vedere.
//!
//! **IL GETTONE NON ENTRA QUI.** Nessuna variante porta credenziali, e nessuna
//! deve portarne: quello che serve a chi guarda è **quale casa** e **come è
//! stata scelta**. Il percorso è il fondo su cui una diagnostica si appoggia;
//! il contenuto di quella casa non è affare di una riga di registro.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Con quale identità è partito il processo di cui questa riga parla.
///
/// Ogni variante risponde a due domande insieme — **quale casa** e **come è
/// stata scelta** — perché separarle vorrebbe dire poter scrivere un percorso
/// giusto con una ragione sbagliata, e nessuno se ne accorgerebbe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngineIdentity {
    /// Un profilo era in forza e la sua casa è stata messa nell'ambiente.
    ProfileInForce {
        cli_id: String,
        profile_name: String,
        /// **IL DATO CHE VALE.** Il nome di un profilo si può riusare, spostare
        /// o cancellare; il percorso è ciò su cui si va a guardare quando
        /// qualcosa è andato storto.
        home_dir: PathBuf,
    },
    /// Il passo ha scritto da sé la variabile di casa, e vince lui.
    ///
    /// **NON È UN GUASTO: È LA DECISIONE.** Chi scrive una variabile dentro un
    /// passo sta dicendo qualcosa di preciso su *quella* chiamata, e non deve
    /// poter essere scavalcato da uno stato che vive altrove. Il guasto era
    /// tacerlo — registrare il nome del profilo attivo mentre il processo
    /// partiva da un'altra parte.
    ChosenByTheStep { cli_id: String, home_dir: PathBuf },
    /// Nessun profilo in forza per questa riga di comando: il processo parte con
    /// la casa di chi ha aperto il terminale.
    ///
    /// **«EREDITATA» NON È «IGNOTA».** È un'identità vera, e dirlo è più utile
    /// che tacere: chi legge sa che quella chiamata ha usato le credenziali
    /// della macchina, non quelle di un profilo. Il percorso non c'è perché
    /// questa decisione si prende senza guardare l'ambiente né il disco, e
    /// dedurlo dal nome della riga di comando sarebbe indovinarlo.
    InheritedFromTheTerminal { cli_id: String },
    /// Lo stato nomina un profilo che l'elenco non ha più.
    ///
    /// Il processo è partito con la casa ereditata, come sopra — ma la ragione è
    /// un'altra, e questa è la sola che chiede di intervenire: c'è uno stato da
    /// riparare. Comporre il percorso dal nome darebbe una casa vuota con l'aria
    /// di un profilo applicato.
    ProfileVanished { cli_id: String, profile_name: String },
    /// Un profilo c'è, ma questa riga di comando non sposta la casa con una
    /// variabile: l'identità dipende da dove punta un file sul disco.
    NotMovedByAnEnvVar {
        cli_id: String,
        profile_name: String,
        /// Perché non è stata messa in forza, con le parole del meccanismo.
        why: String,
    },
    /// Il binario non è una riga di comando che Sailor conosce — `sh`, uno
    /// script: non c'è nessuna casa da spostare, e dargliene una non vorrebbe
    /// dire niente.
    NotAKnownEngine,
    /// Un passo consegnato: a lavorare è stato l'agente già vivo nel terminale.
    ///
    /// Sailor non ha avviato niente e non sa con quale identità quell'agente
    /// abbia lavorato. Scriverne una qualunque sarebbe inventarla; tacere la
    /// confonderebbe con «nessun profilo».
    DeclaredByAnAgent,
    /// La riga viene da prima che Sailor registrasse l'identità.
    ///
    /// `legacy` è il testo che la vecchia colonna portava — `<cli>/<profilo>`
    /// oppure vuoto. **Non si promuove a un profilo dichiarato**, perché quella
    /// colonna nominava il profilo attivo anche quando il passo l'aveva
    /// scavalcato: era già capace di mentire, e trasformarla adesso in una
    /// dichiarazione strutturata darebbe a una bugia vecchia la faccia di una
    /// misura nuova.
    Unrecorded {
        #[serde(default)]
        legacy: String,
    },
}

impl Default for EngineIdentity {
    fn default() -> Self {
        Self::Unrecorded {
            legacy: String::new(),
        }
    }
}

impl EngineIdentity {
    /// La riga di comando di cui si parla, quando se ne conosce una.
    pub fn cli_id(&self) -> Option<&str> {
        match self {
            Self::ProfileInForce { cli_id, .. }
            | Self::ChosenByTheStep { cli_id, .. }
            | Self::InheritedFromTheTerminal { cli_id }
            | Self::ProfileVanished { cli_id, .. }
            | Self::NotMovedByAnEnvVar { cli_id, .. } => Some(cli_id),
            Self::NotAKnownEngine | Self::DeclaredByAnAgent | Self::Unrecorded { .. } => None,
        }
    }

    /// La casa con cui il processo è partito, quando Sailor l'ha decisa lui.
    ///
    /// `None` non vuol dire «non c'era una casa»: vuol dire che questa riga non
    /// la conosce, perché a sceglierla è stato qualcun altro — l'ambiente di chi
    /// ha aperto il terminale, un collegamento simbolico sul disco, un agente.
    pub fn home_dir(&self) -> Option<&std::path::Path> {
        match self {
            Self::ProfileInForce { home_dir, .. } | Self::ChosenByTheStep { home_dir, .. } => {
                Some(home_dir)
            }
            _ => None,
        }
    }

    /// Come si scrive in una colonna di testo.
    pub fn to_column(&self) -> String {
        // Un tipo con `Serialize` non fallisce a serializzarsi: se mai
        // succedesse, la colonna resta vuota e si rilegge come «non registrata»,
        // che è la sola cosa vera che si possa dire di una riga il cui campo non
        // si è potuto scrivere.
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Come si rilegge da una colonna di testo.
    ///
    /// **UN TESTO CHE NON È IL NOSTRO JSON NON SI BUTTA VIA.** Le righe scritte
    /// prima del 01/09/2026 portano lì `<cli>/<profilo>` o niente: diventano
    /// [`EngineIdentity::Unrecorded`] con quel testo dentro, che è l'unico
    /// indizio che quella riga ha.
    pub fn from_column(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_else(|_| Self::Unrecorded {
            legacy: text.to_owned(),
        })
    }
}

/// Una riga per una persona: prima **come** è stata scelta, poi **quale casa**.
impl fmt::Display for EngineIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProfileInForce {
                cli_id,
                profile_name,
                home_dir,
            } => write!(
                formatter,
                "profilo {cli_id}/{profile_name} — casa {}",
                home_dir.display()
            ),
            Self::ChosenByTheStep { cli_id, home_dir } => write!(
                formatter,
                "casa scelta dal passo ({cli_id}) — casa {}",
                home_dir.display()
            ),
            Self::InheritedFromTheTerminal { cli_id } => write!(
                formatter,
                "identità ereditata da chi ha aperto il terminale ({cli_id}): nessun profilo in forza"
            ),
            Self::ProfileVanished {
                cli_id,
                profile_name,
            } => write!(
                formatter,
                "identità ereditata ({cli_id}): lo stato nomina il profilo «{profile_name}», che non esiste più"
            ),
            Self::NotMovedByAnEnvVar {
                cli_id,
                profile_name,
                why,
            } => write!(
                formatter,
                "profilo {cli_id}/{profile_name} non messo in forza: {why}"
            ),
            Self::NotAKnownEngine => {
                write!(formatter, "non è una riga di comando che Sailor conosce")
            }
            Self::DeclaredByAnAgent => write!(
                formatter,
                "dichiarata da un agente: identità non nota a Sailor"
            ),
            Self::Unrecorded { legacy } if legacy.is_empty() => {
                write!(formatter, "non registrata")
            }
            Self::Unrecorded { legacy } => write!(
                formatter,
                "non registrata (la vecchia colonna diceva «{legacy}»)"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **QUELLO CHE VA NELLA COLONNA TORNA INDIETRO TALE E QUALE.** Senza,
    /// l'identità sarebbe scritta e riletta diversa, e nessuno se ne
    /// accorgerebbe finché una diagnostica non guardasse la riga sbagliata.
    #[test]
    fn every_shape_survives_the_column() {
        for identity in every_shape() {
            assert_eq!(
                EngineIdentity::from_column(&identity.to_column()),
                identity,
                "questa forma non sopravvive al giro nel deposito"
            );
        }
    }

    /// Il testo di una colonna vecchia non si butta via e non si promuove.
    #[test]
    fn an_old_column_becomes_unrecorded_with_its_text_kept() {
        assert_eq!(
            EngineIdentity::from_column("codex/lavoro"),
            EngineIdentity::Unrecorded {
                legacy: "codex/lavoro".to_owned()
            }
        );
        assert_eq!(
            EngineIdentity::from_column(""),
            EngineIdentity::Unrecorded {
                legacy: String::new()
            }
        );
    }

    /// **OGNI FORMA SI LEGGE DIVERSA DALLE ALTRE.** È la cura del difetto: se
    /// due fatti diversi producessero la stessa riga per chi guarda, il tipo
    /// avrebbe solo spostato il vuoto più in là.
    #[test]
    fn no_two_shapes_read_the_same() {
        let said: Vec<String> = every_shape().iter().map(ToString::to_string).collect();
        for (position, one) in said.iter().enumerate() {
            for other in said.iter().skip(position + 1) {
                assert_ne!(one, other, "due fatti diversi si leggono uguali");
            }
        }
    }

    fn every_shape() -> Vec<EngineIdentity> {
        vec![
            EngineIdentity::ProfileInForce {
                cli_id: "codex".to_owned(),
                profile_name: "lavoro".to_owned(),
                home_dir: PathBuf::from("/case/codex/lavoro"),
            },
            EngineIdentity::ChosenByTheStep {
                cli_id: "codex".to_owned(),
                home_dir: PathBuf::from("/una/casa/del/passo"),
            },
            EngineIdentity::InheritedFromTheTerminal {
                cli_id: "codex".to_owned(),
            },
            EngineIdentity::ProfileVanished {
                cli_id: "codex".to_owned(),
                profile_name: "sparito".to_owned(),
            },
            EngineIdentity::NotMovedByAnEnvVar {
                cli_id: "antigravity".to_owned(),
                profile_name: "lavoro".to_owned(),
                why: "nessuna variabile nota".to_owned(),
            },
            EngineIdentity::NotAKnownEngine,
            EngineIdentity::DeclaredByAnAgent,
            EngineIdentity::Unrecorded {
                legacy: String::new(),
            },
            EngineIdentity::Unrecorded {
                legacy: "codex/lavoro".to_owned(),
            },
        ]
    }
}
