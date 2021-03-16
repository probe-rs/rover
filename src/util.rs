pub const PACKAGE_NAME: &str = env!("CARGO_PKG_NAME");
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_VERSION: &str = git_version::git_version!(fallback = "crates.io");

include!(concat!(env!("OUT_DIR"), "/probe_rs_version.include"));

pub fn print_version() {
    println!("{}: {} - {}", PACKAGE_NAME, PACKAGE_VERSION, GIT_VERSION);
    println!("probe-rs: {} - {}", PROBE_RS_VERSION, PROBE_RS_COMMIT);
}
