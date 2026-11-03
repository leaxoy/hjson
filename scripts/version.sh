#!/usr/bin/env bash
# The version from Cargo.toml, which the package names are built from.
set -euo pipefail
sed -n 's/^version = "\([^"]*\)".*/\1/p' "$(dirname "${BASH_SOURCE[0]}")/../Cargo.toml" | head -1
