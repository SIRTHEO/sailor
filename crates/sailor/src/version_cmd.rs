//! `sailor version`: the build that is running, to compare it against a
//! release's stamp (`sailor release <target> --dry-run` reads the same stamp)
//! without having to open a debugger.

/// The shape of `sailor version`. See `flow_cmd::USAGE`.
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
