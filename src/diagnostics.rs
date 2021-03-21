/// Error handling
use colored::*;
use std::error::Error;
use std::fmt::Write;

use bytesize::ByteSize;

use probe_rs::{
    config::MemoryRegion,
    config::{RegistryError, TargetDescriptionSource},
    flashing::{FileDownloadError, FlashError},
    DebugProbeError, DebugProbeInfo, Error as ProbeRsError, Target, WireProtocol,
};
use probe_rs_cli_util::ArtifactError;

#[derive(Debug, thiserror::Error)]
pub enum RoverError {
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
    #[error("No .defmt section was present in the ELF binary.")]
    NoDefmtSection,
    #[error("Parsing of the defmt data failed.")]
    DefmtParsing(anyhow::Error),
}

pub(crate) fn render_diagnostics(error: RoverError) {
    let (errors_to_omit, hints) = match &error {
        RoverError::NoProbesFound => (
            0,
            vec![
                "If you are on Linux, you most likely need to install the udev rules for your probe.\nSee https://probe.rs/guide/2_probes/udev/ if you do not know how to install them.".into(),
                "If you are on Windows, make sure to install the correct driver. For J-Link usage you will need the https://zadig.akeo.ie/ driver.".into(),
                "For a guide on how to set up your probes, see https://probe.rs/guide/2_probes/.".into(),
            ],
        ),
        RoverError::FailedToReadFamilies(_e) => (
            0,
            vec![],
        ),
        RoverError::FailedToOpenElf { source, path } => (
            0,
            match source.kind() {
                std::io::ErrorKind::NotFound => vec![
                    format!("Make sure the path '{}' is the correct location of your ELF binary.", path)
                ],
                _ => vec![]
            },
        ),
        RoverError::FailedToLoadElfData(e) => match e {
            FileDownloadError::NoLoadableSegments => (
                1,
                vec![
                    "Please make sure your linker script is correct and not missing at all.".into(),
                    "If you are working with Rust, check your `.cargo/config.toml`? If you are new to the rust-embedded ecosystem, please head over to https://github.com/rust-embedded/cortex-m-quickstart.".into()
                ],
            ),
            FileDownloadError::Flash(e) => match e {
                FlashError::NoSuitableNvm {..} => (
                    1,
                    vec![
                        "Make sure the flash region specified in the linkerscript matches the one specified in the datasheet of your chip.".into()
                    ]
                ),
                _ => (
                    1,
                    vec![]
                ),
            },
            _ => (
                1,
                vec![
                    "Make sure you are compiling for the correct architecture of your chip.".into()
                ],
            ),
        },
        RoverError::FailedToOpenProbe(_e) => (
            0,
            vec![
                "This could be a permission issue. Check our guide on how to make all probes work properly on your system: https://probe.rs/guide/2_probes/.".into()
            ],
        ),
        RoverError::MultipleProbesFound { list } => (
            0,
            vec![
                "You can select a probe with the `--probe` argument. See `--help` for how to use it.".into(),
                format!("The following devices were found:\n \
                                        {} \
                                            \
                                        Use '--probe VID:PID'\n \
                                                                \
                                        You can also set the [default.probe] config attribute \
                                        (in your Embed.toml) to select which probe to use. \
                                        For usage examples see https://github.com/probe-rs/cargo-embed/blob/master/src/config/default.toml .",
                                        list.iter().enumerate().map(|(num, link)| format!("[{}]: {:?}\n", num, link)).collect::<String>())
            ],
        ),
        RoverError::FailedToParseCredentials => (
            0,
            vec![
                "Make sure you specify the chip credentials in hex format.".into()
            ]
        ),
        RoverError::FlashingFailed { source, target, target_spec, .. } => generate_flash_error_hints(source, target, target_spec),
        RoverError::FailedChipDescriptionParsing { .. } => (
            0,
            vec![],
        ),
        RoverError::FailedToChangeWorkingDirectory { .. } => (
            0,
            vec![],
        ),
        RoverError::FailedToBuildExternalCargoProject { source, path } => match source {
            ArtifactError::NoArtifacts => (
                1,
                vec![
                    "Use '--example' to specify an example to flash.".into(),
                    "Use '--package' to specify which package to flash in a workspace.".into(),
                ],
            ),
            ArtifactError::MultipleArtifacts => (
                1,
                vec![
                    "Use '--bin' to specify which binary to flash.".into(),
                ],
            ),
            ArtifactError::CargoBuild(_) => (
                1,
                vec![
                    "'cargo build' was not successful. Have a look at the error output above.".into(),
                    format!("Make sure '{}' is indeed a cargo project with a Cargo.toml in it.", path),
                ],
            ),
            _ => (
                1,
                vec![],
            ),
        },
        RoverError::FailedToBuildCargoProject(e) => match e {
            ArtifactError::NoArtifacts => (
                0,
                vec![
                    "Use '--example' to specify an example to flash.".into(),
                    "Use '--package' to specify which package to flash in a workspace.".into(),
                ],
            ),
            ArtifactError::MultipleArtifacts => (
                0,
                vec![
                    "Use '--bin' to specify which binary to flash.".into(),
                ],
            ),
            ArtifactError::CargoBuild(_) => (
                0,
                vec![
                    "'cargo build' was not successful. Have a look at the error output above.".into(),
                    "Make sure the working directory you selected is indeed a cargo project with a Cargo.toml in it.".into()
                ],
            ),
            _ => (
                0,
                vec![],
            ),
        },
        RoverError::ChipNotFound { source, .. } => match source {
            RegistryError::ChipNotFound(_) => (
                0,
                vec![
                    "Did you spell the name of your chip correctly? Capitalization does not matter."
                        .into(),
                    "Maybe your chip is not supported yet. You could add it yourself with our tool here: https://github.com/probe-rs/target-gen.".into(),
                    "You can list all the available chips by passing the `--list-chips` argument.".into(),
                ],
            ),
            _ => (
                0,
                vec![],
            ),
        },
        RoverError::FailedToSelectProtocol { .. } => (
            0,
            vec![],
        ),
        RoverError::FailedToSelectProtocolSpeed { speed, .. } => (
            0,
            vec![
                format!("Try specifying a speed lower than {} kHz", speed)
            ],
        ),
        RoverError::AttachingFailed { source, connect_under_reset } => match source {
            ProbeRsError::ChipNotFound(RegistryError::ChipAutodetectFailed) => (
                0,
                vec![
                    "Try specifying your chip with the `--chip` argument.".into(),
                    "You can list all the available chips by passing the `--list-chips` argument.".into(),
                ],
            ),
            _ => if !connect_under_reset {
                (
                    0,
                    vec![
                        "A hard reset during attaching might help. This will reset the entire chip. Run with `--connect-under-reset` to enable this feature.".into()
                    ],
                )
            } else {
                (
                    0,
                    vec![],
                )
            },
        },
        RoverError::AttachingToCoreFailed(_e) =>  (
            0,
            vec![],
        ),
        RoverError::TargetResetFailed(_e) =>  (
            0,
            vec![],
        ),
        RoverError::TargetResetHaltFailed(_e) => (
            0,
            vec![],
        ),
        RoverError::NoDefmtSection => (
            0,
            vec![],
        ),
        RoverError::DefmtParsing(_e) => (
            1,
            vec![],
        ),
    };

    use std::io::Write;
    let mut stderr = std::io::stderr();
    let mut source = error.source();
    let mut i = 0;
    while let Some(s) = source {
        if hints.is_empty() || i >= errors_to_omit {
            let string = format!("{}: {}", i, s);
            write_with_offset(
                &mut stderr,
                if i == 0 {
                    "Error".red().bold()
                } else {
                    "".red().bold()
                },
                &string,
            );
        } else {
            log::debug!("{}: {}", i, s);
        }
        i += 1;
        source = s.source();
    }

    // if !hints.is_empty() {
    //     let _ = write_with_offset(&mut stderr, "Error".red().bold(), &selected_error);
    // };

    let _ = writeln!(stderr);

    for hint in &hints {
        write_with_offset(&mut stderr, "Hint".blue().bold(), hint);
        let _ = writeln!(stderr);
    }

    let _ = stderr.flush();
}

fn generate_flash_error_hints(
    error: &FlashError,
    target: &Target,
    target_spec: &Option<String>,
) -> (usize, Vec<String>) {
    (
        0,
        match error {
            FlashError::NoSuitableNvm {
                start: _,
                end: _,
                description_source,
            } => {
                if &TargetDescriptionSource::Generic == description_source {
                    return (
                        0,
                        vec![
                            "A generic chip was selected as the target. For flashing, it is necessary to specify a concrete chip.\n\
                            Use `--list-chips` to see all available chips.".to_owned()
                        ]
                    );
                }

                let mut hints = Vec::new();

                let mut hint_available_regions = String::new();

                // Show the available flash regions
                let _ = writeln!(
                    hint_available_regions,
                    "The following flash memory is available for the chip '{}':",
                    target.name
                );

                for memory_region in &target.memory_map {
                    match memory_region {
                        MemoryRegion::Ram(_) => {}
                        MemoryRegion::Generic(_) => {}
                        MemoryRegion::Nvm(flash) => {
                            let _ = writeln!(
                                hint_available_regions,
                                "  {:#010x} - {:#010x} ({})",
                                flash.range.start,
                                flash.range.end,
                                ByteSize((flash.range.end - flash.range.start) as u64)
                                    .to_string_as(true)
                            );
                        }
                    }
                }

                hints.push(hint_available_regions);

                if let Some(target_spec) = target_spec {
                    // Check if the chip specification was unique
                    let matching_chips = probe_rs::config::search_chips(target_spec).unwrap();

                    log::info!(
                        "Searching for all chips for spec '{}', found {}",
                        target_spec,
                        matching_chips.len()
                    );

                    if matching_chips.len() > 1 {
                        let mut non_unique_target_hint = format!("The specified chip '{}' did match multiple possible targets. Try to specify your chip more exactly. The following possible targets were found:\n", target_spec);

                        for target in matching_chips {
                            non_unique_target_hint.push_str(&format!("\t{}\n", target));
                        }

                        hints.push(non_unique_target_hint)
                    }
                }

                hints
            },
            FlashError::EraseFailed { ..} => vec![
                "Perhaps your chip has write protected sectors that need to be cleared?".into(),
                "Perhaps you need the --nmagic linker arg. See https://github.com/rust-embedded/cortex-m-quickstart/pull/95 for more information.".into()
            ],
            _ => vec![],
        }
    )
}

fn write_with_offset(mut output: impl std::io::Write, header: ColoredString, msg: &str) {
    let _ = write!(output, "{: >1$} ", header, 12);

    let mut lines = msg.lines();

    if let Some(first_line) = lines.next() {
        let _ = writeln!(output, "{}", first_line);
    }

    for line in lines {
        let _ = writeln!(output, "            {}", line);
    }
}
