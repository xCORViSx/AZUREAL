# Installation

There are two ways to install AZUREAL: downloading a pre-built binary from
GitHub Releases, or building from source with Cargo.

## Pre-Built Binaries (Recommended)

Download the latest release for your platform from
[GitHub Releases](https://github.com/xCORViSx/azureal/releases).

The AZUREAL binary is **self-installing**. On first run, it detects that it has
not been installed to a standard location and copies itself to the appropriate
path:

| Platform | Install Path |
|----------|-------------|
| macOS | `/usr/local/bin/azureal` (falls back to `~/.local/bin/azureal` if `/usr/local/bin` is not writable) |
| Linux | `/usr/local/bin/azureal` (falls back to `~/.local/bin/azureal` if `/usr/local/bin` is not writable) |
| Windows | `%USERPROFILE%\.azureal\bin\azureal.exe` |

If the binary installs to a user-local path, make sure that path is in your
shell's `PATH` environment variable.

After the self-install completes, you can run `azureal` from any terminal.

## From Source

Clone the repository and build with Cargo:

```sh
git clone https://github.com/xCORViSx/azureal.git
cd azureal
cargo install --path .
```

This compiles a release-optimized binary and places it in `~/.cargo/bin/`,
which is typically already on your `PATH` if you installed Rust via rustup.

### Build Requirements

Building from source requires all items listed in
[Requirements](requirements.md), including Rust (latest stable), LLVM/Clang,
and CMake.

## Verifying the Installation

Run the following to confirm AZUREAL is installed and accessible:

```sh
azureal --version
```

You should see output showing the current version number.

## Updating

**Pre-built binary:** Download the new release and run it. The self-install
mechanism overwrites the previous binary.

**From source:** Pull the latest changes and rebuild:

```sh
git pull
cargo install --path .
```
