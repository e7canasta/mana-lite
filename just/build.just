# Build recipes. Keep the target and binary explicit so Cargo can reuse its cache.

build-mana-lite:
    #!/usr/bin/env bash
    set -euo pipefail

    repo_root="$(git rev-parse --show-toplevel)"
    cd "$repo_root"
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/mana-lite-target}"
    cargo build --release --bin mana-lite
