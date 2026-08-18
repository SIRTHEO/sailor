//! Cosa conta come «installazione» e cosa come «albero di dipendenze».
//!
//! È la parte del gancio Spotlight che non tocca il mondo: due elenchi chiusi e
//! il riconoscitore che li usa. Il resto — `find`, i file scritti, il messaggio
//! — sta in `claude-hooks/src/spotlight_marker.rs`, perché tocca il disco.

/// Elenco chiuso di ciò che ricrea un albero di dipendenze.
///
/// Un elenco di divieti sarebbe sempre in ritardo sullo strumento: qui si nomina
/// cosa conta. Il confronto è per **sottostringa** sul comando normalizzato,
/// quindi `cd /tmp && pnpm install` conta, e conta anche `echo pnpm install` —
/// un falso positivo che costa una scrittura di file vuoto, non un blocco.
pub const INSTALLERS: &[&str] = &[
    "pnpm install",
    "pnpm add",
    "pnpm update",
    "npm install",
    "npm ci",
    "npm update",
    "yarn install",
    "yarn add",
    "bun install",
    "bun add",
    "cargo build",
    "cargo update",
];

/// Il file che dice a Spotlight di non indicizzare l'albero che lo contiene.
pub const MARKER: &str = ".metadata_never_index";

/// Le cartelle che un'installazione ricrea, e con loro il buco nell'esclusione.
pub const DEPENDENCY_DIRS: &[&str] = &["node_modules", "target"];

/// Quanto in giù si guarda, contando la radice come zero: è il `-maxdepth` di
/// `find`, e va tenuto uguale o il porto cercherebbe in un albero diverso.
pub const MAX_DEPTH: usize = 4;

/// Vero se il comando ricrea un albero di dipendenze.
pub fn is_an_install(command: &str) -> bool {
    let normalised = normalise(command);
    INSTALLERS.iter().any(|k| normalised.contains(k))
}

/// Vero se una cartella con questo nome è un albero di dipendenze.
pub fn is_dependency_dir(name: &str) -> bool {
    DEPENDENCY_DIRS.contains(&name)
}

/// `' '.join(command.split())` di Python: gli spazi si compattano a uno solo.
///
/// Serve perché un comando su più righe — o con un `\` a capo — nominerebbe
/// `pnpm\n  install`, che nessuna sottostringa dell'elenco trova.
fn normalise(command: &str) -> String {
    command
        .split(is_python_space)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lo spazio secondo `str.split()` di Python, che non è quello di Rust.
///
/// `char::is_whitespace` segue la proprietà Unicode White_Space; Python ci
/// aggiunge i quattro separatori di controllo `\x1c`–`\x1f`, che White_Space non
/// contiene. Senza questa riga `pnpm\x1cinstall` combacerebbe da una parte sola.
fn is_python_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riconosce_le_installazioni() {
        for c in [
            "pnpm install",
            "pnpm install --frozen-lockfile",
            "npm ci",
            "npm install --save-dev x",
            "yarn add react",
            "bun install",
            "cd /tmp && pnpm install",
            "cargo build --release",
        ] {
            assert!(is_an_install(c), "{c}");
        }
    }

    #[test]
    fn lascia_stare_il_resto() {
        for c in [
            "pnpm run build",
            "npm test",
            "git status",
            "ls node_modules",
            "pnpm exec tsc",
            "npm run install-check",
        ] {
            assert!(!is_an_install(c), "{c}");
        }
    }

    /// Gli spazi si compattano prima del confronto, e «spazio» è quello di
    /// Python: i separatori di controllo compresi.
    #[test]
    fn gli_spazi_si_compattano_come_in_python() {
        assert!(is_an_install("pnpm\n\n   install"));
        assert!(is_an_install("  pnpm\tinstall  "));
        assert!(is_an_install("pnpm\u{1c}install"));
        // Un a capo dentro la parola non la ricompone: resta `pnpminstall`.
        assert!(!is_an_install("pnpmi\nnstall"));
    }
}
