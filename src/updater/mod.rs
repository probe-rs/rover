pub mod stdio;
pub mod tcp;
pub mod websocket;

use std::fmt::Debug;
use std::sync::mpsc::{Receiver, Sender};

use serde::{de::DeserializeOwned, Serialize};

/// The `Updater` trait specifies an interface for a statemachine updater.
/// An `Updater` is basically a self contained unit that runs asynchronously and pushes/receives events to/from mpscs.
pub trait Updater<I, O> {
    /// Starts the `Updater`.
    /// This should never block and run the `Updater` asynchronously.
    fn start(&mut self) -> UpdaterChannel<I, O>
    where
        I: DeserializeOwned + Send + Sync + Debug + 'static,
        O: Serialize + Send + Sync + Debug + 'static;
    /// Stops the `Updater` if currently running.
    /// Returns `Ok` if everything went smooth during the run of the `Updater`.
    /// Returns `Err` if something went wrong during the run of the `Updater`.
    fn stop(&mut self) -> Result<(), ()>;
}

pub enum Value<T> {
    Bytes(Vec<u8>),
    String(String),
    StructuredString(T),
}

/// A complete channel to an updater.
/// Rx and tx naming is done from the user view of the channel, not the `Updater` view.
pub struct UpdaterChannel<I, O>
where
    I: DeserializeOwned + Send + Sync + Debug + 'static,
    O: Serialize + Send + Sync + Debug + 'static,
{
    /// The rx where the user reads data from.
    rx: Receiver<Value<I>>,
    /// The tx where the user sends data to.
    tx: Sender<Value<O>>,
}

impl<I, O> UpdaterChannel<I, O>
where
    I: DeserializeOwned + Send + Sync + Debug + 'static,
    O: Serialize + Send + Sync + Debug + 'static,
{
    /// Creates a new `UpdaterChannel` where crossover is done internally.
    ///
    /// The argument naming is done from the `Updater`s view. Where as the member naming is done from a user point of view.
    pub fn new(rx: Sender<Value<O>>, tx: Receiver<Value<I>>) -> Self {
        Self { rx: tx, tx: rx }
    }

    /// Returns the rx end of the channel.
    pub fn rx(&mut self) -> &mut Receiver<Value<I>> {
        &mut self.rx
    }

    /// Returns the tx end of the channel.
    pub fn tx(&mut self) -> &mut Sender<Value<O>> {
        &mut self.tx
    }
}
