use std::{
    env, fs,
    io::{Read, Write},
    path::Path,
};

use toml::Value;

fn main() {
    integrate_probe_rs_version(find_probe_rs_version());
}

fn integrate_probe_rs_version(version: (String, Option<String>)) {
    // Write out the file to be included in the module stub
    let out_dir = env::var("OUT_DIR").unwrap();
    let mut versions_file =
        fs::File::create(&Path::new(&out_dir).join("probe_rs_version.include")).unwrap();
    versions_file
        .write_all(format!("pub const PROBE_RS_VERSION: &str = {:?};", version.0).as_ref())
        .unwrap();
    versions_file
        .write_all(
            format!(
                "pub const PROBE_RS_COMMIT: &str = {:?};",
                version.1.unwrap_or_else(|| "crates.io".to_owned())
            )
            .as_ref(),
        )
        .unwrap();
}

fn find_probe_rs_version() -> (String, Option<String>) {
    let mut lock_buf = String::new();
    fs::File::open("Cargo.lock")
        .unwrap()
        .read_to_string(&mut lock_buf)
        .unwrap();
    let lock_toml: Value = lock_buf.parse().unwrap();

    // Get the table of [[package]]s. This is the deep list of dependencies and dependencies of
    // dependencies.
    for package in lock_toml.get("package").unwrap().as_array().unwrap() {
        let package = package.as_table().unwrap();
        let name = package.get("name").unwrap().as_str().unwrap();
        if name == "probe-rs" {
            let version = package.get("version").unwrap().as_str().unwrap();
            let source = package.get("source").unwrap().as_str().unwrap();
            if source.starts_with("git") {
                let hash = source.split("#").last().unwrap();
                return (version.to_owned(), Some(hash.to_owned()));
            }
            return (version.to_owned(), None);
        }
    }

    unreachable!();
}
