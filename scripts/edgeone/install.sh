#!/usr/bin/env bash

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
DISTRO=$( ([[ -e "/usr/bin/yum" ]] && echo 'CentOS') || ([[ -e "/usr/bin/apt" ]] && echo 'Debian') || echo 'unknown' )

IS_WINDOWS=$([[ "$OS" == "mingw"* || "$OS" == "msys"* || "$OS" == "cygwin"*  ]] && echo true || echo false)
CLI_NAME="jenkins"
BASE_URL="${JENKINS_CLI_BASE_URL:-}"
VERSION_OVERRIDE="${JENKINS_CLI_VERSION:-}"

if [[ -z "$BASE_URL" ]]; then
  echo "JENKINS_CLI_BASE_URL is required (e.g. https://your-edgeone-domain)"
  exit 1
fi
BASE_URL="${BASE_URL%/}"

cleanup() {
  [[ -f "$FILENAME" ]] && rm "$FILENAME"
}
trap cleanup EXIT

get_filename() {
    local platform
    local arch
    local clibtype="gnu"
    # match arch
    case $ARCH in
        x86_64)
            arch="x86_64"
            ;;
        i386|i686) # 32-bit
            arch="x86_64"
            ;;
        aarch64|arm64) # arm
            arch="aarch64"
            ;;
        *)
            echo "Unsupported architecture: $ARCH" >&2
            exit 1
            ;;
    esac
    # match os
    case $OS in
        linux*)
            platform="unknown-linux"
            clibtype="gnu"
            if ldd --version 2>&1 | grep -qi musl; then
                clibtype="musl"
            fi
            ;;
        darwin*)
            platform="apple-darwin"
            clibtype=""
            ;;
        mingw*|msys*|cygwin*) # windows
            platform="pc-windows"
            # clibtype=$(if [[ $OS == mingw* ]]; then echo "gnu"; else echo "msvc"; fi)
            ;;
        *)
            echo "Unsupported OS: $OS" >&2
            exit 1
            ;;
    esac
    echo "jenkins-${arch}-${platform}${clibtype:+-$clibtype}.tar.gz"
}

get_latest_version() {
  if [[ -n "$VERSION_OVERRIDE" ]]; then
    echo "$VERSION_OVERRIDE"
    return
  fi
  curl -fsSL "${BASE_URL}/latest.txt" | tr -d '[:space:]' || true
}

main() {
    FILENAME=$(get_filename)
    VERSION=$(get_latest_version)
    if [ -n "$VERSION" ]; then
      echo "version: $VERSION"
    fi

    # Download file
    echo "Downloading $FILENAME..."
    DOWNLOAD_URL="${BASE_URL}/${FILENAME}"
    curl -#Lo "$FILENAME" "$DOWNLOAD_URL" || { echo "Failed to download $FILENAME"; exit 1; }

    # Extract file
    echo "Extracting file..."
    tar -xzf "$FILENAME"

    if $IS_WINDOWS; then
        INSTALL_DIR="$HOME/bin"
        mkdir -p "$INSTALL_DIR"
        TARGET_PATH="${INSTALL_DIR}/${CLI_NAME}.exe"
        mv "${CLI_NAME}.exe" $TARGET_PATH
    else
        INSTALL_DIR="/usr/local/bin"
        TARGET_PATH="${INSTALL_DIR}/${CLI_NAME}"
        if [[ -w "$INSTALL_DIR" ]]; then
            mv $CLI_NAME $TARGET_PATH
        else
            sudo mv $CLI_NAME $TARGET_PATH
        fi
        chmod +x $TARGET_PATH
    fi

    cleanup; # cleanup downloaded file

    if [ -x "$TARGET_PATH" ]; then
        echo "$CLI_NAME has been successfully installed to $TARGET_PATH"
        echo "You can use it by running '$CLI_NAME'"
        if $IS_WINDOWS; then
            if command -v powershell &>/dev/null; then
              powershell -command "
                  \$path = [System.Environment]::GetEnvironmentVariable('Path', [System.EnvironmentVariableTarget]::User)
                  \$newPath = [System.Environment]::ExpandEnvironmentVariables('%USERPROFILE%\\bin')
                  if (\$path -split ';' -notcontains \$newPath) {
                      [System.Environment]::SetEnvironmentVariable('Path', \$path + ';' + \$newPath, [System.EnvironmentVariableTarget]::User)
                  }
              "
            fi
        fi
    else
        echo "Failed to install $CLI_NAME"
        exit 1
    fi
}

main
