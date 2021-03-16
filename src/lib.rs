mod config;
mod diagnostics;
mod flashing;
mod util;

use crate::config::Config;
use anyhow::Result;
use colored::*;
use diagnostics::{render_diagnostics, CargoFlashError};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process,
    sync::Arc,
    time::Instant,
};
use std::{panic, sync::Mutex};
use structopt::StructOpt;

use probe_rs::{
    config::TargetSelector,
    flashing::ProgressEvent,
    flashing::{BinOptions, Format},
    DebugProbeSelector, FakeProbe, Probe,
};

use probe_rs_cli_util::{argument_handling, build_artifact, logging, logging::Metadata};

lazy_static::lazy_static! {
    static ref METADATA: Arc<Mutex<Metadata>> = Arc::new(Mutex::new(Metadata {
        release: util::PACKAGE_VERSION.to_string(),
        chip: None,
        probe: None,
        speed: None,
        commit: git_version::git_version!(fallback = "crates.io").to_string(),
    }));
}

const ARGUMENTS_TO_REMOVE: &[&str] = &[
    "chip=",
    "speed=",
    "restore-unwritten",
    "flash-layout=",
    "chip-description-path=",
    "list-chips",
    "list-probes",
    "probe=",
    "file=",
    "format=",
    "work-dir=",
    "disable-progressbars",
    "protocol=",
    "probe-index=",
    "reset-halt",
    "nrf-recover",
    "log=",
    "connect-under-reset",
    "dry-run",
];

pub fn entry(uses_cargo: bool) {
    let next = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        log::info!("{:#?}", &METADATA.lock().unwrap());
        next(info);
    }));

    match main_try(uses_cargo) {
        Ok(_) => (),
        Err(e) => {
            log::info!("{:#?}", &METADATA.lock().unwrap());

            render_diagnostics(e);

            process::exit(1);
        }
    }
}

fn main_try(_uses_cargo: bool) -> Result<(), CargoFlashError> {
    let args = std::env::args();

    // Make sure to collect all the args into a vector so we can manipulate it
    // and pass the filtered arguments to cargo.
    let mut args: Vec<_> = args.collect();

    // When called by Cargo, the first argument after the binary name will be `flash`. If that's the
    // case, remove one argument (`Opt::from_iter` will remove the binary name by itself).
    if args.get(1) == Some(&"flash".to_string()) {
        args.remove(1);
    }

    // Get commandline options.
    let opt = Config::from_iter(&args);

    // If the user instructed us to show the version, show the different info about the binary.
    if opt.version() {
        util::print_version();
        return Ok(());
    }

    logging::init(Some(opt.general().log_level()));

    // If someone wants to list the connected probes, just do that and exit.
    if opt.list_probes() {
        list_connected_probes();
        return Ok(());
    }

    // Load the target description given in the cli parameters.
    for cdp in opt.general().chip_descriptions() {
        probe_rs::config::add_target_from_yaml(&Path::new(cdp)).map_err(|error| {
            CargoFlashError::FailedChipDescriptionParsing {
                source: error,
                path: cdp.clone(),
            }
        })?;
    }

    // If we were instructed to list all available chips, print a list of all the available targets to the commandline.
    if opt.list_chips() {
        print_families()?;
        return Ok(());
    }

    // Determine what chip to use. If none was set in the config or the commandline, use auto.
    let chip = if let Some(chip) = &opt.general().chip() {
        chip.into()
    } else {
        TargetSelector::Auto
    };

    // Store the chip name in the metadata stuct so we can print it as debug information when cargo-flash crashes.
    METADATA.lock().unwrap().chip = Some(format!("{:?}", chip));

    // Always remove the first argument as it is the executable name (cargo-flash) and we don't need that.
    // We cannot do this at the start as structopt will discard the first argument in it's internal parser so it needs to be present.
    args.remove(0);

    // Remove all arguments that `cargo build` does not understand.
    argument_handling::remove_arguments(ARGUMENTS_TO_REMOVE, &mut args);

    // Change the work dir if the user asked to do so. Otherwise use the current working directory
    let work_dir = PathBuf::from(if let Some(work_dir) = opt.general().work_dir() {
        let work_dir = dunce::canonicalize(work_dir.clone()).unwrap();
        std::env::set_current_dir(&work_dir).map_err(|error| {
            CargoFlashError::FailedToChangeWorkingDirectory {
                source: error,
                path: format!("{}", work_dir.display()),
            }
        })?;
        log::info!("Changed working directory to {}", work_dir.display());
        work_dir
    } else {
        dunce::canonicalize(".").unwrap()
    });

    // Get the path to the ELF binary we want to flash.
    // This can either be give from the arguments or can be a cargo build artifact.
    let (path, format): (PathBuf, Format) = if let Some(path) = opt.general().file() {
        (
            path.into(),
            match opt.general().format() {
                Format::Bin(_) => Format::Bin(BinOptions {
                    base_address: opt.general().format_base_address(),
                    skip: opt.general().format_skip().unwrap_or(0),
                }),
                f => f,
            },
        )
    } else {
        // Build the project, and extract the path of the built artifact.
        (
            build_artifact(&work_dir, &args).map_err(|error| {
                if let Some(ref work_dir) = opt.general().work_dir() {
                    CargoFlashError::FailedToBuildExternalCargoProject {
                        source: error,
                        // This unwrap is okay, because if we get this error, the path was properly canonicalized on the internal
                        // `cargo build` step.
                        path: format!(
                            "{}",
                            dunce::canonicalize(work_dir.clone()).unwrap().display()
                        ),
                    }
                } else {
                    CargoFlashError::FailedToBuildCargoProject(error)
                }
            })?,
            Format::Elf,
        )
    };

    // Create a data buffer to be used by the flashloader.
    let mut data_buffer = Vec::new();

    // Try to open the firmware file.
    let mut file = File::open(&path).map_err(|error| CargoFlashError::FailedToOpenElf {
        source: error,
        path: format!("{}", path.display()),
    })?;

    // If we know our target yet (given by the commandline), try and create a flashloader with the firmware data.
    // If we do not know the target yet, try and auto detect and create the flashloader lateron.
    let (target_selector, flash_loader) = if let Some(chip_name) = &opt.general().chip() {
        let target = probe_rs::config::get_target_by_name(chip_name).map_err(|error| {
            CargoFlashError::ChipNotFound {
                source: error,
                name: chip_name.clone(),
            }
        })?;

        let loader = flashing::build_flashloader(
            &target,
            &mut file,
            &format,
            &mut data_buffer,
            opt.flashing().restore_unwritten_bytes(),
        )?;
        (TargetSelector::Specified(target), Some(loader))
    } else {
        (TargetSelector::Auto, None)
    };

    // Try and prepare the probe by opening the probe and selecting the given protocol.
    let mut probe = open_probe(&opt)?;
    probe
        .select_protocol(opt.probe().protocol())
        .map_err(|error| CargoFlashError::FailedToSelectProtocol {
            source: error,
            protocol: opt.probe().protocol(),
        })?;

    // Set the protocol speed if some specific speed was given.
    // Return the actual speed the probe has set afterwards.
    // This can deviate from the speed we set as some probes just allow for a set of values and chose the closest one.
    let protocol_speed = if let Some(speed) = opt.probe().speed() {
        let actual_speed = probe.set_speed(speed).map_err(|error| {
            CargoFlashError::FailedToSelectProtocolSpeed {
                source: error,
                speed,
            }
        })?;

        if actual_speed < speed {
            log::warn!(
                "Unable to use specified speed of {} kHz, actual speed used is {} kHz",
                speed,
                actual_speed
            );
        }

        actual_speed
    } else {
        probe.speed_khz()
    };
    // Store the speed in the metadata struct to be able to print it in case of a crash.
    METADATA.lock().unwrap().speed = Some(format!("{:?}", protocol_speed));

    // Log the probe speed.
    log::info!("Protocol speed {} kHz", protocol_speed);

    // Create a new session.
    // If we wanto attach under reset, we do this with a special function call.
    // In this case we assume the target to be known.
    // If we do an attach without a hard reset, we also try to automatically detect the chip at hand to improve the userexperience.
    let mut session = if opt.general().connect_under_reset() {
        probe.attach_under_reset(target_selector)
    } else {
        probe.attach(target_selector)
    }
    .map_err(|error| CargoFlashError::AttachingFailed {
        source: error,
        connect_under_reset: opt.general().connect_under_reset(),
    })?;

    if opt.flashing().enabled() {
        // Start the timer to measure how long flashing took.
        let instant = Instant::now();

        logging::println(format!(
            "    {} {}",
            "Flashing".green().bold(),
            path.display()
        ));

        flashing::run_flash_download(&mut session, &path, &format, &opt, flash_loader)?;
        // .map_err(|e| handle_flash_error(e, session.target(), opt.chip.as_deref()))?;

        // Stop timer.
        let elapsed = instant.elapsed();
        logging::println(format!(
            "    {} in {}s",
            "Finished".green().bold(),
            elapsed.as_millis() as f32 / 1000.0,
        ));

        {
            let mut core = session
                .core(0)
                .map_err(CargoFlashError::AttachingToCoreFailed)?;
            if opt.reset().halt_afterwards() {
                core.reset_and_halt(std::time::Duration::from_millis(500))
                    .map_err(CargoFlashError::TargetResetFailed)?;
            } else {
                core.reset()
                    .map_err(CargoFlashError::TargetResetHaltFailed)?;
            }
        }
    }

    Ok(())
}

/// Print all the available families and their contained chips to the commandline.
fn print_families() -> Result<(), CargoFlashError> {
    logging::println("Available chips:");
    for family in probe_rs::config::families().map_err(CargoFlashError::FailedToReadFamilies)? {
        logging::println(&family.name);
        logging::println("    Variants:");
        for variant in family.variants() {
            logging::println(format!("        {}", variant.name));
        }
    }
    Ok(())
}

/// Lists all connected debug probes.
fn list_connected_probes() {
    let probes = Probe::list_all();

    if !probes.is_empty() {
        logging::println("The following debug probes were found:");
        probes
            .iter()
            .enumerate()
            .for_each(|(num, link)| println!("[{}]: {:?}", num, link));
    } else {
        logging::println("No debug probes were found.");
    }
}

/// Tries to open the debug probe from the given commandline arguments.
/// This ensures that there is only one probe connected or if multiple probes are found,
/// a single one is specified via the commandline parameters.
fn open_probe(config: &Config) -> Result<Probe, CargoFlashError> {
    if config.dry_run() {
        return Ok(Probe::from_specific_probe(Box::new(FakeProbe::new())));
    }

    // If we got a probe selector as an argument, open the probe matching the selector if possible.
    match &config.probe().selector() {
        Some(selector) => Probe::open(selector.clone()).map_err(CargoFlashError::FailedToOpenProbe),
        None => {
            match (config.probe().usb_vid(), config.probe().usb_pid()) {
                (Some(vid), Some(pid)) => {
                    let selector = DebugProbeSelector {
                        vendor_id: u16::from_str_radix(vid, 16)
                            .map_err(|_| CargoFlashError::FailedToParseCredentials)?,
                        product_id: u16::from_str_radix(pid, 16)
                            .map_err(|_| CargoFlashError::FailedToParseCredentials)?,
                        serial_number: config.probe().serial().clone(),
                    };
                    // if two probes with the same VID:PID pair exist we just choose one
                    Probe::open(selector).map_err(CargoFlashError::FailedToOpenProbe)
                }
                _ => {
                    if config.probe().usb_vid().is_some() {
                        log::warn!("USB VID ignored, because PID is not specified.");
                    }
                    if config.probe().usb_pid().is_some() {
                        log::warn!("USB PID ignored, because VID is not specified.");
                    }

                    // Only automatically select a probe if there is only
                    // a single probe detected.
                    let list = Probe::list_all();
                    if list.len() > 1 {
                        Err(CargoFlashError::MultipleProbesFound { list })
                    } else {
                        Probe::open(
                            list.first()
                                .map(|info| {
                                    METADATA.lock().unwrap().probe =
                                        Some(format!("{:?}", info.probe_type));
                                    info
                                })
                                .ok_or_else(|| CargoFlashError::NoProbesFound)?,
                        )
                        .map_err(CargoFlashError::FailedToOpenProbe)
                    }
                }
            }
        }
    }
}
