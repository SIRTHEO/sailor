//! Where a command line keeps what people add to it, read instead of compiled.
//!
//! **THE RULE THIS SERVES.** A product's name belongs in a label, never in a
//! condition — and a constructed path is a condition. Before this, a machine
//! holding a different command line got "you have nothing" with no check
//! failing.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The descriptor that ships. Data, not code: a second product is a file, not a
/// branch.
const BUILT_IN: &str = include_str!("../descriptors/extensions.json");

/// The directory whose `.json` files add to or replace what ships.
pub const EXTENSIONS_DIR: &str = ".config/sailor/extensions.d";

/// One place to look, and the shape of what lives under it.
#[derive(Debug, Clone, Deserialize)]
pub struct Place {
    /// Relative to the home, so that a test can hand it a scratch one.
    pub under: String,
    /// Literal components and `*`, as [`crate::discovery::glob`] reads them.
    pub glob: String,
}

/// A command line, and where it keeps the things people add to it.
#[derive(Debug, Clone, Deserialize)]
pub struct Product {
    pub id: String,
    pub label: String,
    /// The directory it keeps under a home.
    pub home: String,
    /// The directory it keeps inside a project, which need not be the same.
    pub project: String,
    #[serde(default)]
    pub settings: Vec<String>,
    #[serde(default)]
    pub installed_plugins: String,
    #[serde(default)]
    pub plugin_manifest: String,
    /// Under `project`, not under the home: a project declares its own.
    #[serde(default)]
    pub commands: String,
    #[serde(default)]
    pub rules: String,
    #[serde(default)]
    pub skills: Vec<Place>,
    #[serde(default)]
    pub agents: Vec<Place>,
}

impl Product {
    /// A path this product keeps under the given home.
    pub fn in_home(&self, home: &Path, rest: &str) -> PathBuf {
        home.join(rest)
    }

    /// The directory it keeps inside a project root.
    pub fn in_project(&self, root: &Path) -> PathBuf {
        root.join(&self.project)
    }
}

#[derive(Debug, Deserialize)]
struct Catalogue {
    #[serde(default)]
    products: Vec<Product>,
}

/// Every command line declared, the shipped one and whatever a home adds.
///
/// **AN UNREADABLE FILE IS SKIPPED, NOT FATAL.** Somebody else's typo in their
/// own descriptor must not stop the inventory from reporting what it can see;
/// what it could not read is the business of the caller that asked for it.
pub fn declared(home: Option<&Path>) -> Vec<Product> {
    let mut found: Vec<Product> = match serde_json::from_str::<Catalogue>(BUILT_IN) {
        Ok(catalogue) => catalogue.products,
        Err(_) => Vec::new(),
    };
    let Some(home) = home else {
        return found;
    };
    let mut extra: Vec<PathBuf> = match std::fs::read_dir(home.join(EXTENSIONS_DIR)) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|kind| kind == "json"))
            .collect(),
        Err(_) => Vec::new(),
    };
    extra.sort();
    for path in extra {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(catalogue) = serde_json::from_str::<Catalogue>(&text) else {
            continue;
        };
        for product in catalogue.products {
            match found.iter().position(|had| had.id == product.id) {
                Some(at) => found[at] = product,
                None => found.push(product),
            }
        }
    }
    found
}

/// How many command lines were looked at, for a report that must not read as
/// "you have nothing" when the truth is "nobody said where to look".
pub fn how_many_declared(home: Option<&Path>) -> usize {
    declared(home).len()
}

/// What is declared for the machine this runs on.
///
/// For the places that are handed a project root and no home: a project's
/// extensions still belong to a product, and which products exist is a fact
/// about the machine.
pub fn on_this_machine() -> Vec<Product> {
    declared(std::env::var("HOME").ok().map(PathBuf::from).as_deref())
}

/// The directories a project may keep its extensions in, one per product.
pub fn project_dirs() -> Vec<String> {
    on_this_machine()
        .into_iter()
        .map(|product| product.project)
        .filter(|dir| !dir.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_ships_parses_and_declares_at_least_one() {
        let products = declared(None);
        assert!(!products.is_empty(), "the shipped descriptor declares none");
        for product in &products {
            assert!(
                !product.home.is_empty(),
                "{}: no home directory",
                product.id
            );
            assert!(!product.skills.is_empty(), "{}: no skills", product.id);
        }
    }

    /// **THE NAME LIVES HERE AND NOWHERE ELSE**, which is the whole point: the
    /// shipped descriptor is allowed to say it, the code is not.
    #[test]
    fn the_shipped_descriptor_is_where_a_product_is_named() {
        let products = declared(None);
        assert!(
            products.iter().any(|product| product.home.starts_with('.')),
            "a home directory should be a dotted folder under a home"
        );
    }
}
