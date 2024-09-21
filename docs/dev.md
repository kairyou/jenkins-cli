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

# cargo add serde --features derive # 序列化/反序列化
# cargo add serde_json # json
# cargo add serde_yaml # yaml
# cargo add toml # toml
# cargo add quick-xml --features "serialize" # xml
# cargo add dirs # dirs::home_dir
# cargo add tokio --features "full" # async/await
# cargo add reqwest --no-default-features --features "json,rustls-tls" # reqwest disable openssl-sys
# cargo add anyhow # 错误处理 thiserror/anyhow
# cargo add once_cell # once_cell::sync::Lazy;

# cargo add chrono # 时间
# cargo add regex # 正则
# cargo add url # url解析
# cargo add base64
# cargo add dialoguer --features "fuzzy-select" # 单选/多选
# cargo add console # 控制台交互 (dialoguer ColorfulTheme)
# cargo add indicatif # 进度条/spinner
# cargo add colored # 颜色
# cargo add crossterm # 终端交互 clear/position
# cargo add libc # c库 #flush_stdin
# cargo add winapi --features "wincon" # windows api #flush_stdin

# cargo add fluent fluent-langneg # i18n
# cargo add sys-locale # get system locale
# cargo add rust-embed # embed files to binary

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

# 分析二进制文件的大小
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
cargo test --test test_history -- --nocapture
cargo test --test test_i18n -- --nocapture
```

#### FAQs

- cargo add xxx 一直卡在 Blocking waiting for file lock on package cache

```sh
ps aux | grep cargo | grep -v grep | awk '{print $2}' | xargs kill -9
```

- 报错 `选择 Jenkins 环境失败: IO error: 函数不正确 （os error 1）`, `IO error: Incorrect function. (os error 1)`
  在低版本的`git-bash.exe`中会出现这个问题, 使用 `git-cmd.exe`, windows自带的`cmd.exe`或者`powershell.exe`都正常

  [dialoguer/console](https://github.com/console-rs/console/issues/35)

  [mintty inputoutput](https://github.com/mintty/mintty/wiki/Tips#inputoutput-interaction-with-alien-programs)

  通过升级git-bash解决, 或在其他终端运行, 如 `cmd.exe`, `powershell.exe`, `wsl.exe`, `git-cmd.exe` 等, 或者用 `winpty jenkins`
  
