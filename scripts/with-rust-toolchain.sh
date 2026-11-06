#!/usr/bin/env bash
set -euo pipefail

# Resolve one existing local toolchain. This wrapper never downloads or updates Rust.
if command -v cargo >/dev/null 2>&1; then
  cargo_bin=$(command -v cargo)
else
  candidates=()
  while IFS= read -r candidate; do
    candidates+=("$candidate")
  done < <(find /home -mindepth 4 -maxdepth 4 \( -type f -o -type l \) -path '*/.cargo/bin/cargo' -perm -u+x 2>/dev/null | sort)
  if [ "${#candidates[@]}" -ne 1 ]; then
    echo "Code Intelligence requires exactly one discoverable local Cargo toolchain; found ${#candidates[@]}." >&2
    echo "Install/configure it manually or expose cargo through the Worker service PATH." >&2
    exit 127
  fi
  cargo_bin=${candidates[0]}
  toolchain_home=${cargo_bin%/.cargo/bin/cargo}
  export CARGO_HOME="$toolchain_home/.cargo"
  export RUSTUP_HOME="$toolchain_home/.rustup"
  export PATH="$CARGO_HOME/bin:/usr/local/bin:/usr/bin:/bin"
fi

if [ "${1:-}" = "--exec" ]; then
  shift
  if [ "$#" -eq 0 ]; then
    echo "--exec requires a command" >&2
    exit 64
  fi
  exec "$@"
fi

exec "$cargo_bin" "$@"
