# Workshop recipes. Start with one scenario; add the next only after its gate is green.

workshop-01 duration='180': build-mana-lite
    #!/usr/bin/env bash
    set -euo pipefail

    repo_root="$(git rev-parse --show-toplevel)"
    cd "$repo_root"
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/mana-lite-target}"
    export MANA_MODELS_HOME="${MANA_MODELS_HOME:-$repo_root}"

    printf 'scenario: 01-ingest-only\n'
    printf 'duration:  %ss\n' "{{duration}}"
    printf 'source:    home2\n'
    printf 'config:    workshop/scenarios/01-ingest-only/mana.toml\n'

    set +e
    timeout "{{duration}}" cargo run --release --bin mana-lite -- \
        --config workshop/scenarios/01-ingest-only/mana.toml
    status=$?
    set -e

    if [[ "$status" -ne 0 && "$status" -ne 124 ]]; then
        exit "$status"
    fi
