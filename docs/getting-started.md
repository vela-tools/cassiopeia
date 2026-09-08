# Getting started

This guide gets Cassiopeia installed and verifies that it runs. Choose a prebuilt binary, build it from source, or run it in Docker.

## Prerequisites

Cassiopeia is written in Rust and builds with Cargo. You need a recent stable toolchain. The workspace uses the 2024 edition, so install Rust 1.85 or newer. If Rust is not installed, install it with [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then either restart your shell or source the Cargo environment for the current session:

```bash
source "$HOME/.cargo/env"
```

GRIB1 is the only source format that needs a system library. Whether you need that library depends on the build you choose, so read the next section before building.

## GRIB1 and ecCodes

GRIB comes in two editions. GRIB2 is handled entirely in Rust and needs nothing extra. GRIB1 is decoded through [ecCodes](https://confluence.ecmwf.int/display/ECC), the ECMWF C library, using the `eccodes-sys` crate. GRIB1 support is controlled by the `grib1` Cargo feature, which is **on by default**. A default build therefore links ecCodes; a build without the feature does not.

You can build with GRIB1 and install the dependencies below, or leave GRIB1 out. Add one Cargo flag to whichever install command you use, as shown in [Installing](#installing).

### System dependencies for GRIB1

A GRIB1 build needs the ecCodes library, `pkg-config` so the build can locate it, and `libclang` for the bindings that `eccodes-sys` generates at build time. Install them with your system package manager.

macOS, with Homebrew:

```bash
brew install eccodes pkg-config
```

Clang is already present on macOS through the Xcode command line tools, so no separate libclang package is needed.

Debian or Ubuntu:

```bash
sudo apt install libeccodes-dev libclang-dev pkg-config
```

If you do not need GRIB1, skip these dependencies and add `--no-default-features` to the install command you choose below. That build does not link ecCodes or require a C library, and GRIB2 still works. It reports GRIB1 input as unsupported instead of decoding it.

## Installing

There are four ways to get a `cassiopeia` binary. The first is the quickest.

### 1. Download a prebuilt release binary (fastest)

Every tagged release includes a binary for each platform on the [releases page](https://github.com/vela-tools/cassiopeia/releases/latest). Linux and macOS builds ship as `cassiopeia-<platform>.tar.xz` for `linux-x86_64`, `linux-aarch64`, and `macos-aarch64`; the Windows build ships as `cassiopeia-windows-x86_64.zip`. Download the archive for your platform, unpack it, and move the binary to a directory on your PATH:

```bash
wget https://github.com/vela-tools/cassiopeia/releases/latest/download/cassiopeia-linux-x86_64.tar.xz
tar -xf cassiopeia-linux-x86_64.tar.xz
install -m 0755 cassiopeia ~/.local/bin/   # or any directory on your PATH
```

Every release also publishes a `.sha256` checksum beside each archive. The checksum covers the archive itself, so download both and verify before unpacking: `sha256sum -c cassiopeia-<platform>.tar.xz.sha256` on Linux, or `shasum -a 256 -c cassiopeia-<platform>.tar.xz.sha256` on macOS.

Each archive also carries a SLSA build provenance attestation, signed with the release workflow's own identity and stored by GitHub. It records the commit, workflow, and run that produced the archive, which a checksum published on the same page cannot. Verify it with the GitHub CLI:

```bash
gh attestation verify cassiopeia-linux-x86_64.tar.xz --repo vela-tools/cassiopeia
```

Add `--signer-workflow vela-tools/cassiopeia/.github/workflows/release.yaml` to pin the check to the release workflow rather than accepting any workflow in the repository.

The release binaries are built with `--no-default-features`, so they do **not** link ecCodes. They can decode GRIB2 with the pure-Rust reader, but report GRIB1 input as unsupported. If you need GRIB1, install ecCodes and build from source with the default features using one of the options below.

### 2. Clone and install onto your PATH (recommended for the examples)

Clone the repository if you want to work through the examples. Each example includes a dataset and a mapping that you can run from the clone. From the project root, install the binary into your Cargo bin directory, which is usually `~/.cargo/bin` and already on your PATH:

```bash
git clone https://github.com/vela-tools/cassiopeia.git
cd Cassiopeia
cargo install --path .
```

The `cassiopeia` command is then available from any directory. When you pull a newer version, run `cargo install` again to rebuild it in place.

### 3. Install directly from git

If you do not need a local clone of the examples, Cargo can build and install the binary directly from the repository:

```bash
cargo install --git https://github.com/vela-tools/cassiopeia.git
```

This puts the binary on your PATH, just like the previous option.

### 4. Build a standalone binary

If you would rather not install anything on your PATH, build the binary in place:

```bash
cargo build --release
```

The binary is left at `target/release/cassiopeia`, and you run it by that path.

To build any of these without GRIB1, add `--no-default-features` to the command.

## Confirm it runs

If you installed onto your PATH, check the command from any directory:

```bash
cassiopeia --version
```

If you built a standalone binary instead, run it by its path:

```bash
./target/release/cassiopeia --version
```

You should see the release number, the edition, the licence, and the build environment. This confirms that the toolchain and features are set up correctly and, for a default build, that ecCodes is linked.

## Run with Docker

If you would rather not install Cassiopeia on the host, run its container image, `ghcr.io/vela-tools/cassiopeia`. The image supports `linux/amd64` and `linux/arm64`. Its entrypoint is the binary, so arguments after the image name go straight to `cassiopeia`. Unlike the release binaries, the image uses the default build and decodes GRIB1 as well as GRIB2.

```bash
docker pull ghcr.io/vela-tools/cassiopeia:latest
docker run --rm ghcr.io/vela-tools/cassiopeia:latest --version
```

To run a mapping, mount the source data and mapping into the container, then refer to them by their container paths:

```bash
docker run --rm -v "$PWD:/data:ro" -v "$PWD/out:/out" \
    ghcr.io/vela-tools/cassiopeia:latest \
    map -i /data/data.csv -m /data/mapping.json5 -o /out
```

That is the basic run. For a real job, you may also need volumes for the Smart Data Models catalog, named mappings, and configuration, plus network access to a broker. The [Docker guide](docker.md) covers those details, along with Compose and scheduling.

## Download the Smart Data Models catalog

Many of the examples validate their output against published [Smart Data Models](https://smartdatamodels.org/) schemas. Cassiopeia keeps a copy of those schemas on disk, so download the catalog once:

```bash
cassiopeia sdm download
```

The command stores the catalog in a local data directory under your platform's application-data folder. Later validation runs can use it offline. Run `cassiopeia sdm list` to inspect the catalog, or use `cassiopeia sdm search <query>` to find a model.

The catalog is optional. Cassiopeia can produce valid NGSI-LD without it, and a model you define yourself does not need a catalog entry. Download it before using the examples or the terminal interfaces, because `cassiopeia explorer` and `cassiopeia wizard` expect the catalog to be present. The [validation guide](validation.md#where-schemas-come-from) explains how stored schemas are named and resolved. The [data-model guide](data-models.md) explains when to use a published model and when to define your own.

## Next steps

- [Concepts](concepts.md): learn what Cassiopeia produces and how a record moves through the pipeline before reading the how-to guides.
- [Running Cassiopeia](running.md): use `cassiopeia map` from the command line once you know the main pieces.
- [Documentation home](index.md): find the full reading path and the reference pages.
