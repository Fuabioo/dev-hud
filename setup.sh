#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
SERVICE_NAME="dev-hud.service"
SERVICE_SRC="${REPO_DIR}/${SERVICE_NAME}"
SERVICE_DST="${HOME}/.config/systemd/user/${SERVICE_NAME}"
BIN_DIR="${HOME}/.local/bin"

usage() {
    echo "usage: $0 <install|uninstall|status>"
    echo
    echo "  install    build release, symlink binaries, enable & start service"
    echo "  uninstall  stop & disable service, remove symlinks and service file"
    echo "  status     show service status and binary locations"
}

do_install() {
    echo "==> Building release..."
    cargo build --release --manifest-path "${REPO_DIR}/Cargo.toml"

    echo "==> Symlinking binaries to ${BIN_DIR}/"
    mkdir -p "${BIN_DIR}"
    ln -sf "${REPO_DIR}/target/release/dev-hud" "${BIN_DIR}/dev-hud"
    ln -sf "${REPO_DIR}/target/release/dev-hud-ctl" "${BIN_DIR}/dev-hud-ctl"

    echo "==> Installing systemd user service..."
    mkdir -p "$(dirname "${SERVICE_DST}")"
    ln -sf "${SERVICE_SRC}" "${SERVICE_DST}"
    systemctl --user daemon-reload
    systemctl --user enable "${SERVICE_NAME}"
    systemctl --user restart "${SERVICE_NAME}"

    echo "==> Done. Service is running:"
    systemctl --user --no-pager status "${SERVICE_NAME}" || true
}

do_uninstall() {
    echo "==> Stopping and disabling service..."
    systemctl --user stop "${SERVICE_NAME}" 2>/dev/null || true
    systemctl --user disable "${SERVICE_NAME}" 2>/dev/null || true

    echo "==> Removing service file..."
    rm -f "${SERVICE_DST}"
    systemctl --user daemon-reload

    echo "==> Removing symlinks..."
    rm -f "${BIN_DIR}/dev-hud"
    rm -f "${BIN_DIR}/dev-hud-ctl"

    echo "==> Done."
}

do_status() {
    echo "Service:"
    systemctl --user --no-pager status "${SERVICE_NAME}" 2>/dev/null || echo "  not installed"
    echo
    echo "Binaries:"
    for bin in dev-hud dev-hud-ctl; do
        target="${BIN_DIR}/${bin}"
        if [ -L "${target}" ]; then
            echo "  ${target} -> $(readlink "${target}")"
        elif [ -f "${target}" ]; then
            echo "  ${target} (file)"
        else
            echo "  ${target} (not found)"
        fi
    done
}

case "${1:-}" in
    install)   do_install ;;
    uninstall) do_uninstall ;;
    status)    do_status ;;
    *)         usage; exit 1 ;;
esac
