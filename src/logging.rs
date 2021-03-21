use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{Read, Seek},
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use defmt_elf2table::{Location, Table};
use probe_rs::Session;
use probe_rs_rtt::{DownChannel, Rtt, ScanRegion, UpChannel};

use crate::{
    config::{Channel, ChannelKind, LinkKind, RttMode},
    diagnostics::RoverError,
    updater::{
        stdio::StdioUpdater, tcp::TcpUpdater, websocket::WebsocketUpdater, Updater, UpdaterChannel,
        Value,
    },
};

pub fn run_logging(
    session: Arc<Mutex<Session>>,
    elf_path: impl AsRef<Path>,
    channels: Vec<Channel>,
) -> Result<JoinHandle<Result<(), RoverError>>, RoverError> {
    let mut updaters: HashMap<LinkKind, UpdaterChannel<(), ()>> = HashMap::new();
    for channel in &channels {
        let link = channel.link().clone();
        updaters.insert(
            link.clone(),
            match link {
                LinkKind::Command(command) => StdioUpdater::new(Command::new(command)).start(),
                LinkKind::Tcp(socket) => TcpUpdater::new(socket).start(),
                LinkKind::WebSocket(socket) => WebsocketUpdater::new(socket).start(),
            },
        );
    }

    // Initialize defmt if necessary.
    let mut defmt_state = None;
    for channel in &channels {
        for kind in channel.kinds() {
            match kind {
                ChannelKind::Rtt {
                    up: _up,
                    down: _down,
                    mode,
                } => match mode {
                    RttMode::Defmt | RttMode::DefmtJson => {
                        if defmt_state.is_none() {
                            defmt_state = Some(create_defmt_state(elf_path.as_ref())?);
                            break;
                        }
                    }
                    _ => (),
                },
                _ => (),
            }
        }
    }

    let elf_path = elf_path.as_ref().to_path_buf();

    Ok(std::thread::spawn(move || {
        let _t = std::time::Instant::now();
        // let mut error = None;

        let mut i = 1;

        // t.elapsed().as_millis() as usize) < config.rtt.timeout
        loop {
            log::info!("Initializing RTT (attempt {})...", i);
            i += 1;

            let rtt_header_address = if let Ok(mut file) = File::open(elf_path.as_path()) {
                if let Some(address) = get_rtt_symbol(&mut file) {
                    log::info!("RTT symbol found at address {:x}", address);
                    ScanRegion::Exact(address as u32)
                } else {
                    log::warn!("RTT symbol not found in ELF binary. Scanning RAM for RTT symbols.");
                    ScanRegion::Ram
                }
            } else {
                log::warn!("ELF binary could not be opened. Scanning RAM for RTT symbols.");
                ScanRegion::Ram
            };

            match Rtt::attach_region(session.clone(), &rtt_header_address) {
                Ok(mut rtt) => {
                    log::info!("RTT synbols found.");

                    let mut up_channels = rtt.up_channels().drain().collect::<Vec<_>>();

                    loop {
                        for channel in &channels {
                            for kind in channel.kinds() {
                                match kind {
                                    ChannelKind::Rtt {
                                        up,
                                        down: _down,
                                        mode,
                                    } => {
                                        let mut up_channel = up_channels.get_mut(*up);
                                        let data = if let Some(up_channel) = &mut up_channel {
                                            poll_rtt(up_channel)
                                        } else {
                                            log::warn!("RTT up channel {} does not exist.", up);
                                            vec![]
                                        };

                                        match mode {
                                            RttMode::Raw => {
                                                updaters
                                                    .get_mut(channel.link())
                                                    .map(|v| v.tx().send(Value::Bytes(data)));
                                            }
                                            RttMode::String { timestamps: _ts } => {
                                                let incoming =
                                                    String::from_utf8_lossy(&data).to_string();
                                                updaters
                                                    .get_mut(channel.link())
                                                    .map(|v| v.tx().send(Value::String(incoming)));
                                            }
                                            RttMode::StringJson => {}
                                            RttMode::Defmt => {}
                                            RttMode::DefmtJson => {}
                                        }
                                    }
                                    ChannelKind::Itm { mode: _mode } => {}
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Err(_err) => {
                    log::warn!("Failed to initialize RTT. Retrying.");
                }
            };

            log::warn!("Failed to initialize RTT. Retrying.");
            std::thread::sleep(Duration::from_millis(10));
        }
    }))
}

/// Creates a new defmt state which holds all the information about the defmt symbols.
fn create_defmt_state(
    elf_path: impl AsRef<Path>,
) -> Result<(Table, Option<BTreeMap<u64, Location>>), RoverError> {
    let elf = fs::read(elf_path).unwrap();
    let table = defmt_elf2table::parse(&elf);

    let table = match table {
        Ok(Some(table)) => table,
        Err(e) => Err(RoverError::DefmtParsing(e))?,
        Ok(None) => Err(RoverError::NoDefmtSection)?,
    };

    let locs = {
        let locs =
            defmt_elf2table::get_locations(&elf, &table).map_err(RoverError::DefmtParsing)?;

        if !table.is_empty() && locs.is_empty() {
            log::warn!("Insufficient DWARF info; compile your program with `debug = 2` to enable location info.");
            None
        } else if table.indices().all(|idx| locs.contains_key(&(idx as u64))) {
            Some(locs)
        } else {
            log::warn!("Location info is incomplete; it will be omitted from the output.");
            None
        }
    };

    Ok((table, locs))
}

/// Finds and returns the address of the RTT header in the flash region of the ELF binary.
fn get_rtt_symbol<T: Read + Seek>(file: &mut T) -> Option<u64> {
    let mut buffer = Vec::new();
    if file.read_to_end(&mut buffer).is_ok() {
        if let Ok(binary) = goblin::elf::Elf::parse(&buffer.as_slice()) {
            for sym in &binary.syms {
                if let Some(Ok(name)) = binary.strtab.get(sym.st_name) {
                    if name == "_SEGGER_RTT" {
                        return Some(sym.st_value);
                    }
                }
            }
        }
    }

    log::warn!("No RTT header info was present in the ELF file. Does your firmware run RTT?");
    None
}

/// Polls the RTT target for new data on the specified channel.
///
/// Processes all the new data and adds it to the linebuffer of the respective channel.
pub fn poll_rtt(channel: &mut UpChannel) -> Vec<u8> {
    // TODO: Proper error handling.
    let mut buffer = vec![0; 1 << 16];
    let count = match channel.read(&mut buffer) {
        Ok(count) => count,
        Err(err) => {
            log::error!("\nError reading from RTT: {}", err);
            return vec![];
        }
    };
    buffer.truncate(count);

    return buffer;
}

/// Sends data back to the target.
pub fn push_rtt(channel: &mut DownChannel, data: &[u8]) {
    channel.write(data).unwrap();
}
