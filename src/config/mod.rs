use std::{collections::HashMap, path::PathBuf, str::FromStr};

use anyhow::{bail, Context};
use probe_rs::{flashing::Format, DebugProbeSelector, WireProtocol};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

/// A struct which holds all configs.
#[derive(Debug, Deserialize, Serialize)]
pub struct Configs(HashMap<String, Config>);

/// The main struct holding all the possible config options.
#[derive(Debug, Deserialize, Serialize, StructOpt)]
pub struct Config {
    #[structopt(flatten)]
    general: General,
    #[structopt(flatten)]
    flashing: Flashing,
    #[structopt(flatten)]
    reset: Reset,
    #[structopt(flatten)]
    probe: Probe,
    #[structopt(flatten)]
    gdb: Gdb,
    #[structopt(flatten)]
    logging: Logging,

    #[structopt(short = "V", long = "version")]
    version: bool,
    #[structopt(name = "list-chips", long = "list-chips")]
    list_chips: bool,
    #[structopt(
        name = "list-probes",
        long = "list-probes",
        help = "Lists all the connected probes that can be seen.\n\
        If udev rules or permissions are wrong, some probes might not be listed."
    )]
    list_probes: bool,
    #[structopt(name = "disable-progressbars", long = "disable-progressbars")]
    disable_progressbars: bool,
    #[structopt(long = "dry-run")]
    dry_run: bool,
    // `cargo build` arguments
    #[structopt(name = "binary", long = "bin")]
    bin: Option<String>,
    #[structopt(name = "example", long = "example")]
    example: Option<String>,
    #[structopt(name = "package", short = "p", long = "package")]
    package: Option<String>,
    #[structopt(name = "release", long = "release")]
    release: bool,
    #[structopt(name = "target", long = "target")]
    target: Option<String>,
    #[structopt(name = "PATH", long = "manifest-path", parse(from_os_str))]
    manifest_path: Option<PathBuf>,
    #[structopt(long)]
    no_default_features: bool,
    #[structopt(long)]
    all_features: bool,
    #[structopt(long)]
    features: Vec<String>,
}

impl Config {
    /// Get a reference to the config's general.
    pub fn general(&self) -> &General {
        &self.general
    }

    /// Get a reference to the config's flashing.
    pub fn flashing(&self) -> &Flashing {
        &self.flashing
    }

    /// Get a reference to the config's reset.
    pub fn reset(&self) -> &Reset {
        &self.reset
    }

    /// Get a reference to the config's probe.
    pub fn probe(&self) -> &Probe {
        &self.probe
    }

    /// Get a reference to the config's gdb.
    pub fn gdb(&self) -> &Gdb {
        &self.gdb
    }

    /// Get a reference to the config's logging.
    pub fn logging(&self) -> &Logging {
        &self.logging
    }

    /// Get a reference to the config's version.
    pub fn version(&self) -> bool {
        self.version
    }

    /// Get a reference to the config's list chips.
    pub fn list_chips(&self) -> bool {
        self.list_chips
    }

    /// Get a reference to the config's list probes.
    pub fn list_probes(&self) -> bool {
        self.list_probes
    }

    /// Get a reference to the config's disable progressbars.
    pub fn disable_progressbars(&self) -> bool {
        self.disable_progressbars
    }

    /// Get a reference to the config's dry run.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
}

/// The probe config struct holding all the possible probe options.
#[derive(Debug, Deserialize, Serialize, StructOpt)]
pub struct Probe {
    #[structopt(long = "probe.selector")]
    selector: Option<DebugProbeSelector>,
    #[structopt(long = "probe.usb-vid")]
    usb_vid: Option<String>,
    #[structopt(long = "probe.usb-pid")]
    usb_pid: Option<String>,
    #[structopt(long = "probe.serial")]
    serial: Option<String>,
    #[structopt(long = "probe.protocol")]
    protocol: Option<WireProtocol>,
    #[structopt(long = "probe.speed")]
    speed: Option<u32>,
}

impl Probe {
    pub fn usb_vid(&self) -> &Option<String> {
        &self.usb_vid
    }

    pub fn usb_pid(&self) -> &Option<String> {
        &self.usb_pid
    }

    pub fn serial(&self) -> &Option<String> {
        &self.serial
    }

    pub fn protocol(&self) -> WireProtocol {
        self.protocol.unwrap_or(WireProtocol::Swd)
    }

    pub fn speed(&self) -> Option<u32> {
        self.speed
    }

    /// Get a reference to the probe's selector.
    pub fn selector(&self) -> &Option<DebugProbeSelector> {
        &self.selector
    }
}

/// The flashing config struct holding all the possible flashing options.
#[derive(Debug, Deserialize, Serialize, StructOpt)]
pub struct Flashing {
    #[structopt(long = "flashing.enabled")]
    enabled: Option<bool>,
    #[structopt(long = "flashing.restore-unwritten-bytes")]
    restore_unwritten_bytes: Option<bool>,
    #[structopt(long = "flashing.flash-layout-output-path")]
    flash_layout_output_path: Option<String>,
    #[structopt(long = "flashing.do-chip-erase")]
    do_chip_erase: Option<bool>,
}

impl Flashing {
    pub fn enabled(&self) -> bool {
        if let Some(enabled) = self.enabled {
            enabled
        } else {
            self.restore_unwritten_bytes.is_some()
                || self.flash_layout_output_path.is_some()
                || self.do_chip_erase.is_some()
        }
    }

    pub fn restore_unwritten_bytes(&self) -> bool {
        self.restore_unwritten_bytes.unwrap_or(false)
    }

    pub fn flash_layout_output_path(&self) -> &Option<String> {
        &self.flash_layout_output_path
    }

    pub fn do_chip_erase(&self) -> bool {
        self.do_chip_erase.unwrap_or(false)
    }
}

/// The reset config struct holding all the possible reset options.
#[derive(Debug, Deserialize, Serialize, StructOpt)]
pub struct Reset {
    #[structopt(long = "reset.enabled")]
    enabled: Option<bool>,
    #[structopt(long = "reset.halt-afterwards")]
    #[structopt(long)]
    halt_afterwards: Option<bool>,
}

impl Reset {
    pub fn enabled(&self) -> bool {
        if let Some(enabled) = self.enabled {
            enabled
        } else {
            self.halt_afterwards.is_some()
        }
    }

    pub fn halt_afterwards(&self) -> bool {
        self.halt_afterwards.unwrap_or(false)
    }
}

/// The general config struct holding all the possible general options.
#[derive(Debug, Deserialize, Serialize, StructOpt)]
pub struct General {
    #[structopt(long = "general.chip")]
    chip: Option<String>,
    #[structopt(long = "general.chip-descriptions")]
    chip_descriptions: Vec<String>,
    #[structopt(long = "general.log-level", default_value = "WARN")]
    log_level: log::Level,
    #[structopt(long = "general.derives")]
    derives: Option<String>,
    /// Use this flag to assert the nreset & ntrst pins during attaching the probe to the chip.
    #[structopt(long = "general.connect-under-reset")]
    connect_under_reset: bool,
    #[structopt(
        name = "binary file",
        long = "file",
        help = "The path to the binary file to be flashed."
    )]
    file: Option<String>,
    #[structopt(
        name = "format",
        long = "format",
        help = "The format of the binary file to be flashed. This is only read if the --file option is used.",
        default_value = "ELF"
    )]
    format: Format,
    #[structopt(
        name = "base-address",
        long = "format.base-address",
        help = "The address where to put the binary data in flash. This is only considered for binary files."
    )]
    format_base_address: Option<u32>,
    #[structopt(
        name = "skip",
        long = "format.skip",
        help = "The number of bytes to skip and not to be flashed at the start of the binary. This is only considered for binary files."
    )]
    format_skip: Option<u32>,
    #[structopt(
        name = "directory",
        long = "work-dir",
        help = "The work directory from which cargo-flash should operate from."
    )]
    work_dir: Option<String>,
}

impl General {
    pub fn chip(&self) -> &Option<String> {
        &self.chip
    }

    pub fn chip_descriptions(&self) -> &Vec<String> {
        &self.chip_descriptions
    }

    pub fn log_level(&self) -> log::Level {
        self.log_level
    }

    pub fn connect_under_reset(&self) -> bool {
        self.connect_under_reset
    }

    /// Get a reference to the config's file.
    pub fn file(&self) -> &Option<String> {
        &self.file
    }

    /// Get a reference to the config's format.
    pub fn format(&self) -> Format {
        self.format.clone()
    }

    /// Get a reference to the config's format base address.
    pub fn format_base_address(&self) -> Option<u32> {
        self.format_base_address
    }

    /// Get a reference to the general's format skip.
    pub fn format_skip(&self) -> Option<u32> {
        self.format_skip
    }

    /// Get a reference to the config's work dir.
    pub fn work_dir(&self) -> &Option<String> {
        &self.work_dir
    }
}

/// The logging config struct which controls what logging facilities to use and how.
#[derive(Debug, Deserialize, Serialize, StructOpt)]
pub struct Gdb {
    #[structopt(long = "gdb.enabled")]
    enabled: Option<bool>,
    #[structopt(long = "gdb.socket")]
    socket: Option<String>,
}

impl Gdb {
    pub fn enabled(&self) -> bool {
        if let Some(enabled) = self.enabled {
            enabled
        } else {
            self.socket.is_some()
        }
    }

    pub fn socket(&self) -> &Option<String> {
        &self.socket
    }
}

/// The logging config struct which controls what logging facilities to use and how.
#[derive(Debug, Deserialize, Serialize, StructOpt)]
pub struct Logging {
    #[structopt(long = "logging.enabled")]
    enabled: Option<bool>,
    #[structopt(long = "logging.channels")]
    channels: Vec<Channel>,
}

impl Logging {
    pub fn enabled(&self) -> bool {
        if let Some(enabled) = self.enabled {
            enabled
        } else {
            !self.channels.is_empty()
        }
    }

    pub fn channels(&self) -> &Vec<Channel> {
        &self.channels
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Channel {
    kinds: Vec<ChannelKind>,
    link: LinkKind,
}

impl Channel {
    /// Get a reference to the channel's kind.
    pub fn kinds(&self) -> &Vec<ChannelKind> {
        &self.kinds
    }

    /// Get a reference to the channel's link.
    pub fn link(&self) -> &LinkKind {
        &self.link
    }
}

impl FromStr for Channel {
    type Err = ron::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ron::de::from_str(s)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
pub enum LinkKind {
    Command(String),
    Tcp(String),
    WebSocket(String),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum ChannelKind {
    Rtt {
        up: usize,
        down: usize,
        mode: RttMode,
    },
    Itm {
        mode: ItmMode,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum RttMode {
    Raw,
    String { timestamps: bool },
    StringJson,
    Defmt,
    DefmtJson,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum ItmMode {
    Raw,
    String { timestamps: bool },
    DecodedJson,
}

impl Configs {
    pub fn try_new(name: impl AsRef<str>) -> anyhow::Result<Config> {
        let mut s = config::Config::new();

        // Start off by merging in the default configuration file.
        s.merge(config::File::from_str(
            include_str!("default.yaml"),
            config::FileFormat::Yaml,
        ))?;

        // Ordered list of config files, which are handled in the order specified here.
        let config_files = [
            // Merge in the project-specific configuration files.
            // These files may be added to your git repo.
            // ".embed",
            "Rover",
            // Merge in the local configuration files.
            // These files should not be added to your git repo.
            // ".embed.local",
            // "Embed.local",
            // As described in https://github.com/mehcode/config-rs/issues/101
            // the above lines will not work unless that bug is fixed, until
            // then, we add ".ext" to be replaced with a valid format name.
            // ".embed.local.ext",
            // "Embed.local.ext",
        ];

        for file in &config_files {
            s.merge(config::File::with_name(file).required(false))
                .with_context(|| format!("Failed to merge config file '{}", file))?;
        }

        let map: HashMap<String, serde_json::value::Value> = s.try_into()?;

        let config = match map.get(name.as_ref()) {
            Some(c) => c,
            None => bail!(
                "Cannot find config \"{}\" (available configs: {})",
                name.as_ref(),
                map.keys().cloned().collect::<Vec<String>>().join(", "),
            ),
        };

        let mut s = config::Config::new();

        Self::apply(name.as_ref(), &mut s, config, &map)?;

        // You can deserialize (and thus freeze) the entire configuration
        Ok(s.try_into()?)
    }

    pub fn apply(
        name: &str,
        s: &mut config::Config,
        config: &serde_json::value::Value,
        map: &HashMap<String, serde_json::value::Value>,
    ) -> Result<(), config::ConfigError> {
        // If this config derives from another config, merge the other config first.
        // Do this recursively.
        if let Some(derives) = config
            .get("general")
            .and_then(|g| g.get("derives").and_then(|d| d.as_str()))
            .or(Some("default"))
        {
            if derives == name {
                log::warn!("Endless recursion within the {} config.", derives);
            } else if let Some(dconfig) = map.get(derives) {
                Self::apply(derives, s, dconfig, map)?;
            }
        }

        // Merge this current config.
        s.merge(config::File::from_str(
            // This unwrap can never fail as we just deserialized this. The reverse has to work!
            &serde_json::to_string(&config).unwrap(),
            config::FileFormat::Json,
        ))
        .map(|_| ())
    }
}

#[cfg(test)]
mod test {
    use std::vec;

    use probe_rs::flashing::Format;

    use super::{
        Channel, ChannelKind, Config, Configs, Flashing, Gdb, General, ItmMode, LinkKind, Logging,
        Probe, Reset,
    };

    #[test]
    fn default_config() {
        // Ensure the default config can be parsed.

        let _config = Configs::try_new("default").unwrap();
    }

    #[test]
    fn create_config() {
        let config = Config {
            general: General {
                chip: None,
                chip_descriptions: vec![],
                log_level: log::Level::Info,
                derives: None,
                connect_under_reset: false,
                file: None,
                format: Format::Elf,
                format_base_address: None,
                format_skip: None,
                work_dir: None,
            },
            flashing: Flashing {
                enabled: Some(true),
                restore_unwritten_bytes: None,
                flash_layout_output_path: None,
                do_chip_erase: None,
            },
            reset: Reset {
                enabled: Some(false),
                halt_afterwards: None,
            },
            probe: Probe {
                usb_vid: None,
                usb_pid: None,
                serial: None,
                protocol: None,
                speed: None,
                selector: None,
            },
            gdb: Gdb {
                enabled: None,
                socket: None,
            },
            logging: Logging {
                channels: vec![Channel {
                    kinds: vec![ChannelKind::Itm { mode: ItmMode::Raw }],
                    link: LinkKind::Command("echo".into()),
                }],
                enabled: None,
            },
            version: false,
            list_chips: false,
            list_probes: false,
            disable_progressbars: false,
            bin: None,
            example: None,
            package: None,
            release: false,
            target: None,
            manifest_path: None,
            no_default_features: false,
            all_features: false,
            features: vec![],
            dry_run: false,
        };

        serde_yaml::to_writer(std::io::stdout(), &config).unwrap();
    }
}
