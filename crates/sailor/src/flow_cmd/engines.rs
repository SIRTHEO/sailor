//! The two questions `sailor flow check` asks each engine without spending:
//! whether the line it assembles is sound, and whether its home is logged in.

use flow::Graph;
use std::collections::{BTreeMap, BTreeSet};

use super::check::{engines_of, EngineWorld};

// ── le case di credenziali, chieste al motore ────────────────────────────

/// **UNA CASA DICHIARATA E VUOTA SI APPLICA IN SILENZIO, ED È QUELLO CHE QUESTA
/// SEZIONE ROMPE.**
///
/// Dal 01/09/2026 un motore lanciato da un passo parte nella casa del profilo
/// attivo — è la cura del guasto 18 — e un profilo che punta a una cartella
/// senza credenziali fa partire ogni chiamata **non autenticata** senza che
/// niente lo dica. Il vaglio a secco non può vederlo e non deve provarci: toglie
/// la domanda apposta, quindi il motore si ferma su «non mi hai dato niente da
/// fare» e non arriva mai ai controlli che vengono dopo. È il guasto 39, e la
/// metà che restava scoperta.
///
/// **SI CHIEDE AL MOTORE, E COME SI CHIEDE LO DICE IL DESCRITTORE.** Non si va a
/// cercare `auth.json` sul disco: sarebbe una seconda copia della verità, una per
/// motore, da tenere allineata a mano. Chi non dichiara `login_status` non fa
/// scattare niente — **vuoto vuol dire «nessuno ha guardato», mai «è
/// autenticato»** — ed è la stessa regola di `refuses_without_prompt`.
///
/// **NON FA FALLIRE IL CONTROLLO, E IL VERSO È DELIBERATO.** Fermare un flusso
/// perché un profilo non è autenticato punirebbe chi non c'entra — è la cura
/// sbagliata del guasto 35 — e chi controlla un flusso lo fa anche per capirlo,
/// non solo per lanciarlo. Deve **vedersi**, e basta.
///
/// **COSTA ZERO E LO STESSO ESEGUE.** `codex login status` e `claude auth status`
/// leggono un file locale: nessun modello, nessun fornitore, nessun denaro.
/// Restano processi avviati, quindi vivono dietro la stessa sonda delle righe di
/// comando e tacciono insieme a lei con `--no-engines`.
pub(super) fn login_states_into(
    report: &mut String,
    graph: &Graph,
    tools: &toolbox::Tools,
    world: &EngineWorld,
) {
    use actions::{LoginVerdict, ToolResolver};

    let mut unauthenticated = Vec::new();
    let mut authenticated = Vec::new();
    let mut unknown = Vec::new();

    let mut asked: BTreeSet<String> = BTreeSet::new();
    for wanted in engines_wanted(graph) {
        // Un motore si interroga UNA VOLTA SOLA anche quando lo nominano sei
        // passi: la casa viene dal profilo attivo, non dal passo, quindi sei
        // domande darebbero sei volte la stessa risposta. Il rapporto nomina il
        // motore e il profilo, che è ciò che chi legge deve cambiare.
        if !tools.declares(&wanted.tool) || !asked.insert(wanted.tool.clone()) {
            continue;
        }
        // Un motore che non è invocabile qui è già nominato dalla sezione delle
        // righe: ripeterlo manderebbe a cercare due difetti dove ce n'è uno.
        let Ok(bin) = tools.resolve(&wanted.tool) else {
            continue;
        };
        // **SOLO DOVE UN PROFILO È IN FORZA.** Senza profilo attivo il motore
        // parte nella casa di chi ha aperto il terminale, che è la casa di
        // sempre: non c'è nessuna scelta di Sailor da rendere visibile, e
        // un avviso qui parlerebbe di una cosa che questo comando non governa.
        let equipment = actions::equipment_for(world.profiles, &bin, &BTreeMap::new());
        let ledger::EngineIdentity::ProfileInForce {
            cli_id,
            profile_name,
            ..
        } = &equipment.identity
        else {
            continue;
        };
        // La casa si mostra come la riceve il motore — variabile e valore — e
        // non ricalcolata da un'altra parte: due strade che compongono la stessa
        // cosa divergono al primo che cambia.
        let home = equipment
            .env
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(" ");
        let who = format!(
            "{} (profilo «{cli_id}/{profile_name}», {home})",
            wanted.tool
        );

        let Some(recipe) = tools.login_recipe(&wanted.tool) else {
            unknown.push(catalogue::say(
                "cli.flow.engine_login_not_declared",
                &[("who", &who)],
            ));
            continue;
        };
        match actions::probe_login_status(world.probe, &bin, &equipment.env, &recipe) {
            LoginVerdict::LoggedIn { .. } => authenticated.push(who),
            // LE PAROLE DEL MOTORE, come per una riga rotta: «non autenticato»
            // detto da noi non dice quale credenziale manca, e la frase sua sì.
            LoginVerdict::LoggedOut { said } => unauthenticated.push(format!("{who}: «{said}»")),
            LoginVerdict::NotDeclared => unknown.push(catalogue::say(
                "cli.flow.engine_login_half_declared",
                &[("who", &who)],
            )),
            LoginVerdict::Unrecognised { said } => unknown.push(catalogue::say(
                "cli.flow.engine_login_unrecognised",
                &[("who", &who), ("said", &said)],
            )),
            LoginVerdict::NoAnswer { why } => {
                unknown.push(format!("{who}: nessuna risposta — {why}"))
            }
        }
    }

    if !unauthenticated.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.homes_without_credentials",
            &[("engines", &unauthenticated.join("; "))],
        ));
    }
    if !authenticated.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.homes_authenticated",
            &[("engines", &authenticated.join("; "))],
        ));
    }
    if !unknown.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.homes_unknown",
            &[("engines", &unknown.join("; "))],
        ));
    }
}

// ── le righe di comando, montate e provate senza domanda ────────────────

/// Un motore su cui un passo si affida al descrittore per comporre la riga.
struct WantedEngine {
    step: String,
    tool: String,
}

/// I motori di cui `flow check` deve provare la riga, passo per passo e
/// **motore per motore della catena**.
///
/// **TUTTA LA CATENA, NON IL PRIMO.** Il guasto 16 è nato da sei passi che
/// nominavano un motore solo; il guasto 27 dice che il difetto stava nel
/// *secondo* — nessun flusso mette `agy` per primo, quindi quel ramo non era
/// mai stato eseguito e la riga sbagliata è vissuta indisturbata. Guardare solo
/// il primo motore è non guardare dove il difetto era.
///
/// **E SOLO I PASSI CHE LA RIGA NON SE LA SCRIVONO.** Un passo che dichiara i
/// propri `args` vince sulla ricetta — lo decide `ExternalEngineAction`, e qui
/// si legge la stessa regola, non una seconda copia di essa. Sono i passi che
/// invocano `cargo` o `git` attraverso la stessa azione: la loro riga non viene
/// da nessun blocco `ask`, e chiamarla «non montabile» sarebbe un allarme su un
/// passo sano.
fn engines_wanted(graph: &Graph) -> Vec<WantedEngine> {
    let mut wanted = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        if with.get("args").is_some() {
            continue;
        }
        for tool in engines_of(with) {
            wanted.push(WantedEngine {
                step: step.id.clone(),
                tool,
            });
        }
    }
    wanted
}

/// Cosa si è potuto sapere della riga di un motore.
enum EngineOutcome {
    /// Il motore non è invocabile qui, e il rilevatore dice perché.
    NotHere(String),
    /// Nessun blocco `ask`: la riga non si compone affatto, e non c'è niente
    /// da provare. È un'assenza nel descrittore, non un difetto della riga.
    NotAssemblable,
    /// La riga si è montata e si è provata: ecco com'è venuta e cosa ha detto.
    Tried {
        line: String,
        verdict: actions::ProbeVerdict,
    },
}

/// Monta la riga di ogni motore di ogni catena, la prova **senza dare la
/// domanda**, e scrive nel rapporto come sta messa.
///
/// **QUI `flow check` CAMBIA NATURA, E VA DETTO.** `resolver.rs` dichiara in
/// testa che risolvere un nome non deve eseguire niente, e resta vero: è questa
/// funzione che avvia processi, non la risoluzione. Da qui in poi `flow check`
/// avvia un processo per ogni motore dichiarato — **senza rete, senza denaro,
/// con un tetto di tempo**, perché senza la domanda nessuno di quei processi
/// chiama un fornitore. Il prezzo è che un controllo statico non è più solo
/// statico; il ricavo è che la cura scritta accanto al guasto 1 esiste davvero.
///
/// **ACCESO IN MODO PREDEFINITO.** Un controllo dietro una bandiera è un
/// controllo che nessuno interroga: il guasto 27 sarebbe rimasto invisibile
/// esattamente come è rimasto, perché nessuno avrebbe scritto la bandiera. Chi
/// non lo vuole scrive `--no-engines`, e allora il rapporto **tace** invece di
/// dichiarare sane righe che non ha guardato.
///
/// **L'ASSE «È STATO CHIAMATO DAVVERO» NON È QUESTO, E RESTA SEPARATO.** Una
/// riga sana non dice che quel motore abbia mai risposto a una domanda vera:
/// quello lo sa il deposito, che registra le chiamate. Mescolare le due cose
/// farebbe passare per «usato» un motore che nessuna corsa ha mai nominato —
/// che è precisamente il guasto 32.
pub(super) fn engine_lines_into(
    report: &mut String,
    graph: &Graph,
    tools: &toolbox::Tools,
    probe: &dyn actions::DryProbe,
) {
    use actions::{ProbeVerdict, ToolResolver};

    // Un motore si prova UNA VOLTA SOLA anche quando lo nominano sei passi: la
    // riga che si monta viene dal descrittore, non dal passo, quindi sei prove
    // avvierebbero sei processi per sapere sei volte la stessa cosa. Il
    // rapporto resta passo per passo, che è ciò che chi legge deve correggere.
    let mut judged: BTreeMap<String, EngineOutcome> = BTreeMap::new();

    let mut sound = Vec::new();
    let mut broken = Vec::new();
    let mut untried = Vec::new();
    let mut unassemblable = Vec::new();
    let mut exhausted = Vec::new();

    for wanted in engines_wanted(graph) {
        // Uno strumento che nessun descrittore dichiara è già nominato sopra:
        // ripeterlo qui manderebbe a cercare due difetti dove ce n'è uno.
        if !tools.declares(&wanted.tool) {
            continue;
        }
        if !judged.contains_key(&wanted.tool) {
            let outcome = match tools.resolve(&wanted.tool) {
                Err(reason) => EngineOutcome::NotHere(reason),
                Ok(bin) => match tools.ask_recipe(&wanted.tool) {
                    None => EngineOutcome::NotAssemblable,
                    Some(recipe) => {
                        let line = std::iter::once(bin.clone())
                            .chain(actions::command_line(&recipe))
                            .collect::<Vec<_>>()
                            .join(" ");
                        EngineOutcome::Tried {
                            verdict: actions::probe_dry_run(probe, &bin, &recipe),
                            line,
                        }
                    }
                },
            };
            judged.insert(wanted.tool.clone(), outcome);
        }

        let who = format!("{} → {}", wanted.step, wanted.tool);
        match judged.get(&wanted.tool).expect("appena inserito") {
            EngineOutcome::NotHere(reason) => untried.push(catalogue::say(
                "cli.flow.engine_not_invocable_here",
                &[("who", &who), ("reason", reason)],
            )),
            EngineOutcome::NotAssemblable => unassemblable.push(catalogue::say(
                "cli.flow.engine_line_not_assemblable",
                &[("who", &who)],
            )),
            EngineOutcome::Tried { line, verdict } => match verdict {
                ProbeVerdict::Sound => sound.push(who),
                // LE PAROLE DEL MOTORE PER INTERO, E LA RIGA CHE LE HA
                // PRODOTTE. Sul guasto 27 la frase di `agy` diceva quale
                // bandiera aveva mangiato quale argomento: una diagnosi che
                // nessuna parola nostra avrebbe potuto sostituire. Tagliarla, o
                // riassumerla, riporterebbe chi legge a indovinare.
                ProbeVerdict::Broken { said } => broken.push(catalogue::say(
                    "cli.flow.engine_line_broken",
                    &[("who", &who), ("line", line), ("said", said)],
                )),
                ProbeVerdict::CannotWork { said } => exhausted.push(format!("{who}: «{said}»")),
                ProbeVerdict::NotDeclared => untried.push(catalogue::say(
                    "cli.flow.engine_refusal_not_declared",
                    &[("who", &who), ("line", line)],
                )),
                ProbeVerdict::TimedOut { why } => untried.push(catalogue::say(
                    "cli.flow.engine_no_answer_to_line",
                    &[("who", &who), ("line", line), ("why", why)],
                )),
            },
        }
    }

    if !sound.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.command_lines_sound",
            &[("engines", &sound.join("; "))],
        ));
    }
    if !broken.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.command_lines_broken",
            &[("engines", &broken.join("; "))],
        ));
    }
    if !exhausted.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.engines_that_cannot_work_now",
            &[("engines", &exhausted.join("; "))],
        ));
    }
    if !untried.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.command_lines_untried",
            &[("engines", &untried.join("; "))],
        ));
    }
    if !unassemblable.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.command_lines_not_assemblable",
            &[("engines", &unassemblable.join("; "))],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::super::check::check_report;
    use super::*;
    use flow::FlowFile;
    use registry::{registry_in, House};
    use std::path::{Path, PathBuf};

    // ── le case di credenziali ────────────────────────────────────────

    /// Un finto `codex` che si comporta come quello vero **su questa domanda**:
    /// risponde su stderr, dice «Not logged in» quando in casa non c'è
    /// `auth.json`, e «Logged in using ChatGPT» quando c'è. Anche i due codici
    /// d'uscita sono quelli misurati il 01/09/2026 — 1 e 0 — apposta: se un
    /// giorno qualcuno facesse dipendere il verdetto dall'esito, questa prova
    /// resterebbe verde, e la prova gemella in `crates/actions/tests` dice
    /// perché non basterebbe.
    ///
    /// **SI CHIAMA `codex` PERCHÉ IL LEGAME È L'ESEGUIBILE**: è su quel nome che
    /// `profiles::cli_for_executable` decide quale variabile sposta la casa.
    fn a_fake_codex_that_answers_about_its_home(dir: &Path) -> String {
        let path = dir.join("codex");
        std::fs::write(
            &path,
            "#!/bin/sh\n\
             if [ \"$1\" = login ] && [ \"$2\" = status ]; then\n\
             \x20 if [ -f \"$CODEX_HOME/auth.json\" ]; then\n\
             \x20   echo 'Logged in using ChatGPT' >&2; exit 0\n\
             \x20 fi\n\
             \x20 echo 'Not logged in' >&2; exit 1\n\
             fi\n\
             echo 'No prompt provided via stdin.' >&2\n\
             exit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("bit di esecuzione");
        }
        path.to_string_lossy().into_owned()
    }

    /// Una cartella usa-e-getta con dentro il finto motore e il suo descrittore.
    fn a_machine_with_a_real_fake_codex(declares_login: bool) -> (PathBuf, toolbox::Tools) {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("prova-case-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("la cartella di prova");
        a_fake_codex_that_answers_about_its_home(&dir);

        let login = if declares_login {
            r#","login_status":{"args":["login","status"],
               "logged_in_when":["logged in using"],
               "logged_out_when":["not logged in"]}"#
        } else {
            ""
        };
        let file = dir.join("tools.json");
        std::fs::write(
            &file,
            format!(
                r#"{{"tools":[{{"id":"codex","family":"ai_cli","label":"codex",
                   "detect":{{"command":"codex"}},
                   "ask":{{"args":["exec"],"prompt":"stdin",
                           "refuses_without_prompt":["no prompt provided via stdin"]}}
                   {login}}}]}}"#
            ),
        )
        .expect("scrivere i descrittori");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        let tools = toolbox::Tools::new(
            catalog,
            toolbox::Machine {
                path_dirs: vec![dir.clone()],
                home: dir.clone(),
                env: BTreeMap::new(),
                version_probes: false,
            },
        );
        (dir, tools)
    }

    /// Uno stato dei profili che dichiara una casa sola, attiva.
    fn a_store_pointing_at(home: &Path) -> profiles::ProfileStore {
        profiles::ProfileStore {
            profiles: vec![profiles::Profile {
                name: "prove".to_owned(),
                cli_id: "codex".to_owned(),
                home_dir: home.to_path_buf(),
                endpoint: None,
            }],
            active: [("codex".to_owned(), "prove".to_owned())]
                .into_iter()
                .collect(),
        }
    }

    /// **IL GUASTO 39, L'ALTRA METÀ, CONTRO UN PROCESSO VERO.**
    ///
    /// Il vaglio a secco continua a dire «riga sana» in tutti e due i casi — è
    /// quello che deve fare, toglie la domanda apposta — e accanto compare la
    /// cosa che nessuno diceva: da quale casa parte questo motore, e se quella
    /// casa ha delle credenziali.
    ///
    /// **DUE BRACCI, E SERVONO TUTTI E DUE.** Il primo da solo resterebbe verde
    /// con un controllo che gridasse sempre; il secondo da solo resterebbe verde
    /// con un controllo che non guarda niente. Insieme dicono che la risposta
    /// viene dalla casa.
    ///
    /// **E LA SONDA È QUELLA VERA.** `RealDryProbe` avvia un processo: una
    /// finta risponderebbe quello che le diciamo noi, cioè proverebbe che
    /// sappiamo scrivere una risposta. Qui il motore la legge dal disco.
    ///
    /// *Mutanti eseguiti*: (a) leggere `logged_in_when` prima di
    /// `logged_out_when` in `judge_login_status` — il primo braccio diventa
    /// rosso, cioè si rimette il silenzio originale; (b) togliere
    /// `login_status` dal descrittore — vedi la prova qui sotto.
    #[test]
    fn a_flow_check_says_which_home_the_engine_starts_from_and_whether_it_has_credentials() {
        let (dir, tools) = a_machine_with_a_real_fake_codex(true);
        let flow = flow_with_chain(r#""codex""#);
        let real = actions::RealDryProbe;

        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");
        let store = a_store_pointing_at(&empty);
        let (report, unknown) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );
        assert!(
            report.contains("HOMES WITHOUT CREDENTIALS"),
            "una casa senza credenziali si applica in silenzio: {report}"
        );
        assert!(
            report.contains(&empty.display().to_string()) && report.contains("codex/prove"),
            "chi legge deve sapere QUALE profilo e QUALE casa, o non sa cosa cambiare: {report}"
        );
        assert!(
            report.contains("Not logged in"),
            "le parole del motore sono la diagnosi: {report}"
        );
        assert!(
            report.contains("sound command lines"),
            "il vaglio a secco continua a dire la sua, e continua a dire il vero: {report}"
        );
        assert!(
            unknown.is_empty(),
            "un profilo senza credenziali NON fa fallire il controllo: punire chi non \
             c'entra è la cura sbagliata"
        );

        let full = dir.join("casa-piena");
        std::fs::create_dir_all(&full).expect("la casa autenticata");
        std::fs::write(full.join("auth.json"), "{}").expect("le credenziali");
        let store = a_store_pointing_at(&full);
        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );
        assert!(
            report.contains("authenticated homes"),
            "una casa piena deve risultare piena: {report}"
        );
        assert!(
            !report.contains("HOMES WITHOUT CREDENTIALS"),
            "e non deve comparire fra quelle vuote: {report}"
        );
    }

    /// **CHI NON DICHIARA IL BLOCCO NON FA SCATTARE NIENTE — E NON DICE
    /// «AUTENTICATO».**
    ///
    /// È il mutante (b) scritto una volta per tutte invece che eseguito una
    /// volta sola: la casa è vuota identica a quella del primo braccio qui
    /// sopra, e il solo cambiamento è che il descrittore non dice come si
    /// chiede. Il rapporto deve dire **che nessuno ha guardato**, mai tacere e
    /// mai rassicurare. Un predefinito comodo qui rimetterebbe il difetto per
    /// ogni motore che il blocco non ce l'ha ancora — cioè per tutti quelli che
    /// verranno.
    #[test]
    fn a_descriptor_without_the_block_makes_the_check_say_nobody_looked() {
        let (dir, tools) = a_machine_with_a_real_fake_codex(false);
        let flow = flow_with_chain(r#""codex""#);
        let real = actions::RealDryProbe;
        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");
        let store = a_store_pointing_at(&empty);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld {
                probe: &real,
                profiles: &store,
            }),
        );

        assert!(
            report.contains("homes whose authentication nobody could read")
                && report.contains("nobody looked"),
            "un'assenza deve dirsi: {report}"
        );
        assert!(
            !report.contains("authenticated homes"),
            "«nessuno ha guardato» non è «è autenticato»: {report}"
        );
        assert!(
            !report.contains("HOMES WITHOUT CREDENTIALS"),
            "e non è nemmeno «non è autenticato»: inventare un no dove non si è \
             guardato manderebbe a riparare una casa sana: {report}"
        );
    }

    // ── le righe di comando provate a secco ───────────────────────────

    /// Una macchina finta con dei motori dentro, e i loro descrittori.
    ///
    /// Niente dipende da cosa è installato su chi esegue: il percorso è una
    /// cartella temporanea, e i motori sono file vuoti col bit di esecuzione —
    /// non vengono mai avviati, perché la sonda di queste prove è finta.
    fn tools_with_engines(entries: &[(&str, &str)]) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("prova-motori-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("la cartella di prova");
        let mut declared = Vec::new();
        for (id, ask) in entries {
            let path = dir.join(id);
            std::fs::write(&path, "").expect("il finto eseguibile");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .expect("bit di esecuzione");
            }
            declared.push(format!(
                r#"{{"id":"{id}","family":"ai_cli","label":"{id}","detect":{{"command":"{id}"}}{ask}}}"#
            ));
        }
        let file = dir.join("tools.json");
        std::fs::write(&file, format!(r#"{{"tools":[{}]}}"#, declared.join(",")))
            .expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(
            catalog,
            toolbox::Machine {
                path_dirs: vec![dir.clone()],
                home: dir,
                env: BTreeMap::new(),
                version_probes: false,
            },
        )
    }

    /// Una sonda che non esegue niente e risponde ciò che le diciamo, in base a
    /// come si chiama l'eseguibile che le viene passato.
    struct ScriptedProbe(Vec<(&'static str, &'static str)>);

    impl actions::DryProbe for ScriptedProbe {
        fn run(&self, bin: &str, _args: &[String], _stdin: Option<Vec<u8>>) -> actions::DryRun {
            let said = self
                .0
                .iter()
                .find(|(name, _)| bin.ends_with(name))
                .map(|(_, said)| *said)
                .unwrap_or("");
            actions::DryRun::Answered {
                stdout: String::new(),
                stderr: said.to_owned(),
            }
        }
    }

    /// Alla domanda sulle credenziali non risponde niente: queste prove parlano
    /// delle righe di comando, e senza profilo attivo la domanda non si fa
    /// nemmeno. Un finto che rispondesse qualcosa direbbe qualcosa di questo
    /// mondo, e ci sono prove apposta per quello.
    impl actions::LoginProbe for ScriptedProbe {
        fn ask(
            &self,
            _bin: &str,
            _args: &[String],
            _env: &BTreeMap<String, String>,
        ) -> actions::DryRun {
            actions::DryRun::NoAnswer {
                why: "questa sonda non risponde alla domanda sulle credenziali".to_owned(),
            }
        }
    }

    fn flow_with_chain(chain: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "chiedi",
                        "deps": [],
                        "action": "external_engine",
                        "max_attempts": 1,
                        "when": null,
                        "with": {{"tool": {chain}, "stdin": "ciao", "timeout_secs": 10}},
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    const REFUSES: &str = r#","ask":{"args":["-p"],"prompt":"stdin","refuses_without_prompt":["input must be provided"]}"#;
    const SAYS_NOTHING: &str = r#","ask":{"args":["-p"],"prompt":"stdin"}"#;
    const NO_ASK: &str = "";

    /// **UNA RIGA SANA SI VEDE, E COSTA ZERO.** È il controllo che il guasto 1
    /// aveva chiesto il 28/08 e che nessuno aveva scritto perché sembrava voler
    /// dire spendere.
    #[test]
    fn a_line_the_engine_only_complains_about_the_missing_prompt_is_called_sound() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);
        let probe = ScriptedProbe(vec![("motore", "Input must be provided through stdin")]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(report.contains("sound command lines"), "{report}");
        assert!(report.contains("chiedi → motore"), "{report}");
    }

    /// **LE PAROLE DEL MOTORE SONO LA DIAGNOSI, E VANNO SCRITTE PER INTERO.**
    /// Sul guasto 27 la frase di `agy` diceva quale bandiera aveva mangiato
    /// quale argomento; un rapporto che dicesse solo «rotta» rimanderebbe a
    /// indovinare, cioè non varrebbe più della sua assenza.
    #[test]
    fn a_broken_line_is_reported_with_the_engines_own_words_and_the_line_that_produced_it() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);
        let probe = ScriptedProbe(vec![(
            "motore",
            "--print took \"--output-format\" as its prompt",
        )]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(report.contains("BROKEN command lines"), "{report}");
        assert!(
            report.contains("--print took \"--output-format\" as its prompt"),
            "senza le parole del motore la riga rossa non dice cosa correggere: {report}"
        );
        assert!(
            report.contains("assembled line «") && report.contains("-p»"),
            "e senza la riga montata non si sa nemmeno cosa è stato provato: {report}"
        );
    }

    /// **SI GUARDA TUTTA LA CATENA, NON IL PRIMO.** Il guasto 27 stava nel
    /// **secondo** motore di ogni catena, ed è vissuto indisturbato proprio
    /// perché nessun flusso lo metteva per primo. Un controllo che leggesse
    /// solo il primo motore sarebbe un controllo che non guarda dov'era il
    /// difetto.
    #[test]
    fn every_engine_of_the_chain_is_tried_not_only_the_first() {
        let flow = flow_with_chain(r#"["primo", "secondo", "terzo"]"#);
        let tools =
            tools_with_engines(&[("primo", REFUSES), ("secondo", REFUSES), ("terzo", REFUSES)]);
        let probe = ScriptedProbe(vec![
            ("primo", "Input must be provided through stdin"),
            ("secondo", "took --output-format as its prompt"),
            ("terzo", "Input must be provided through stdin"),
        ]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(
            report.contains("chiedi → secondo"),
            "il secondo della catena non è stato guardato: {report}"
        );
        assert!(
            report.contains("took --output-format as its prompt"),
            "{report}"
        );
        assert!(report.contains("chiedi → terzo"), "né il terzo: {report}");
    }

    /// **«NON PROVATA» E «NON MONTABILE» SONO DUE FATTI DIVERSI.** Un motore
    /// senza blocco `ask` non ha nessuna riga da provare — si ripara scrivendo
    /// il descrittore; uno che ha la riga ma non dichiara come rifiuta ce l'ha
    /// e nessuno l'ha guardata — si ripara eseguendola. Sotto la stessa parola
    /// manderebbero a fare il lavoro sbagliato, ed è il guasto 32 che vive
    /// nella prima delle due.
    #[test]
    fn a_missing_ask_block_is_not_confused_with_a_line_nobody_looked_at() {
        let flow = flow_with_chain(r#"["senza-ask", "senza-rifiuto"]"#);
        let tools = tools_with_engines(&[("senza-ask", NO_ASK), ("senza-rifiuto", SAYS_NOTHING)]);
        let probe = ScriptedProbe(vec![("senza-rifiuto", "un errore qualunque")]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        let untried = report
            .lines()
            .find(|line| line.starts_with("command lines not tried"))
            .unwrap_or_else(|| panic!("manca la riga «non provate»: {report}"));
        let unassemblable = report
            .lines()
            .find(|line| line.starts_with("command lines that cannot be assembled"))
            .unwrap_or_else(|| panic!("manca la riga «non montabili»: {report}"));

        assert!(untried.contains("senza-rifiuto"), "{untried}");
        assert!(
            !untried.contains("senza-ask"),
            "un motore senza `ask` non è una riga non provata: {untried}"
        );
        assert!(unassemblable.contains("senza-ask"), "{unassemblable}");
        assert!(!unassemblable.contains("senza-rifiuto"), "{unassemblable}");
    }

    /// **UN MOTORE ESAURITO NON È UNA RIGA ROTTA**, e la sua frase è la quarta.
    /// Confonderli manderebbe a correggere un descrittore sano mentre bastava
    /// aspettare.
    #[test]
    fn an_engine_that_cannot_work_now_gets_its_own_sentence() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[(
            "motore",
            r#","ask":{"args":["-p"],"prompt":"stdin","unusable_when":["weekly limit"],"refuses_without_prompt":["input must be provided"]}"#,
        )]);
        let probe = ScriptedProbe(vec![("motore", "You've hit your weekly limit")]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(
            report.contains("engines that cannot work right now"),
            "{report}"
        );
        assert!(
            !report.contains("BROKEN command lines"),
            "la riga è sana, è la quota che è finita: {report}"
        );
    }

    /// **SENZA SONDA IL RAPPORTO TACE**, non dichiara sane righe che non ha
    /// guardato: è la stessa regola del rilevatore assente, e senza di essa
    /// `--no-engines` diventerebbe un modo per far dire al controllo una cosa
    /// che non ha verificato.
    #[test]
    fn with_no_engines_the_report_says_nothing_about_command_lines() {
        let flow = flow_with_chain(r#""motore""#);
        let tools = tools_with_engines(&[("motore", REFUSES)]);

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(!report.contains("command lines"), "{report}");
    }

    /// I passi che scrivono i propri `args` non compongono nessuna riga dal
    /// descrittore: sono quelli che invocano `cargo` o `git` attraverso la
    /// stessa azione, e chiamarli «non montabili» sarebbe un allarme su un
    /// passo sano — cioè rumore che insegna a non leggere il rapporto.
    #[test]
    fn a_step_that_writes_its_own_arguments_is_not_reported_as_unassemblable() {
        let json = r#"{
            "id": "prova",
            "description": "flusso di prova",
            "graph": {
                "steps": [{
                    "id": "prove",
                    "deps": [],
                    "action": "external_engine",
                    "max_attempts": 1,
                    "when": null,
                    "with": {"tool": "cargo", "args": ["test"], "timeout_secs": 10},
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"}
                }],
                "skippable_dependencies": []
            },
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");
        let tools = tools_with_engines(&[("cargo", NO_ASK)]);
        let probe = ScriptedProbe(vec![]);

        let (report, _) = check_report(
            &flow,
            &registry_in(House::empty(), None, None),
            Some(&tools),
            Some(&EngineWorld::without_profiles(&probe)),
        );

        assert!(!report.contains("command lines"), "{report}");
    }
}
