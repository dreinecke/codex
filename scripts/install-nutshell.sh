#!/usr/bin/env bash
# Install Nutshell: the Codex CLI built from this repository, installed under its own name so a
# stock `codex` install is never touched. Both binaries share ~/.codex configuration and auth.
#
# The command on PATH is a tiny wrapper: herdr's agent manifests key on process names it
# knows (`codex`), and the documented hint for a renamed or wrapped agent is the
# HERDR_AGENT environment variable on the foreground process. The wrapper presets
# HERDR_AGENT=codex (an explicit value from the environment always wins) so herdr's
# state detection, sessions, prompts and nudges drive Nutshell panes unmodified.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
install_dir="${NUTSHELL_INSTALL_DIR:-${HUSHDEX_INSTALL_DIR:-${HOME}/.local/bin}}"
libexec_dir="${NUTSHELL_LIBEXEC_DIR:-${HUSHDEX_LIBEXEC_DIR:-${HOME}/.local/libexec/nutshell}}"

echo "Building nutshell and codex-code-mode-host (release) from ${repo_root} ..."
host_target="$(rustc -vV | sed -n 's/^host: //p')"
eval "$(
  CODEX_REPO_ROOT="${repo_root}" python3 - "${repo_root}/scripts" "${host_target}" <<'PY'
import shlex
import sys

scripts_dir, target = sys.argv[1], sys.argv[2]
sys.path.insert(0, scripts_dir)
from codex_package.targets import TARGET_SPECS
from codex_package.v8 import resolve_codex_v8_cargo_env

spec = TARGET_SPECS.get(target)
if spec is not None:
    for key, value in resolve_codex_v8_cargo_env(spec).items():
        print(f"export {key}={shlex.quote(value)}")
PY
)"
cargo build --release --manifest-path "${repo_root}/codex-rs/Cargo.toml" \
  --package codex-cli --bin codex \
  --package codex-code-mode-host --bin codex-code-mode-host

mkdir -p "${install_dir}" "${libexec_dir}"
install -m 0755 "${repo_root}/codex-rs/target/release/codex" "${libexec_dir}/nutshell"
install -m 0755 "${repo_root}/codex-rs/target/release/codex-code-mode-host" "${libexec_dir}/codex-code-mode-host"
ln -sfn "nutshell" "${libexec_dir}/hushdex"
if [ "${libexec_dir}" = "${HOME}/.local/libexec/nutshell" ]; then
  rm -rf "${HOME}/.local/libexec/hushdex"
  ln -sfn "${libexec_dir}" "${HOME}/.local/libexec/hushdex"
fi

# Replace atomically: overwriting in place fails with ETXTBSY while a pane runs the launcher.
wrapper_tmp="$(mktemp "${install_dir}/.nutshell-wrapper.XXXXXX")"
cat > "${wrapper_tmp}" <<'WRAPPER'
#!/bin/sh
# Nutshell launcher: present to herdr as the Codex agent this binary forks.
HERDR_AGENT="${HERDR_AGENT:-codex}"
export HERDR_AGENT
exec "${HOME}/.local/libexec/nutshell/nutshell" "$@"
WRAPPER
chmod 0755 "${wrapper_tmp}"
mv -f "${wrapper_tmp}" "${install_dir}/nutshell"
ln -sfn "nutshell" "${install_dir}/hushdex"

echo "Installed ${install_dir}/nutshell (binary: ${libexec_dir}/nutshell)"
echo "Make sure ${install_dir} is on your PATH."
echo "Focus mode is on by default; run nutshell with NUTSHELL_FOCUS=0 to disable it, or use /focus inside a session."
