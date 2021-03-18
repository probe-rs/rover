use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use probe_rs::Session;
use probe_rs_cli_util::logging;

use crate::diagnostics::RoverError;

const DEFAULT_GDB_LINK: &str = "127.0.0.1:1337";

pub fn run_gdb(
    session: Arc<Mutex<Session>>,
    link: Option<String>,
) -> JoinHandle<Result<(), RoverError>> {
    std::thread::spawn(move || {
        let gdb_connection_string = link.as_deref().or(Some(DEFAULT_GDB_LINK));
        // This next unwrap will always resolve as the connection string is always Some(T).
        log::info!(
            "Firing up GDB stub at {}.",
            gdb_connection_string.as_ref().unwrap(),
        );
        if let Err(e) = probe_rs_gdb_server::run(gdb_connection_string, &session) {
            logging::eprintln("During the execution of GDB an error was encountered:");
            logging::eprintln(format!("{:?}", e));
        }

        Ok(())
    })
}
