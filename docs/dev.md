## Development

### Environment Setup

```bash
# install proto
bash <(curl -fsSL https://moonrepo.dev/install/proto.sh)
# install rust
proto install rust
# install cross
cargo install cross

# apt update -y && apt upgrade -y
sudo apt install -y libssl-dev pkg-config

# vscode rust-analyzer , dependi
```

### Initialize

```bash
# cargo init # create new project

# Dependencies
# cargo tree -i openssl-sys # dependency tree
# cargo add clap --features derive # clap = { version = "4.0", features = ["derive"] }

# cargo add serde --features derive # Serialization/deserialization
# cargo add serde_json # JSON
# cargo add serde_yaml # YAML
# cargo add toml # TOML
# cargo add quick-xml --features "serialize" # XML
# cargo add dirs # dirs::home_dir
# cargo add tokio --features "full" # Async/await
# cargo add reqwest --no-default-features --features "json,rustls-tls" # reqwest disable openssl-sys
# cargo add anyhow # Error handling thiserror/anyhow
# cargo add once_cell # once_cell::sync::Lazy

# cargo add chrono # Date/time formatting
# cargo add regex # Regular expressions
# cargo add url # URL parsing
# cargo add base64
# cargo add dialoguer --features "fuzzy-select" # Single/multi-select
# cargo add console # Console interaction (dialoguer ColorfulTheme)
# cargo add indicatif # Progress bar/spinner
# cargo add colored # Colors
# cargo add crossterm # Terminal interaction clear/position
# cargo add libc # C library #flush_stdin
# cargo add winapi --features "wincon" # Windows API #flush_stdin

# cargo add fluent fluent-langneg # i18n
# cargo add sys-locale # Get system locale
# cargo add rust-embed # Embed files to binary

# cargo add semver # Version compare

# cargo add --dev tempfile # For temp files/dirs in tests
```

<!-- 
cargo add spinners # spinner
# cargo add rust-i18n # i18n
  # println!("Current language: {}", rust_i18n::locale().to_string());
  # println!("Available languages: {:?}", rust_i18n::available_locales!());
-->

### Run

```bash
cargo run --
# FORCE_UPDATE_CHECK=true cargo run --
```

### Build

```bash
# Install local cross-compilation toolchain
# sudo apt install -y gcc-mingw-w64
# rustup target add x86_64-pc-windows-gnu

# cargo build --release

cargo build --target x86_64-pc-windows-gnu --release # build for windows
cargo build --target x86_64-unknown-linux-gnu --release # build for linux

# rustup target add aarch64-apple-darwin # for mac m1 build
# rustup target add x86_64-apple-darwin # for mac intel build
# use osxcross for macOs build
# git clone https://github.com/tpoechtrager/osxcross.git /opt/osxcross
# cd /opt/osxcross;
# macOS SDK: https://github.com/tpoechtrager/osxcross/tree/master?tab=readme-ov-file#packaging-the-sdk-on-recent-macos-xcode
# ./tools/gen_sdk_package.sh /path/to/Xcode_12.4.xip # get sdk from xcode
# mv MacOSX10.15.sdk.tar.xz /opt/osxcross/tarballs/
# UNATTENDED=1 OSX_VERSION_MIN=10.7 ./build.sh
# export PATH="$PATH:/opt/osxcross/bin"; export CC=o64-clang; export CXX=o64-clang++;
# cargo build --target x86_64-apple-darwin --release # build for intel mac
# cargo build --target aarch64-apple-darwin --release # build for M1/M2 Mac

# Analyze binary file size
# cargo bloat --release --crates # cargo install cargo-bloat

# = Release
cargo test -v --no-fail-fast # test
cargo clippy # --fix --allow-dirty # Static code analysis - Check for potential errors/performance issues/code style
cargo fmt -- --check # Check code formatting
# cargo doc --no-deps # Generate documentation
# python3 -m http.server 8000 -d ./target/doc/jenkins/ # Preview documentation

# cargo install cargo-release
cargo release patch --execute --no-publish # auto update version and push tag to remote
# publish to cargo.io
# cargo login
cargo publish # publish to crates.io
```

### Test

```bash
# cargo test
cargo test --test test_git_branches -- --nocapture
cargo test --test test_version_compare -- --nocapture
cargo test --test test_jenkins_job_parameter -- --nocapture
cargo test --test test_config -- --nocapture
cargo test --test test_history -- --nocapture
cargo test --test test_i18n -- --nocapture
```

#### FAQs

- `cargo add xxx` command hangs with `Blocking waiting for file lock on package cache`

```sh
ps aux | grep cargo | grep -v grep | awk '{print $2}' | xargs kill -9
```

- `IO error: Incorrect function. (os error 1)`
  This issue occurs in low-version `git-bash.exe`, use `git-cmd.exe`, `cmd.exe` or `powershell.exe` on windows, or `wsl.exe`, `git-cmd.exe` on linux/macos

  [dialoguer/console](https://github.com/console-rs/console/issues/35)

  [mintty inputoutput](https://github.com/mintty/mintty/wiki/Tips#inputoutput-interaction-with-alien-programs)

  To fix this issue, upgrade git-bash, or use other terminals, such as `cmd.exe`, `powershell.exe`, `wsl.exe`, `git-cmd.exe`, or `winpty jenkins`

- `#[cfg(debug_assertions)]` dev mode, `#[cfg(not(debug_assertions))]` release mode

- `cfg!(target_os = "windows")` platform: windows/linux/macos

- `#[cfg(feature = "force_update_check")]` / `[cfg(not(feature = "force_update_check"))]` features
