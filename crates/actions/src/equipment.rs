//! What an engine starts with: the environment of the profile in force under
//! the step's own, and the identity the ledger row names for it.

use ledger::EngineIdentity;
use std::collections::BTreeMap;
use std::path::PathBuf;

// ── la dotazione con cui un motore parte ─────────────────────────────────

/// Con che cosa una chiamata a un motore esterno parte davvero.
///
/// **PERCHÉ I DUE CAMPI STANNO INSIEME.** L'ambiente decide *quale casa* quel
/// motore leggerà; il nome del profilo è ciò che finisce nel deposito. Separarli
/// vorrebbe dire risolvere due volte lo stesso profilo e poter sbagliare in un
/// posto solo — cioè scrivere nel deposito una dotazione diversa da quella con
/// cui la chiamata è girata, che è peggio di non scriverla.
pub struct Equipment {
    /// Da sovrapporre all'ambiente ereditato prima di lanciare.
    pub env: BTreeMap<String, String>,
    /// Con quale identità il processo parte: **quale casa** e **come è stata
    /// scelta**. Risponde sempre — non esiste il caso «vuoto».
    pub identity: EngineIdentity,
    /// Why this engine must not start under this profile: an endpoint the
    /// command line cannot be pointed at, or a key the machine lacks.
    pub refused: Option<String>,
}

/// La dotazione per invocare `bin`, secondo lo stato dei profili dato.
///
/// **IL GUASTO 18, ED È LA STESSA MALATTIA DEL 35.** Tutte e due sono «Sailor ha
/// un dato in casa propria e non lo usa». Il listino c'era e non viaggiava col
/// prodotto; la dotazione c'era — `~/.config/sailor/` ha `equipment/`, `flows/`,
/// un listino, una firma — e non arrivava ai motori, perché la sovrapposizione
/// d'ambiente la chiamava solo `sailor run`. Un motore lanciato da un passo di
/// flusso ereditava l'ambiente di chi aveva aperto il terminale: leggeva la casa
/// del vicino, e due corse dello stesso flusso non erano la stessa misura.
///
/// **L'AMBIENTE DEL PROFILO STA SOTTO QUELLO DEL PASSO, E IL VERSO È LA
/// DECISIONE.** Chi scrive una variabile dentro un passo sta dicendo qualcosa di
/// preciso su *quella* chiamata — un profilo diverso per un solo passo, una casa
/// usa-e-getta per una prova — e non deve poter essere scavalcato da uno stato
/// che vive altrove e che quel passo non nomina. Il verso opposto renderebbe la
/// riga scritta nel flusso inerte, in silenzio.
///
/// **PURO: LO STATO ENTRA, LA DOTAZIONE ESCE.** Chi legge il file dei profili sta
/// in [`current_equipment_for`], per la stessa ragione di `price_list_from`.
pub fn equipment_for(
    store: &profiles::ProfileStore,
    bin: &str,
    step_env: &BTreeMap<String, String>,
) -> Equipment {
    equipment_with_keys(store, bin, step_env, &|variable| std::env::var(variable).ok())
}

/// [`equipment_for`] with the machine's key variables read through `key_of`,
/// so a test hands its own.
pub fn equipment_with_keys(
    store: &profiles::ProfileStore,
    bin: &str,
    step_env: &BTreeMap<String, String>,
    key_of: &dyn Fn(&str) -> Option<String>,
) -> Equipment {
    let Some(cli) = profiles::cli_for_executable(bin) else {
        // Un comando qualunque — `sh`, uno script — non ha nessuna casa da
        // spostare, e dargliene una non vorrebbe dire niente.
        return Equipment {
            env: step_env.clone(),
            identity: EngineIdentity::NotAKnownEngine,
            refused: None,
        };
    };
    let named = store.active.get(&cli.id);
    let resolved = named.and_then(|active| {
        store
            .profiles
            .iter()
            // **UNO STATO CHE NOMINA UN PROFILO SPARITO NON INVENTA UNA
            // CARTELLA.** Comporre il percorso dal nome darebbe una casa vuota,
            // cioè senza credenziali, con l'aria di aver applicato un profilo.
            .find(|profile| profile.cli_id == cli.id && &profile.name == active)
    });
    let mut from_the_profile = resolved
        .map(|profile| profiles::build_environment(cli, &profile.home_dir))
        .unwrap_or_default();
    // The endpoint, when the profile declares one: the same overlay, and a
    // refusal instead of a launch when it cannot be pointed there.
    let refused = match resolved.map(|profile| profiles::endpoint_environment(cli, profile, key_of)) {
        Some(Ok(pointed)) => {
            from_the_profile.extend(pointed);
            None
        }
        Some(Err(why)) => Some(why),
        None => None,
    };
    // Il profilo prima, il passo sopra: chi scrive una variabile nel passo vince.
    let mut env = from_the_profile;
    env.extend(
        step_env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    Equipment {
        env,
        identity: identity_of(cli, named.map(String::as_str), resolved, step_env),
        refused,
    }
}

/// Con quale identità questa invocazione parte davvero.
///
/// **IL PASSO SI GUARDA PER PRIMO, ED È LA CURA DEL DIFETTO.** Fino al
/// 01/09/2026 questa decisione era un booleano — «un profilo è stato applicato»
/// — che restava vero anche quando il passo aveva scritto da sé la variabile di
/// casa. Il motore partiva nella casa del passo e la riga nel deposito nominava
/// il profilo attivo: **il registro diceva un'identità e il processo ne aveva
/// usata un'altra**, proprio nel caso in cui qualcuno l'aveva cambiata apposta.
/// L'ordine qui sotto è quello della sovrapposizione vera, non quello dello
/// stato: si registra ciò che accade.
fn identity_of(
    cli: &profiles::KnownCli,
    named: Option<&str>,
    resolved: Option<&profiles::Profile>,
    step_env: &BTreeMap<String, String>,
) -> EngineIdentity {
    let cli_id = cli.id.clone();
    if let profiles::HomeMechanism::EnvVar(variable) = &cli.home {
        if let Some(home) = step_env.get(variable) {
            return EngineIdentity::ChosenByTheStep {
                cli_id,
                home_dir: PathBuf::from(home),
            };
        }
    }
    match (resolved, named) {
        (Some(profile), _) => match &cli.home {
            profiles::HomeMechanism::EnvVar(_) => EngineIdentity::ProfileInForce {
                cli_id,
                profile_name: profile.name.clone(),
                home_dir: profile.home_dir.clone(),
                endpoint: profile.endpoint.as_ref().map(|endpoint| endpoint.url.clone()),
            },
            // **UN PROFILO DICHIARATO NON È UN PROFILO IN FORZA.** Dove la casa
            // si sposta scambiando un collegamento simbolico, o dove non si sa
            // come si sposti, questa funzione non ha messo niente
            // nell'ambiente: l'identità dipende da dove punta un file sul disco,
            // e questo codice il disco non lo tocca.
            _ => EngineIdentity::NotMovedByAnEnvVar {
                cli_id,
                profile_name: profile.name.clone(),
                why: why_it_stays_where_it_is(cli),
            },
        },
        (None, Some(active)) => EngineIdentity::ProfileVanished {
            cli_id,
            profile_name: active.to_owned(),
        },
        // **«EREDITATA» NON È «NIENTE».** Il processo parte con la casa di chi ha
        // aperto il terminale, che è un'identità vera e nominabile: dirlo è più
        // utile che lasciare un vuoto in cui questo caso si confonde con gli
        // altri quattro.
        (None, None) => EngineIdentity::InheritedFromTheTerminal { cli_id },
    }
}

/// Why a declared profile did not reach the environment, in the words of the
/// mechanism that keeps it out — and, where no mechanism is declared, in the
/// words the command line's own entry gives: «not known» would be false for
/// an entry that was measured and found unmovable.
fn why_it_stays_where_it_is(cli: &profiles::KnownCli) -> String {
    match &cli.home {
        profiles::HomeMechanism::CredentialSymlink { .. } => {
            "this command line has no variable that moves the home: the profile swaps a symlink, and the identity depends on where that file points on the disk".to_owned()
        }
        profiles::HomeMechanism::Unknown => {
            let why = "no variable is declared to move this command line's home, so nothing was overlaid";
            match cli.home_note.trim() {
                "" => why.to_owned(),
                note => format!("{why}; its entry says: {note}"),
            }
        }
        // Un meccanismo a variabile qui non ci arriva: chi chiama lo ha già
        // trattato sopra. Se un giorno ci arrivasse, la frase dice il vero.
        profiles::HomeMechanism::EnvVar(_) => {
            "the mechanism goes through a variable, and it was not overlaid".to_owned()
        }
    }
}

/// La dotazione di **questa** macchina per invocare `bin`.
///
/// **RILETTA A OGNI CHIAMATA**, per la stessa ragione del listino: un profilo
/// cambiato a metà di una corsa lunga vale dalla chiamata dopo, invece che dal
/// prossimo riavvio, e leggere un file piccolo accanto all'avvio di un processo
/// esterno non costa niente.
///
/// Uno stato dei profili illeggibile non ferma la chiamata: si parte senza
/// sovrapporre niente, che è come si è sempre partiti. Fermare un passo perché
/// non si è potuto leggere un file di preferenze punirebbe chi non c'entra.
pub(crate) fn current_equipment_for(bin: &str, step_env: &BTreeMap<String, String>) -> Equipment {
    let store = profiles::store_io::load_store().unwrap_or_default();
    equipment_for(&store, bin, step_env)
}
