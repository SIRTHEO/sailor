//! Più profili per ogni riga di comando conosciuta, anche quando quella riga
//! di comando non li supporta da sé.
//!
//! Il meccanismo: dove la riga di comando legge una variabile d'ambiente per
//! spostare la propria cartella di casa, un profilo è solo una cartella —
//! cambiarlo è cambiare una variabile, senza copie né rischio di sovrascrivere.
//! Dove quella variabile non esiste, il ripiego è un collegamento simbolico
//! sul file di credenziali dentro la casa fissa: più fragile, va marcato come
//! tale. Qui solo la parte pura; il filesystem lo tocca `main.rs`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Come una riga di comando trova la propria cartella di casa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeMechanism {
    /// Questa variabile sposta l'intera cartella di casa.
    EnvVar(&'static str),
    /// Nessuna variabile nota: il profilo scambia un collegamento simbolico
    /// su questo percorso, relativo alla casa fissa.
    CredentialSymlink { relative_path: &'static str },
    /// Non ancora verificato: non si sa come questa riga di comando sposti
    /// la sua casa, se lo fa.
    Unknown,
}

/// Se la riga di comando gestisce più profili da sé, e quanto è certo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeProfiles {
    Supported,
    NotSupported,
    /// Non verificato in questo ambiente: il comando vero non era
    /// raggiungibile, o non è stato lanciato.
    Unverified,
}

/// Una riga di comando conosciuta: come si invoca e come si sposta la sua
/// casa. `known_clis` è la tabella dichiarata — allungala aggiungendo una
/// voce, non serve altro.
#[derive(Debug, Clone, Copy)]
pub struct KnownCli {
    pub id: &'static str,
    pub display_name: &'static str,
    pub executable: &'static str,
    pub native_profiles: NativeProfiles,
    /// Come si è arrivati al giudizio sopra: cosa dice il comando vero, o
    /// perché non è stato verificato.
    pub native_profiles_note: &'static str,
    pub home: HomeMechanism,
    pub home_note: &'static str,
}

const KNOWN_CLIS: &[KnownCli] = &[
    KnownCli {
        id: "claude",
        display_name: "Claude Code",
        executable: "claude",
        native_profiles: NativeProfiles::NotSupported,
        native_profiles_note: "verificato su claude 2.1.247: `claude auth` offre solo login/logout/status, nessun sotto-comando di profilo o account multiplo in `--help`.",
        home: HomeMechanism::EnvVar("CLAUDE_CONFIG_DIR"),
        home_note: "verificato leggendo il binario installato: la variabile sposta l'intera cartella, incluso `.credentials.json` e `settings.json`.",
    },
    KnownCli {
        id: "codex",
        display_name: "Codex",
        executable: "codex",
        native_profiles: NativeProfiles::Supported,
        native_profiles_note: "`-p/--profile` in `codex --help`: sovrappone `$CODEX_HOME/<nome>.config.toml` sulla configurazione base — profili di configurazione, non credenziali separate di per sé.",
        home: HomeMechanism::EnvVar("CODEX_HOME"),
        home_note: "verificato con `codex doctor`: mostra auth.json e config.toml dentro la cartella indicata da CODEX_HOME.",
    },
    KnownCli {
        id: "gemini",
        display_name: "Gemini CLI",
        executable: "gemini",
        native_profiles: NativeProfiles::NotSupported,
        native_profiles_note: "nessun `--profile` in `gemini --help`: solo sessioni (`--resume`, `--session-id`), non identità separate.",
        home: HomeMechanism::EnvVar("GEMINI_CLI_HOME"),
        home_note: "verificato leggendo il sorgente installato: `baseDir = process.env[\"GEMINI_CLI_HOME\"] || join(homedir, \".gemini\")`.",
    },
    KnownCli {
        id: "antigravity",
        display_name: "Antigravity",
        executable: "antigravity",
        native_profiles: NativeProfiles::Unverified,
        native_profiles_note: "nessun binario `antigravity` in PATH in questo ambiente: non verificato.",
        home: HomeMechanism::Unknown,
        home_note: "i suoi dati vivono sotto ~/.gemini/antigravity-cli/, ipotesi di condividere GEMINI_CLI_HOME non verificata lanciando il comando vero.",
    },
];

/// La tabella delle righe di comando conosciute.
pub fn known_clis() -> &'static [KnownCli] {
    KNOWN_CLIS
}

/// Un profilo: nome scelto dall'utente, a quale riga di comando appartiene,
/// dove sta la sua cartella di casa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub cli_id: String,
    pub home_dir: PathBuf,
}

/// L'elenco dei profili e quale, per ciascuna riga di comando, è attivo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileStore {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// `cli_id` -> nome del profilo attivo.
    #[serde(default)]
    pub active: BTreeMap<String, String>,
}

/// Stringa vuota vale come elenco vuoto: è la forma di un file mai scritto.
pub fn parse_store(json: &str) -> Result<ProfileStore, serde_json::Error> {
    if json.trim().is_empty() {
        return Ok(ProfileStore::default());
    }
    serde_json::from_str(json)
}

pub fn serialize_store(store: &ProfileStore) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(store)
}

/// Perché un nome di profilo non va bene: uno di questi, se non catturato,
/// è un guasto di sicurezza — un nome che esce dalla cartella dei profili.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileNameError {
    Empty,
    /// Contiene `/` o `\`: da solo basta a impedire sia `../fuga` sia un
    /// nome assoluto che rimpiazzerebbe l'intero percorso in `Path::join`.
    PathSeparator,
    Traversal,
}

impl fmt::Display for ProfileNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "il nome del profilo è vuoto"),
            Self::PathSeparator => write!(f, "il nome del profilo contiene un separatore di percorso"),
            Self::Traversal => write!(f, "il nome del profilo è '.' o '..'"),
        }
    }
}

/// Niente `/`, niente `\`, niente `.`/`..`, niente nome vuoto: un nome
/// scelto dall'utente diventa un segmento di percorso, mai un percorso.
pub fn validate_profile_name(name: &str) -> Result<(), ProfileNameError> {
    if name.is_empty() {
        return Err(ProfileNameError::Empty);
    }
    if name.contains('/') || name.contains('\\') {
        return Err(ProfileNameError::PathSeparator);
    }
    if name == "." || name == ".." {
        return Err(ProfileNameError::Traversal);
    }
    Ok(())
}

/// Dove sta la cartella di casa di un profilo, dentro la radice dei profili.
/// Valida sia `cli_id` sia `profile_name`: entrambi diventano un segmento.
pub fn profile_home_path(
    profiles_root: &Path,
    cli_id: &str,
    profile_name: &str,
) -> Result<PathBuf, ProfileNameError> {
    validate_profile_name(cli_id)?;
    validate_profile_name(profile_name)?;
    Ok(profiles_root.join(cli_id).join(profile_name))
}

/// L'ambiente da sovrapporre per lanciare `cli` con la casa in
/// `profile_home`. Vuoto quando il meccanismo non usa una variabile: lì lo
/// scambio è un'operazione sul filesystem, vedi [`symlink_swap`].
pub fn build_environment(cli: &KnownCli, profile_home: &Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if let HomeMechanism::EnvVar(name) = cli.home {
        env.insert(name.to_owned(), profile_home.to_string_lossy().into_owned());
    }
    env
}

/// I due percorsi coinvolti in uno scambio per collegamento simbolico: dove
/// sta il collegamento dentro la casa fissa, e dove deve puntare per
/// arrivare al file di questo profilo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymlinkSwap {
    pub link_path: PathBuf,
    pub target_path: PathBuf,
}

pub fn symlink_swap(fixed_home: &Path, relative_path: &str, profile_home: &Path) -> SymlinkSwap {
    SymlinkSwap {
        link_path: fixed_home.join(relative_path),
        target_path: profile_home.join(relative_path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_profile_name_rejects_empty() {
        assert_eq!(validate_profile_name(""), Err(ProfileNameError::Empty));
    }

    #[test]
    fn validate_profile_name_rejects_dot_and_dotdot() {
        assert_eq!(validate_profile_name("."), Err(ProfileNameError::Traversal));
        assert_eq!(validate_profile_name(".."), Err(ProfileNameError::Traversal));
    }

    #[test]
    fn validate_profile_name_rejects_path_separators() {
        assert_eq!(
            validate_profile_name("a/b"),
            Err(ProfileNameError::PathSeparator)
        );
        assert_eq!(
            validate_profile_name("a\\b"),
            Err(ProfileNameError::PathSeparator)
        );
    }

    #[test]
    fn validate_profile_name_accepts_ordinary_names() {
        assert_eq!(validate_profile_name("lavoro"), Ok(()));
        assert_eq!(validate_profile_name("cliente-1"), Ok(()));
        assert_eq!(validate_profile_name("a.b"), Ok(()));
    }

    /// La prova che conta: nomi pensati per uscire dalla cartella dei
    /// profili — traversal e percorso assoluto — sono tutti respinti prima
    /// di diventare un `Path::join`, dove un nome assoluto rimpiazzerebbe
    /// l'intero percorso invece di aggiungersi.
    #[test]
    fn profile_home_path_rejects_every_escape_attempt() {
        let root = Path::new("/var/profiles");
        let malicious = [
            "../../etc/passwd",
            "..",
            "/etc/passwd",
            "sub/../../escape",
            "",
        ];
        for name in malicious {
            assert!(
                profile_home_path(root, "claude", name).is_err(),
                "atteso rifiuto per {name:?}"
            );
        }
    }

    #[test]
    fn profile_home_path_stays_inside_the_root_for_a_valid_name() {
        let root = Path::new("/var/profiles");
        let home = profile_home_path(root, "claude", "lavoro").unwrap();
        assert!(home.starts_with(root));
        assert_eq!(home, root.join("claude").join("lavoro"));
    }

    #[test]
    fn build_environment_sets_the_env_var_for_the_env_mechanism() {
        let cli = known_clis().iter().find(|c| c.id == "codex").unwrap();
        let env = build_environment(cli, Path::new("/home/profiles/codex/lavoro"));
        assert_eq!(
            env.get("CODEX_HOME").map(String::as_str),
            Some("/home/profiles/codex/lavoro")
        );
        assert_eq!(env.len(), 1);
    }

    #[test]
    fn build_environment_is_empty_without_an_env_var_mechanism() {
        let cli = KnownCli {
            id: "acme",
            display_name: "Acme CLI",
            executable: "acme",
            native_profiles: NativeProfiles::NotSupported,
            native_profiles_note: "di prova",
            home: HomeMechanism::CredentialSymlink {
                relative_path: "credentials.json",
            },
            home_note: "di prova",
        };
        let env = build_environment(&cli, Path::new("/home/profiles/acme/lavoro"));
        assert!(env.is_empty());
    }

    #[test]
    fn symlink_swap_composes_the_two_paths() {
        let swap = symlink_swap(
            Path::new("/home/theo/.acme"),
            "credentials.json",
            Path::new("/home/profiles/acme/lavoro"),
        );
        assert_eq!(
            swap.link_path,
            Path::new("/home/theo/.acme/credentials.json")
        );
        assert_eq!(
            swap.target_path,
            Path::new("/home/profiles/acme/lavoro/credentials.json")
        );
    }

    #[test]
    fn store_roundtrip_through_json() {
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "lavoro".to_owned(),
            cli_id: "claude".to_owned(),
            home_dir: PathBuf::from("/home/profiles/claude/lavoro"),
        });
        store.active.insert("claude".to_owned(), "lavoro".to_owned());

        let json = serialize_store(&store).unwrap();
        let parsed = parse_store(&json).unwrap();
        assert_eq!(parsed, store);
    }

    #[test]
    fn parse_store_treats_empty_string_as_empty_store() {
        assert_eq!(parse_store("").unwrap(), ProfileStore::default());
        assert_eq!(parse_store("   \n").unwrap(), ProfileStore::default());
    }

    #[test]
    fn known_clis_have_unique_non_empty_ids() {
        let clis = known_clis();
        assert!(!clis.is_empty());
        let mut ids: Vec<&str> = clis.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), clis.len(), "id duplicato nella tabella");
        for cli in clis {
            assert!(!cli.id.is_empty());
            assert!(!cli.executable.is_empty());
        }
    }
}
