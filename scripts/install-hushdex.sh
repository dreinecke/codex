#!/usr/bin/env bash
# Install Hushdex: the Codex CLI built from this repository, installed under its own name so a
# stock `codex` install is never touched. Both binaries share ~/.codex configuration and auth.
#
# The command on PATH is a tiny wrapper: herdr's agent manifests key on process names it
# knows (`codex`), and the documented hint for a renamed or wrapped agent is the
# HERDR_AGENT environment variable on the foreground process. The wrapper presets
# HERDR_AGENT=codex (an explicit value from the environment always wins) so herdr's
# state detection, sessions, prompts and nudges drive Hushdex panes unmodified.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
install_dir="${HUSHDEX_INSTALL_DIR:-${HOME}/.local/bin}"
libexec_dir="${HUSHDEX_LIBEXEC_DIR:-${HOME}/.local/libexec/hushdex}"

echo "Building hushdex (release) from ${repo_root} ..."
cargo build --release --manifest-path "${repo_root}/codex-rs/Cargo.toml" --package codex-cli --bin codex

mkdir -p "${install_dir}" "${libexec_dir}"
install -m 0755 "${repo_root}/codex-rs/target/release/codex" "${libexec_dir}/hushdex"
cat > "${install_dir}/hushdex" <<'WRAPPER'
#!/bin/sh
# Hushdex launcher: present to herdr as the Codex agent this binary forks.
HERDR_AGENT="${HERDR_AGENT:-codex}"
export HERDR_AGENT
exec "${HOME}/.local/libexec/hushdex/hushdex" "$@"
WRAPPER
chmod 0755 "${install_dir}/hushdex"

echo "Installed ${install_dir}/hushdex (binary: ${libexec_dir}/hushdex)"
echo "Make sure ${install_dir} is on your PATH."
echo "Focus mode is on by default; run hushdex with HUSHDEX_FOCUS=0 to disable it, or use /focus inside a session."
