//! `sailor version`: la build che sta girando, per confrontarla col timbro di
//! un rilascio (`sailor release <bersaglio> --dry-run` legge lo stesso
//! timbro) senza dover aprire un debugger.

/// La forma di `sailor version`. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor version",
    says_key: "",
}];

pub fn run(_args: &[String]) -> i32 {
    println!("sailor {}", env!("CARGO_PKG_VERSION"));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_exits_clean() {
        assert_eq!(run(&[]), 0);
    }
}
