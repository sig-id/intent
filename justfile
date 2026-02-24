default:
    @just --list

build:
    cargo build --release

test:
    cargo test

compile: build
    #!/usr/bin/env bash
    set -euo pipefail
    for dir in examples/*/; do
        name=$(basename "$dir")
        echo "=== Compiling $name ==="
        ./target/release/intent compile "$dir" --output "formal/tla/$name" || true
    done

verify: build
    #!/usr/bin/env bash
    set -euo pipefail
    export CLICOLOR_FORCE=1
    max_jobs=8
    pids=()
    names=()
    declare -A fds
    cleanup() { kill "${pids[@]}" 2>/dev/null; exit 1; }
    trap cleanup INT TERM
    for dir in formal/tla/*/; do
        [ -d "$dir" ] || continue
        name=$(basename "$dir")
        exec {fd}< <(./target/release/intent verify --obligations "$dir" 2>&1)
        fds[$name]=$fd
        echo "Spawned verification worker for $name..."
        pids+=($!)
        names+=("$name")
        if (( ${#pids[@]} >= max_jobs )); then
            wait "${pids[0]}" || true
            pids=("${pids[@]:1}")
        fi
    done
    trap - INT TERM
    failed=0
    for pid in "${pids[@]}"; do
        wait "$pid" || failed=$((failed + 1))
    done
    echo ""
    echo "=== Verification Results ==="
    for name in "${names[@]}"; do
        echo ""
        echo "--- $name ---"
        cat <&"${fds[$name]}"
        exec {fds[$name]}<&-
    done
    echo ""
    [ "$failed" -eq 0 ] && echo "All verifications passed." || echo "$failed verification(s) failed."
