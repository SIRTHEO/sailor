//! Il vaglio a secco prova la riga **nella casa in cui girerà davvero**.
//!
//! **PERCHÉ NON BASTA CHE IL PASSO PARTA NELLA CASA GIUSTA.** Dal 01/09/2026 un
//! passo di flusso fa partire il motore dentro la casa del profilo attivo — è la
//! cura del guasto 18 — mentre `RealDryProbe`, cioè quello che `sailor flow
//! check` usa per dire «riga di comando sana», passava un ambiente **vuoto**. Le
//! due strade divergono proprio dove fa più male: il controllo prova un motore
//! autenticato perché eredita la casa di chi ha aperto il terminale, e la corsa
//! vera parte in una casa che può non avere nessuna credenziale. Il controllo
//! chiude in verde e la corsa fallisce — e chi legge il verde non ha sbagliato
//! niente.
//!
//! **NON È UN CASO DI SCUOLA.** Il 01/09/2026 su questa macchina i due profili
//! `codex` dichiarati (`lavoro` e `prove`) puntavano tutti e due a cartelle
//! **senza `auth.json`**: da quel giorno ogni chiamata a `codex` da un passo
//! sarebbe partita non autenticata, e `flow check` avrebbe continuato a dire che
//! la riga è sana.
//!
//! **LA CURA NON È UN CONTROLLO NUOVO SULLE CREDENZIALI.** Un «esiste
//! `auth.json`?» sarebbe una seconda copia della verità, da tenere allineata a
//! mano per ogni motore — e i descrittori dichiarano **già** con quali parole un
//! motore dice di non poter lavorare (`unusable_when`, che nomina proprio le
//! credenziali mancanti). Basta che il vaglio parta dove partirà la corsa: la
//! diagnosi la dà il motore stesso, con le sue parole.
//!
//! **UN SOLO `#[test]` IN QUESTO FILE**, per la stessa ragione scritta in
//! `the_engine_really_starts_in_sailors_home`: `PROFILES_STATE_PATH` è di
//! processo, e due prove nello stesso binario se la scriverebbero addosso.

use actions::{DryProbe, DryRun, RealDryProbe};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Una cartella usa-e-getta sotto `$TMPDIR`, cancellata a fine prova.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "actions-dry-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("l'orologio non va all'indietro")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("cartella di prova");
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Un finto `codex`: si chiama come l'eseguibile vero — il legame su cui lavora
/// `profiles::cli_for_executable` — e stampa la sola cosa che serve sapere.
fn a_fake_codex_that_prints_its_home(dir: &Path) -> String {
    let path = dir.join("codex");
    fs::write(&path, "#!/bin/sh\nprintf 'CASA=%s\\n' \"$CODEX_HOME\"\n")
        .expect("scrivere il finto motore");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("renderlo eseguibile");
    path.to_string_lossy().into_owned()
}

/// **IL VAGLIO A SECCO E LA CORSA VERA DEVONO PARTIRE DALLA STESSA CASA.**
///
/// *Mutante che rimette il difetto originale*: `env: BTreeMap::new()` dentro
/// `RealDryProbe::run`. La prova torna rossa con `CASA=` vuota — che è
/// esattamente quello che il vaglio stampava fino al 01/09/2026.
#[test]
fn the_dry_probe_starts_inside_the_home_the_profile_declares() {
    let dir = TempDir::new();
    let bin = a_fake_codex_that_prints_its_home(dir.path());
    let home_of_the_profile = dir.path().join("case").join("codex").join("lavoro");

    let state = dir.path().join("profili.json");
    fs::write(
        &state,
        json!({
            "profiles": [
                {"name": "lavoro", "cli_id": "codex", "home_dir": home_of_the_profile}
            ],
            "active": {"codex": "lavoro"}
        })
        .to_string(),
    )
    .expect("scrivere lo stato dei profili");
    std::env::set_var("PROFILES_STATE_PATH", &state);

    let DryRun::Answered { stdout, .. } = RealDryProbe.run(&bin, &[], None) else {
        panic!("il finto motore risponde sempre: qui non c'è niente da aspettare");
    };

    assert!(
        stdout.contains(&format!("CASA={}", home_of_the_profile.display())),
        "il vaglio a secco ha provato la riga in una casa diversa da quella in cui \
         girerà: {stdout:?}"
    );
}
