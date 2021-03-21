use std::sync::mpsc::{channel, Sender};
use std::thread::{sleep, spawn, JoinHandle};
use std::time::Duration;
use std::{fmt::Debug, io::Write};
use std::{
    io::Read,
    net::{SocketAddr, TcpListener, TcpStream},
};

use serde::{de::DeserializeOwned, Serialize};

use super::{Updater, UpdaterChannel, Value};

/// An updater which receives and sends it's updates from and to a TCP socket.
/// It supports concurrent connections from multiple clients and handles disconnects and errors gracefully.
pub struct TcpUpdater {
    connection_string: String,
    thread_handle: Option<(JoinHandle<()>, Sender<()>)>,
}

impl TcpUpdater {
    /// Creates a new TCP socket updater.
    pub fn new(connection_string: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
            thread_handle: None,
        }
    }

    /// Writes a message to all connected TCP sockets and removes TCP sockets that are no longer connected.
    fn write_to_all_sockets<O>(sockets: &mut Vec<(TcpStream, SocketAddr)>, update: &Value<O>)
    where
        O: Serialize + Send + Sync + Debug + 'static,
    {
        let mut to_remove = vec![];
        for (i, (socket, addr)) in sockets.iter_mut().enumerate() {
            let update = match update {
                Value::StructuredString(update) => {
                    socket.write(serde_json::to_string(update).unwrap().as_bytes())
                }
                Value::Bytes(bytes) => socket.write(bytes),
                Value::String(string) => socket.write(string.as_bytes()),
            };

            match update {
                Ok(_) => (),
                Err(err) => match err.kind() {
                    std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::BrokenPipe => {
                        log::info!("Socket connection to {} was closed", addr);
                        to_remove.push(i);
                    }
                    std::io::ErrorKind::WouldBlock => {
                        log::error!(
                            "Writing to TCP socket at {} experienced an error: {:?}",
                            addr,
                            err
                        )
                    }
                    _ => log::error!(
                        "Writing to TCP socket at {} experienced an error: {:?}",
                        addr,
                        err
                    ),
                },
            }
        }

        // Remove all closed TCP sockets.
        for i in to_remove.into_iter().rev() {
            sockets.swap_remove(i);
        }
    }

    /// Reads all messages from all connected TCP sockets and removes TCP sockets that are no longer connected.
    fn read_from_all_sockets<I>(
        sockets: &mut Vec<(TcpStream, SocketAddr)>,
        sender: Sender<Value<I>>,
    ) where
        I: DeserializeOwned + Send + Sync + Debug + 'static,
    {
        let mut to_remove = vec![];
        for (i, (socket, addr)) in sockets.iter_mut().enumerate() {
            let mut buffer = Vec::with_capacity(1 << 16);
            match socket.read(&mut buffer) {
                Ok(count) => {
                    buffer.truncate(count);
                    match String::from_utf8(buffer.clone()) {
                        Ok(string) => {
                            let v: Result<I, _> = serde_json::from_str(&string);
                            match v {
                                Ok(update) => {
                                    log::debug!("Parsed JSON: {:#?}", update);
                                    let _ = sender.send(Value::StructuredString(update));
                                }
                                Err(error) => {
                                    log::debug!("Failed to parse JSON: {:#?}", error);
                                    let _ = sender.send(Value::String(string));
                                }
                            }
                        }
                        Err(error) => {
                            log::debug!("Failed to parse string: {:#?}", error);
                            let _ = sender.send(Value::Bytes(buffer));
                        }
                    }
                }
                Err(err) => match err.kind() {
                    std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::BrokenPipe => {
                        log::info!("Socket connection to {} was closed", addr);
                        to_remove.push(i);
                    }
                    std::io::ErrorKind::WouldBlock => {
                        log::error!(
                            "Writing to TCP socket at {} experienced an error: {:?}",
                            addr,
                            err
                        )
                    }
                    _ => log::error!(
                        "Writing to TCP socket at {} experienced an error: {:?}",
                        addr,
                        err
                    ),
                },
            }
        }

        // Remove all closed TCP sockets.
        for i in to_remove.into_iter().rev() {
            sockets.swap_remove(i);
        }
    }
}

impl<I, O> Updater<I, O> for TcpUpdater {
    fn start(&mut self) -> UpdaterChannel<I, O>
    where
        I: DeserializeOwned + Send + Sync + Debug + 'static,
        O: Serialize + Send + Sync + Debug + 'static,
    {
        let mut sockets = Vec::new();

        let (rx, inbound) = channel::<Value<O>>();
        let (outbound, tx) = channel::<Value<I>>();
        let (halt_tx, halt_rx) = channel::<()>();

        log::info!("Opening TCP socket on '{}'", self.connection_string);
        let server = TcpListener::bind(&self.connection_string).unwrap();
        server.set_nonblocking(true).unwrap();

        self.thread_handle = Some((
            spawn(move || {
                let mut incoming = server.incoming();
                loop {
                    // If a halt was requested, cease operations.
                    if halt_rx.try_recv().is_ok() {
                        return ();
                    }

                    // Handle new incoming connections.
                    match incoming.next() {
                        Some(Ok(stream)) => {
                            // Assume we always get a peer addr, so this unwrap is fine.
                            let addr = stream.peer_addr().unwrap();
                            // Try accepting the TCP socket.
                            stream.set_nonblocking(true).unwrap();

                            log::info!("Accepted a new TCP socket connection from {}", addr);
                            sockets.push((stream, addr));
                        }
                        Some(Err(err)) => {
                            if err.kind() == std::io::ErrorKind::WouldBlock {
                            } else {
                                log::error!(
                                    "Connecting to a TCP socket experienced an error: {:?}",
                                    err
                                )
                            }
                        }
                        None => {
                            log::error!("The TCP listener iterator was exhausted. Shutting down TCP socket listener.");
                            return ();
                        }
                    }

                    // Read at max one new message from each socket.
                    Self::read_from_all_sockets(&mut sockets, outbound.clone());

                    // Send at max one pending message to each socket.
                    match inbound.try_recv() {
                        Ok(update) => {
                            Self::write_to_all_sockets(&mut sockets, &update);
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
