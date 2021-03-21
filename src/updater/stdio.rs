use std::io::Read;
use std::{fmt::Debug, io::Write};
use std::{process::Child, time::Duration};
use std::{
    process::Command,
    sync::mpsc::{channel, Sender},
};
use std::{
    process::Stdio,
    thread::{sleep, spawn, JoinHandle},
};

use serde::{de::DeserializeOwned, Serialize};

use super::{Updater, UpdaterChannel, Value};

/// An updater which receives and sends it's updates from and to a TCP socket.
/// It supports concurrent connections from multiple clients and handles disconnects and errors gracefully.
pub struct StdioUpdater {
    command: Command,
    thread_handle: Option<(JoinHandle<()>, Sender<()>)>,
}

impl StdioUpdater {
    /// Creates a new TCP socket updater.
    pub fn new(command: impl Into<Command>) -> Self {
        Self {
            command: command.into(),
            thread_handle: None,
        }
    }

    /// Writes a message to all connected TCP sockets and removes TCP sockets that are no longer connected.
    fn write_to_all_sockets<O>(child: &mut Child, update: &Value<O>) -> bool
    where
        O: Serialize + Send + Sync + Debug + 'static,
    {
        let stdin = child.stdin.as_mut().unwrap();
        let update = match update {
            Value::StructuredString(update) => {
                stdin.write(serde_json::to_string(update).unwrap().as_bytes())
            }
            Value::Bytes(bytes) => stdin.write(bytes),
            Value::String(string) => stdin.write(string.as_bytes()),
        };

        match update {
            Ok(_) => true,
            Err(err) => match err.kind() {
                std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::BrokenPipe => {
                    log::info!("Stdin was closed");
                    false
                }
                _ => {
                    log::error!("Writing to stdin experienced an error: {:?}", err);
                    true
                }
            },
        }
    }

    /// Reads all messages from all connected TCP sockets and removes TCP sockets that are no longer connected.
    fn read_from_all_sockets<I>(child: &mut Child, sender: Sender<Value<I>>) -> bool
    where
        I: DeserializeOwned + Send + Sync + Debug + 'static,
    {
        let stdout = child.stdout.as_mut().unwrap();
        let mut buffer = Vec::with_capacity(1 << 16);
        match stdout.read(&mut buffer) {
            Ok(count) => {
                buffer.truncate(count);
                match String::from_utf8(buffer.clone()) {
                    Ok(string) => {
                        let v: Result<I, _> = serde_json::from_str(&string);
                        match v {
                            Ok(update) => {
                                log::debug!("Parsed JSON: {:#?}", update);
                                let _ = sender.send(Value::StructuredString(update));
                                true
                            }
                            Err(error) => {
                                log::debug!("Failed to parse JSON: {:#?}", error);
                                let _ = sender.send(Value::String(string));
                                true
                            }
                        }
                    }
                    Err(error) => {
                        log::debug!("Failed to parse string: {:#?}", error);
                        let _ = sender.send(Value::Bytes(buffer));
                        true
                    }
                }
            }
            Err(err) => match err.kind() {
                std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::BrokenPipe => {
                    log::info!("Stdout was closed");
                    false
                }
                _ => {
                    log::error!("Reading from stdout experienced an error: {:?}", err);
                    true
                }
            },
        }
    }
}

impl<I, O> Updater<I, O> for StdioUpdater {
    fn start(&mut self) -> UpdaterChannel<I, O>
    where
        I: DeserializeOwned + Send + Sync + Debug + 'static,
        O: Serialize + Send + Sync + Debug + 'static,
    {
        let (rx, inbound) = channel::<Value<O>>();
        let (outbound, tx) = channel::<Value<I>>();
        let (halt_tx, halt_rx) = channel::<()>();

        let mut child = self
            .command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Command could not be executed");

        self.thread_handle = Some((
            spawn(move || {
                loop {
                    // If a halt was requested, cease operations.
                    if halt_rx.try_recv().is_ok() {
                        child.kill().unwrap();
                    }

                    // Read at max one new message from each socket.
                    let keep_running = Self::read_from_all_sockets(&mut child, outbound.clone());
                    if !keep_running {
                        return;
                    }

                    // Send at max one pending message to each socket.
                    match inbound.try_recv() {
                        Ok(update) => {
                            let keep_running = Self::write_to_all_sockets(&mut child, &update);
                            if !keep_running {
                                return;
                            }
                        }
                        _ => (),
                    }

                    // Pause the current thread to not use CPU for no reason.
                    sleep(Duration::from_micros(100));
                }
            }),
            halt_tx,
        ));

        UpdaterChannel::new(rx, tx)
    }

    fn stop(&mut self) -> Result<(), ()> {
        let thread_handle = self.thread_handle.take();
        match thread_handle.map(|h| {
            // If we have a running thread, send the request to stop it and then wait for a join.
            // If this unwrap fails the thread has already been destroyed.
            // This cannot be assumed under normal operation conditions. Even with normal fault handling this should never happen.
            // So this unwarp is fine.
            h.1.send(()).unwrap();
            h.0.join()
        }) {
            Some(Err(err)) => {
                log::error!("An error occured during thread execution: {:?}", err);
                Err(())
            }
            _ => Ok(()),
        }
    }
}
