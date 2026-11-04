#!/usr/bin/env bash
# Run cargo inside the build container (podman or docker): nothing is
# installed on the host. The cargo cache and build output stay in ./target.
#
# Usage:
#   ./container.sh                     # cargo build --release
#   ./container.sh test                # any cargo command
#   ./container.sh clippy --all-targets
#   ENGINE=docker ./container.sh ...   # force an engine
set -euo pipefail
cd "$(dirname "$0")"

ENGINE="${ENGINE:-$(command -v podman || command -v docker || true)}"
if [[ -z "$ENGINE" ]]; then
    echo "error: podman or docker is required" >&2
    exit 1
fi
IMAGE=cartridge-build

# Built from stdin: no build context, so the games folder is never copied.
# Cached after the first run; edit Containerfile to change the toolchain.
"$ENGINE" build -q -t "$IMAGE" - < Containerfile > /dev/null

run_args=(
    --rm
    -v "$PWD:/src"
    # No SELinux relabel of the project (games included); just allow access.
    --security-opt label=disable
    -e CARGO_HOME=/src/target/cargo-home
    -e STEAMGRIDDB_API_KEY
)
# Rootless podman maps container root to you; docker needs your uid explicitly.
[[ "$(basename "$ENGINE")" == docker ]] && run_args+=(--user "$(id -u):$(id -g)")
[[ -t 1 ]] && run_args+=(-t)

if [[ $# -eq 0 ]]; then
    set -- build --release
fi
exec "$ENGINE" run "${run_args[@]}" "$IMAGE" cargo "$@"
