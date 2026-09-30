#!/usr/bin/env bash
# Install Hushdex: the Codex CLI built from this repository, installed under its own name so a
# stock `codex` install is never touched. Both binaries share ~/.codex configuration and auth.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
install_dir="${HUSHDEX_INSTALL_DIR:-${HOME}/.local/bin}"

echo "Building hushdex (release) from ${repo_root} ..."
cargo build --release --manifest-path "${repo_root}/codex-rs/Cargo.toml" --package codex-cli --bin codex

mkdir -p "${install_dir}"
install -m 0755 "${repo_root}/codex-rs/target/release/codex" "${install_dir}/hushdex"

echo "Installed ${install_dir}/hushdex"
echo "Make sure ${install_dir} is on your PATH."
echo "Focus mode is on by default; run hushdex with HUSHDEX_FOCUS=0 to disable it, or use /focus inside a session."
