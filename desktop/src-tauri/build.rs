fn main() {
    // **A CHANGED PAGE MUST REBUILD THE BINARY, AND CARGO DOES NOT KNOW IT.**
    // The page is embedded at compile time from `frontendDist`, but nothing
    // tells cargo to look there: measured, `npm run build` followed by
    // `cargo build --release` finished in half a second and kept the page from
    // before, which is a window showing old code without saying so — fault 11
    // arriving from the side nobody watches.
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build();
}
