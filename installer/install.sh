#!/bin/sh
# Concern: one-shot installer that downloads the right prebuilt annotated-tree binary for the host | Non-concern: building from source | IO: (host os/arch, version) -> installed binary
#
# Role:  Detect the host OS/arch, download the matching prebuilt release
#        tarball plus its `.sha256`, verify the checksum (aborting on any
#        mismatch — never install an unverified binary), then extract the
#        binary into an install directory and make it executable.
#
# Usage: curl --proto '=https' --tlsv1.2 -LsSf \
#          https://github.com/fredrikolis/annotated-tree/releases/latest/download/annotated-tree-installer.sh | sh
#
# Env overrides:
#   INSTALL_DIR              install location (default: $HOME/.local/bin)
#   ANNOTATED_TREE_BASE_URL  release-asset base URL (default: GitHub latest);
#                            set to e.g. http://localhost:8000 to test against a
#                            local file server.
#
# Portability: POSIX sh only. Uses curl or wget (whichever exists) and
# sha256sum or `shasum -a 256` (whichever exists); fails fast if neither is
# available.

set -eu

BIN="annotated-tree"
DEFAULT_BASE_URL="https://github.com/fredrikolis/annotated-tree/releases/latest/download"
BASE_URL="${ANNOTATED_TREE_BASE_URL:-$DEFAULT_BASE_URL}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

err() {
    printf 'install.sh: error: %s\n' "$1" >&2
    exit 1
}

info() {
    printf 'install.sh: %s\n' "$1" >&2
}

# --- Map `uname -sm` to the release target triple used in release.yml --------
detect_target() {
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)
            case "$arch" in
                x86_64 | amd64) echo "x86_64-unknown-linux-musl" ;;
                aarch64 | arm64) echo "aarch64-unknown-linux-musl" ;;
                *) err "unsupported Linux architecture: $arch" ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64 | amd64) echo "x86_64-apple-darwin" ;;
                arm64 | aarch64) echo "aarch64-apple-darwin" ;;
                *) err "unsupported macOS architecture: $arch" ;;
            esac
            ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            err "Windows is not supported by this installer; use 'cargo install annotated-tree', 'cargo binstall annotated-tree', or download the x86_64-pc-windows-msvc archive from the releases page"
            ;;
        *)
            err "unsupported operating system: $os"
            ;;
    esac
}

# --- Download <url> to <dest>, failing fast on HTTP/transport error ----------
download() {
    url="$1"
    dest="$2"
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -fLsS "$url" -o "$dest" 2>/dev/null ||
            curl -fLsS "$url" -o "$dest" ||
            err "download failed: $url"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$dest" "$url" || err "download failed: $url"
    else
        err "need either curl or wget to download release assets"
    fi
}

# --- Compute the hex sha256 of <file> using whatever tool exists -------------
sha256_of() {
    file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | cut -d ' ' -f 1
    else
        err "need either sha256sum or 'shasum -a 256' to verify the checksum"
    fi
}

main() {
    target="$(detect_target)"
    archive="${BIN}-${target}.tar.gz"
    archive_url="${BASE_URL}/${archive}"
    checksum_url="${archive_url}.sha256"

    info "installing ${BIN} for ${target}"

    tmp="$(mktemp -d)"
    # Clean up the scratch dir on any exit path.
    trap 'rm -rf "$tmp"' EXIT INT TERM

    download "$archive_url" "$tmp/$archive"
    download "$checksum_url" "$tmp/$archive.sha256"

    # The published .sha256 is `<hex>  <filename>`; take the first field only so
    # a bare-hash file also works.
    expected="$(cut -d ' ' -f 1 "$tmp/$archive.sha256")"
    [ -n "$expected" ] || err "checksum file $checksum_url was empty"
    actual="$(sha256_of "$tmp/$archive")"

    if [ "$expected" != "$actual" ]; then
        err "checksum mismatch for ${archive} (expected ${expected}, got ${actual}); refusing to install"
    fi
    info "checksum verified"

    tar -xzf "$tmp/$archive" -C "$tmp" || err "failed to extract ${archive}"
    [ -f "$tmp/$BIN" ] || err "archive did not contain expected binary '${BIN}'"

    mkdir -p "$INSTALL_DIR"
    install_path="$INSTALL_DIR/$BIN"
    # `install` isn't guaranteed on every POSIX box; cp + chmod is universal.
    cp "$tmp/$BIN" "$install_path"
    chmod +x "$install_path"

    info "installed ${BIN} to ${install_path}"
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) ;;
        *) info "note: ${INSTALL_DIR} is not on your PATH; add it, e.g. export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
    esac
}

main
