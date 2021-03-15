mod diagnostics;

mod config;

use crate::config::Config;
use anyhow::Result;
use colored::*;
use diagnostics::render_diagnostics;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    process,
    sync::Arc,
    time::Instant,
};
use std::{panic, sync::Mutex};
use structopt::StructOpt;

use probe_rs::{
    config::{RegistryError, TargetSelector},
    flashing::{BinOptions, Format},
    flashing::{FileDownloadError, FlashError, FlashLoader, FlashProgress, ProgressEvent},
    DebugProbeError, DebugProbeInfo, DebugProbeSelector, FakeProbe, Probe, Session, Target,
    WireProtocol,
};

#[cfg(feature = "sentry")]
use probe_rs_cli_util::logging::{ask_to_log_crash, capture_panic};
use probe_rs_cli_util::{
    argument_handling, build_artifact, logging, logging::Metadata, read_metadata, ArtifactError,
};

const CARGO_NAME: &str = env!("CARGO_PKG_NAME");
const CARGO_VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_VERSION: &str = git_version::git_version!(fallback = "crates.io");

lazy_static::lazy_static! {
    static ref METADATA: Arc<Mutex<Metadata>> = Arc::new(Mutex::new(Metadata {
        release: CARGO_VERSION.to_string(),
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

#[derive(Debug, thiserror::Error)]
enum CargoFlashError {
    #[error("No connected probes were found.")]
    NoProbesFound,
    #[error("Failed to list the target descriptions.")]
    FailedToReadFamilies(#[source] RegistryError),
    #[error("Failed to open the ELF file '{path}' for flashing.")]
    FailedToOpenElf {
        #[source]
        source: std::io::Error,
        path: String,
    },
    #[error("Failed to load the ELF data.")]
    FailedToLoadElfData(#[source] FileDownloadError),
    #[error("Failed to open the debug probe.")]
    FailedToOpenProbe(#[source] DebugProbeError),
    #[error("The given probe credentials could not be parsed.")]
    FailedToParseCredentials,
    #[error("{} probes were found.", .list.len())]
    MultipleProbesFound { list: Vec<DebugProbeInfo> },
    #[error("The flashing procedure failed for '{path}'.")]
    FlashingFailed {
        #[source]
        source: FlashError,
        target: Target,
        target_spec: Option<String>,
        path: String,
    },
    #[error("Failed to parse the chip description '{path}'.")]
    FailedChipDescriptionParsing {
        #[source]
        source: RegistryError,
        path: String,
    },
    #[error("Failed to change the working directory to '{path}'.")]
    FailedToChangeWorkingDirectory {
        #[source]
        source: std::io::Error,
        path: String,
    },
    #[error("Failed to build the cargo project at '{path}'.")]
    FailedToBuildExternalCargoProject {
        #[source]
        source: ArtifactError,
        path: String,
    },
    #[error("Failed to build the cargo project.")]
    FailedToBuildCargoProject(#[source] ArtifactError),
    #[error("The chip '{name}' was not found in the database.")]
    ChipNotFound {
        #[source]
        source: RegistryError,
        name: String,
    },
    #[error("The protocol '{protocol}' could not be selected.")]
    FailedToSelectProtocol {
        #[source]
        source: DebugProbeError,
        protocol: WireProtocol,
    },
    #[error("The protocol speed coudl not be set to '{speed}' kHz.")]
    FailedToSelectProtocolSpeed {
        #[source]
        source: DebugProbeError,
        speed: u32,
    },
    #[error("Connecting to the chip was unsuccessful.")]
    AttachingFailed {
        #[source]
        source: probe_rs::Error,
        connect_under_reset: bool,
    },
    #[error("Failed to get a handle to the first core.")]
    AttachingToCoreFailed(#[source] probe_rs::Error),
    #[error("The reset of the target failed.")]
    TargetResetFailed(#[source] probe_rs::Error),
    #[error("The target could not be reset and halted.")]
    TargetResetHaltFailed(#[source] probe_rs::Error),
}

fn main() {
    let next = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        #[cfg(feature = "sentry")]
        if ask_to_log_crash() {
            capture_panic(&METADATA.lock().unwrap(), &info)
        }
        #[cfg(not(feature = "sentry"))]
        log::info!("{:#?}", &METADATA.lock().unwrap());
        next(info);
    }));

    match main_try() {
        Ok(_) => (),
        Err(e) => {
            #[cfg(not(feature = "sentry"))]
            log::info!("{:#?}", &METADATA.lock().unwrap());

            // Ensure stderr is flushed before calling proces::exit,
            // otherwise the process might panic, because it tries
            // to access stderr during shutdown.
            //
            // We ignore the errors, not much we can do anyway.
            render_diagnostics(e);

            process::exit(1);
        }
    }
}

fn main_try() -> Result<(), CargoFlashError> {
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

    if opt.version() {
        println!(
            "{} {}\ngit commit: {}",
            CARGO_NAME, CARGO_VERSION, GIT_VERSION
        );
        return Ok(());
    }

    logging::init(Some(opt.general().log_level()));

    let work_dir = PathBuf::from(
        opt.general()
            .work_dir()
            .clone()
            .unwrap_or_else(|| ".".to_owned()),
    );

    // If someone wants to list the connected probes, just do that and exit.
    if opt.list_probes() {
        list_connected_probes();
        return Ok(());
    }

    // Load the cargo manifest if it is available and parse the meta object.
    let meta = read_metadata(&work_dir).ok();

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

    // First use command line, then manifest, then default to auto.
    let chip = match (&opt.general().chip(), meta.map(|m| m.chip).flatten()) {
        (Some(c), _) => c.into(),
        (_, Some(c)) => c.into(),
        _ => TargetSelector::Auto,
    };

    // Store the chip name in the metadata stuct so we can print it as debug information when cargo-flash crashes.
    METADATA.lock().unwrap().chip = Some(format!("{:?}", chip));

    // Always remove the first argument as it is the executable name (cargo-flash) and we don't need that.
    // We cannot do this at the start as structopt will discard the first argument in it's internal parser so it needs to be present.
    args.remove(0);

    // Remove all arguments that `cargo build` does not understand.
    argument_handling::remove_arguments(ARGUMENTS_TO_REMOVE, &mut args);

    // Change the work dir if the user asked to do so.
    std::env::set_current_dir(&work_dir).map_err(|error| {
        CargoFlashError::FailedToChangeWorkingDirectory {
            source: error,
            path: format!("{}", work_dir.display()),
        }
    })?;
    log::debug!("Changed working directory to {}", work_dir.display());

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

    logging::println(format!(
        "    {} {}",
        "Flashing".green().bold(),
        path.display()
    ));

    let mut data_buffer = Vec::new();

    // Add data from the ELF.
    let mut file = File::open(&path).map_err(|error| CargoFlashError::FailedToOpenElf {
        source: error,
        path: format!("{}", path.display()),
    })?;

    let (target_selector, flash_loader) = if let Some(chip_name) = &opt.general().chip() {
        let target = probe_rs::config::get_target_by_name(chip_name).map_err(|error| {
            CargoFlashError::ChipNotFound {
                source: error,
                name: chip_name.clone(),
            }
        })?;

        let loader = build_flashloader(
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

    // Set the SWD or JTAG speed.
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

    // Start timer.
    let instant = Instant::now();

    run_flash_download(&mut session, &path, &format, &opt, flash_loader)?;
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
        println!("The following debug probes were found:");
        probes
            .iter()
            .enumerate()
            .for_each(|(num, link)| println!("[{}]: {:?}", num, link));
    } else {
        println!("No debug probes were found.");
    }
}

/// Performs the flash download with the given loader. Ensure that the loader has the data to load already stored.
/// This function also manages the update and display of progress bars.
fn run_flash_download(
    session: &mut Session,
    path: &Path,
    format: &Format,
    config: &Config,
    loader: Option<FlashLoader>,
) -> Result<(), CargoFlashError> {
    let mut buffer = Vec::new();

    // Add data from the ELF.
    let mut file = File::open(&path).map_err(|error| CargoFlashError::FailedToOpenElf {
        source: error,
        path: format!("{}", path.display()),
    })?;

    let mut loader = match loader {
        Some(loader) => loader,
        None => build_flashloader(
            session.target(),
            &mut file,
            format,
            &mut buffer,
            config.flashing().restore_unwritten_bytes(),
        )?,
    };

    if !config.disable_progressbars() {
        // Create progress bars.
        let multi_progress = MultiProgress::new();
        let style = ProgressStyle::default_bar()
                    .tick_chars("⠁⠁⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠈⠈✔")
                    .progress_chars("##-")
                    .template("{msg:.green.bold} {spinner} [{elapsed_precise}] [{wide_bar}] {bytes:>8}/{total_bytes:>8} @ {bytes_per_sec:>10} (eta {eta:3})");

        // Create a new progress bar for the fill progress if filling is enabled.
        let fill_progress = if config.flashing().restore_unwritten_bytes() {
            let fill_progress = Arc::new(multi_progress.add(ProgressBar::new(0)));
            fill_progress.set_style(style.clone());
            fill_progress.set_message("     Reading flash  ");
            Some(fill_progress)
        } else {
            None
        };

        // Create a new progress bar for the erase progress.
        let erase_progress = Arc::new(multi_progress.add(ProgressBar::new(0)));
        {
            logging::set_progress_bar(erase_progress.clone());
        }
        erase_progress.set_style(style.clone());
        erase_progress.set_message("     Erasing sectors");

        // Create a new progress bar for the program progress.
        let program_progress = multi_progress.add(ProgressBar::new(0));
        program_progress.set_style(style);
        program_progress.set_message(" Programming pages  ");

        // Register callback to update the progress.
        let flash_layout_output_path = config.flashing().flash_layout_output_path().clone();
        let progress = FlashProgress::new(move |event| {
            use ProgressEvent::*;
            match event {
                Initialized { flash_layout } => {
                    let total_page_size: u32 = flash_layout.pages().iter().map(|s| s.size()).sum();

                    let total_sector_size: u32 =
                        flash_layout.sectors().iter().map(|s| s.size()).sum();

                    let total_fill_size: u32 = flash_layout.fills().iter().map(|s| s.size()).sum();

                    if let Some(fp) = fill_progress.as_ref() {
                        fp.set_length(total_fill_size as u64)
                    }
                    erase_progress.set_length(total_sector_size as u64);
                    program_progress.set_length(total_page_size as u64);
                    let visualizer = flash_layout.visualize();
                    flash_layout_output_path
                        .as_ref()
                        .map(|path| visualizer.write_svg(path));
                }
                StartedProgramming => {
                    program_progress.enable_steady_tick(100);
                    program_progress.reset_elapsed();
                }
                StartedErasing => {
                    erase_progress.enable_steady_tick(100);
                    erase_progress.reset_elapsed();
                }
                StartedFilling => {
                    if let Some(fp) = fill_progress.as_ref() {
                        fp.enable_steady_tick(100);
                        fp.reset_elapsed();
                    }
                }
                PageProgrammed { size, .. } => {
                    program_progress.inc(size as u64);
                }
                SectorErased { size, .. } => {
                    erase_progress.inc(size as u64);
                }
                PageFilled { size, .. } => {
                    if let Some(fp) = fill_progress.as_ref() {
                        fp.inc(size as u64)
                    };
                }
                FailedErasing => {
                    erase_progress.abandon();
                    program_progress.abandon();
                }
                FinishedErasing => {
                    erase_progress.finish();
                }
                FailedProgramming => {
                    program_progress.abandon();
                }
                FinishedProgramming => {
                    program_progress.finish();
                }
                FailedFilling => {
                    if let Some(fp) = fill_progress.as_ref() {
                        fp.abandon()
                    };
                }
                FinishedFilling => {
                    if let Some(fp) = fill_progress.as_ref() {
                        fp.finish()
                    };
                }
            }
        });

        // Make the multi progresses print.
        // indicatif requires this in a separate thread as this join is a blocking op,
        // but is required for printing multiprogress.
        let progress_thread_handle = std::thread::spawn(move || {
            multi_progress.join().unwrap();
        });

        loader
            .commit(session, &progress, false, config.dry_run())
            .map_err(|error| CargoFlashError::FlashingFailed {
                source: error,
                target: session.target().clone(),
                target_spec: config.general().chip().clone(),
                path: format!("{}", path.display()),
            })?;

        // We don't care if we cannot join this thread.
        let _ = progress_thread_handle.join();
    } else {
        loader
            .commit(
                session,
                &FlashProgress::new(|_| {}),
                false,
                config.dry_run(),
            )
            .map_err(|error| CargoFlashError::FlashingFailed {
                source: error,
                target: session.target().clone(),
                target_spec: config.general().chip().clone(),
                path: format!("{}", path.display()),
            })?;
    }

    Ok(())
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

/// Builds a new flash loader for the given target and ELF.
/// This will check the ELF for validity and check what pages have to be flashed etc.
fn build_flashloader<'data>(
    target: &Target,
    file: &'data mut File,
    format: &Format,
    buffer: &'data mut Vec<Vec<u8>>,
    keep_unwritten: bool,
) -> Result<FlashLoader<'data>, CargoFlashError> {
    // Create the flash loader
    let mut loader = FlashLoader::new(
        target.memory_map.to_vec(),
        keep_unwritten,
        target.source().clone(),
    );

    match format {
        Format::Bin(bin_options) => {
            loader
                .load_bin_data(buffer, file, bin_options.clone())
                .map_err(CargoFlashError::FailedToLoadElfData)?;
        }
        Format::Hex => {
            loader
                .load_hex_data(buffer, file)
                .map_err(CargoFlashError::FailedToLoadElfData)?;
        }
        Format::Elf => {
            loader
                .load_elf_data(buffer, file)
                .map_err(CargoFlashError::FailedToLoadElfData)?;
        }
    }

    Ok(loader)
}
