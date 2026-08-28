//! L'inventario su una casa finta, costruita apposta perché ogni verdetto
//! possa venire diverso da quello atteso.
//!
//! LA DOMANDA CHE QUESTE PROVE DIFENDONO non è «quante cose ci sono»: è «quali
//! non funzionano e nessuno lo sa». Un elenco che dice solo i nomi lo si può
//! fare con `ls`; il valore sta nelle due righe che dicono *plugin spento* e
//! *punta a un file che non esiste* — e quelle due righe sono le sole che, se
//! smettessero di funzionare, lascerebbero l'inventario verde e falso.

use inventory::{collect, Kind, Reach, Root};
use std::fs;
use std::path::{Path, PathBuf};

/// Una casa usa-e-getta sotto la cartella temporanea, cancellata e rifatta a
/// ogni giro: una prova che eredita lo sporco della precedente non prova niente.
fn fake_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("prova-inventario-{name}"));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    home
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn skill(home: &Path, at: &str, name: &str) {
    write(
        &home.join(at),
        &format!("---\nname: {name}\ndescription: che cosa fa {name}\n---\n\n# {name}\n"),
    );
}

#[test]
fn a_skill_inside_a_switched_off_plugin_stays_in_the_list_and_says_why() {
    let home = fake_home("plugin-spento");
    // Un plugin acceso e uno spento, dichiarati come li dichiara Claude Code:
    // la chiave porta la versione dopo la chiocciola.
    write(
        &home.join(".claude/settings.json"),
        r#"{"enabledPlugins": {"acceso@1.0.0": true, "spento@1.0.0": false}}"#,
    );
    skill(
        &home,
        ".claude/plugins/cache/mercato/acceso/skills/prima/SKILL.md",
        "prima",
    );
    skill(
        &home,
        ".claude/plugins/cache/mercato/spento/skills/seconda/SKILL.md",
        "seconda",
    );

    let found = collect(&[Root::home(&home)]);
    let skills = found.of(Kind::Skill);
    assert_eq!(skills.len(), 2, "{skills:#?}");

    let first = skills.iter().find(|e| e.name == "acceso:prima").unwrap();
    assert_eq!(first.reach, Reach::Active);
    assert_eq!(first.origin, "plugin acceso");

    let second = skills.iter().find(|e| e.name == "spento:seconda").unwrap();
    match &second.reach {
        Reach::Inactive(reason) => assert!(reason.contains("spento"), "{reason}"),
        other => panic!("una competenza in un plugin spento risulta {other:?}"),
    }
}

/// Il braccio opposto della prova sopra: acceso lo stesso plugin, la stessa
/// competenza deve cambiare verdetto. Senza questo, «spento» potrebbe essere
/// una risposta costante.
#[test]
fn switching_the_plugin_on_changes_the_verdict_of_the_same_skill() {
    let home = fake_home("plugin-riacceso");
    write(
        &home.join(".claude/settings.json"),
        r#"{"enabledPlugins": {"mercato-mio@1.0.0": true}}"#,
    );
    skill(
        &home,
        ".claude/plugins/cache/mercato/mercato-mio/skills/sola/SKILL.md",
        "sola",
    );

    let found = collect(&[Root::home(&home)]);
    let only = found
        .of(Kind::Skill)
        .into_iter()
        .find(|e| e.name.ends_with("sola"))
        .unwrap();
    assert_eq!(only.reach, Reach::Active, "{only:#?}");
}

#[test]
fn a_hook_pointing_at_a_file_that_is_gone_is_reported_as_dead() {
    let home = fake_home("gancio-morto");
    let script = home.join(".claude/scripts/vivo.sh");
    write(&script, "#!/bin/sh\nexit 0\n");
    write(
        &home.join(".claude/settings.json"),
        &format!(
            r#"{{"hooks": {{
                "PreToolUse": [
                  {{"matcher": "Bash", "hooks": [{{"command": "{} --check"}}]}},
                  {{"matcher": "Write", "hooks": [{{"command": "{}/.claude/scripts/sparito.sh"}}]}}
                ]
            }}}}"#,
            script.to_string_lossy(),
            home.to_string_lossy()
        ),
    );

    let found = collect(&[Root::home(&home)]);
    let hooks = found.of(Kind::Hook);
    assert_eq!(hooks.len(), 2, "{hooks:#?}");

    let alive = hooks.iter().find(|e| e.name.contains("Bash")).unwrap();
    assert_eq!(alive.reach, Reach::Active, "{alive:#?}");

    let dead = hooks.iter().find(|e| e.name.contains("Write")).unwrap();
    match &dead.reach {
        Reach::Inactive(reason) => assert!(reason.contains("sparito.sh"), "{reason}"),
        other => panic!("un gancio che punta al vuoto risulta {other:?}"),
    }
}

/// I due falsi allarmi presi sul disco vero il 28/08/2026, messi qui perché non
/// tornino: la punteggiatura della shell attaccata al percorso, e una parola che
/// comincia per `/` senza essere un file.
#[test]
fn shell_punctuation_and_slash_arguments_do_not_kill_a_living_hook() {
    let home = fake_home("falsi-allarmi");
    let script = home.join(".claude/scripts/vivo.sh");
    write(&script, "#!/bin/sh\nexit 0\n");
    write(
        &home.join(".claude/settings.json"),
        &format!(
            r#"{{"hooks": {{
                "PermissionRequest": [
                  {{"matcher": "*", "hooks": [{{"command": "sh -c 'exec {}';"}}]}}
                ],
                "SessionStart": [
                  {{"matcher": "startup", "hooks": [{{"command": "{} --on /clear"}}]}}
                ]
            }}}}"#,
            script.to_string_lossy(),
            script.to_string_lossy()
        ),
    );

    let found = collect(&[Root::home(&home)]);
    for hook in found.of(Kind::Hook) {
        assert_eq!(hook.reach, Reach::Active, "{hook:#?}");
    }
}

/// Le regole e i comandi di un repo non sono attivi ovunque, e dirlo «attivo»
/// sarebbe la bugia comoda: valgono solo per chi apre una sessione lì dentro.
#[test]
fn what_lives_in_a_repo_is_never_reported_as_active_everywhere() {
    let home = fake_home("regole-di-repo");
    let repo = home.join("lavoro/suo-repo");
    write(
        &repo.join(".claude/rules/una-regola.md"),
        "# Come si fa questa cosa\n\nil testo.\n",
    );
    write(
        &repo.join(".claude/commands/suo-comando.md"),
        "---\ndescription: fa la sua cosa\n---\n\nil corpo.\n",
    );

    let found = collect(&[Root::home(&home), Root::repo(&repo)]);

    let rule = found.of(Kind::Rule).into_iter().next().unwrap();
    assert_eq!(rule.name, "una-regola");
    assert_eq!(rule.description, "Come si fa questa cosa");
    assert_eq!(rule.origin, "repo suo-repo");
    match &rule.reach {
        Reach::Unknown(reason) => assert!(reason.contains("suo-repo"), "{reason}"),
        other => panic!("una regola di repo risulta {other:?}"),
    }

    let command = found.of(Kind::Command).into_iter().next().unwrap();
    assert_eq!(command.name, "/suo-comando");
    assert_eq!(command.description, "fa la sua cosa");
}

/// I due comandi che la prima versione perdeva, entrambi trovati sul disco vero:
/// uno che il modello non può invocare, e uno senza frontmatter. Esistono tutti
/// e due, e una persona li digita.
#[test]
fn a_command_is_listed_even_without_frontmatter_or_model_invocation() {
    let home = fake_home("comandi-nascosti");
    write(
        &home.join(".claude/commands/a-mano.md"),
        "---\ndescription: solo chi digita\ndisable-model-invocation: true\n---\n\nil corpo.\n",
    );
    write(
        &home.join(".claude/commands/nudo.md"),
        "# Il comando nudo\n\nnessun frontmatter, e va bene così.\n",
    );

    let found = collect(&[Root::home(&home)]);
    let commands = found.of(Kind::Command);
    assert_eq!(commands.len(), 2, "{commands:#?}");

    let by_hand = commands.iter().find(|e| e.name == "/a-mano").unwrap();
    assert!(!by_hand.by_model, "{by_hand:#?}");
    assert_eq!(by_hand.description, "solo chi digita");

    let bare = commands.iter().find(|e| e.name == "/nudo").unwrap();
    assert!(bare.by_model);
    assert_eq!(bare.description, "Il comando nudo");
}

/// L'inventario dichiara dove ha guardato. Un elenco che non lo dice non si può
/// smentire: chi legge «zero agenti» non sa se non ce ne sono o se nessuno è
/// andato a vedere.
#[test]
fn the_inventory_names_the_roots_it_walked() {
    let home = fake_home("radici-dichiarate");
    let found = collect(&[Root::home(&home)]);
    assert_eq!(found.roots.len(), 1);
    assert!(found.roots[0].starts_with("casa: "), "{:?}", found.roots);
    assert!(
        found.roots[0].contains("prova-inventario-radici-dichiarate"),
        "{:?}",
        found.roots
    );
}
