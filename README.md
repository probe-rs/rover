# TODO:
- erase command
- RTT
- ITM
- GDB
- Backtrace
- verify command
- replace goblin with object

# rover

[![crates.io](https://meritbadge.herokuapp.com/rover)](https://crates.io/crates/rover) [![documentation](https://docs.rs/rover/badge.svg)](https://docs.rs/rover) [![Actions Status](https://github.com/probe-rs/rover/workflows/CI/badge.svg)](https://github.com/probe-rs/rover/actions) [![chat](https://img.shields.io/badge/chat-probe--rs%3Amatrix.org-brightgreen)](https://matrix.to/#/!vhKMWjizPZBgKeknOo:matrix.org)

This crate provides a cargo subcommand to flash ELF binaries onto ARM chips.

Various chip families including but not limited to **nRF5x**, **STM32** and **LPC800** can be flashed using **DAPLink**, **ST-Link** or **J-Link**. To check if your specific chip is supported, use `rover --list-chips`

## Support

If you think rover makes your embedded journey more enjoyable or even earns you money, please consider supporting the project on [Github Sponsors](https://github.com/sponsors/probe-rs/) for better support and more features.

## Installation

You can install this utility with cargo, after installing the
necessary [prerequisites](#prerequisites):

```bash
cargo install rover
```

Binary releases are not available.

## Usage

You can use it like any cargo command would be used

```bash
rover <args>
```

which will then build your binary and download the contents onto the connected target.

### Examples

#### Flash the debug version of the current crate

```bash
rover --general.chip nrf58122
```

#### Specifying manually what options should be used

```bash
rover --release --general.chip nRF51822 --target thumbv6m-none-eabi --example gpio_hal_blinky
```

#### Use a custom chip definition from a non-builtin file

```bash
rover --release --general.chip-descriptions nRF51822.yaml --target thumbv6m-none-eabi --example gpio_hal_blinky
```

### Manually selecting a chip

To manually select a chip, you can use the `--general.chip <chip name>` argument. The chip name is an identifier such as `nRF51822` or `STM32F042`. Capitalization does not matter; Special characters do matter.

### Specifying a chip family description file

You can add a temporary chip family description by using the `--general.chip-descriptions <chip description file paths>` argument. You need to pass it the path to a valid yaml family description.
All the targets of the family will then be added to the registry temporarily and will override existing variants with the same name.
You can use this feature to tinker with a chip family description until it works and then submit it to upstream for inclusion.

### Extracting a chip family description file from a CMSIS-Pack

You can extract the family description file by running [target-gen](https://github.com/probe-rs/target-gen) on a `.pack` file with `cargo run -- file.pack out_dir`. You can obtain the pack from ARM for example. Their online [registry](https://developer.arm.com/tools-and-software/embedded/cmsis/cmsis-search) is a good start :)
You can also reference to an already unziped `pack` directory instead of the `file.pack` archive file.

## Add more chip definitions

If you have a chip you want to flash, feel free to contribute to [probe-rs](https://github.com/probe-rs/probe-rs).


## Building

`rover` can be built using cargo, after installing the necessary prerequisites. See the list below for your operating
system.

### FTDI support

FTDI support is optional. You can enable it with the `ftdi` feature. You also need the correct prerequisites from the next section installed.

### Prerequisites

rover depends on the [libusb](https://libusb.info/) and optionally on [libftdi](https://www.intra2net.com/en/developer/libftdi/) libraries, which need to be installed to build rover.

#### Linux

On Ubuntu, the following packages need to be installed:

```
> sudo apt install -y pkg-config libusb-1.0-0-dev libftdi1-dev
```

#### Windows

On Windows you can use [vcpkg](https://github.com/microsoft/vcpkg#quick-start-windows) to install the prerequisites:

```
# dynamic linking 64-bit
> vcpkg install libftdi1:x64-windows libusb:x64-windows
> set VCPKGRS_DYNAMIC=1

# static linking 64-bit
> vcpkg install libftdi1:x64-windows-static-md libusb:x64-windows-static-md
```

#### macOS

On macOS, [homebrew](https://brew.sh/) is the suggested method to install libftdi:

```
> brew install libftdi
```
