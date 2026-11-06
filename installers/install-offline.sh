#!/usr/bin/env sh
set -eu

script_dir="$(dirname -- "$0")"
bundle_dir="$(cd -- "$script_dir" && pwd)"
exec "$bundle_dir/install.sh" --artifact-dir "$bundle_dir" "$@"
