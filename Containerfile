# Build environment for •cartridge: Rust + the libraries the UI and gamepad
# support link against. Used by ./container.sh, so nothing is installed on the
# host. Debian 12 keeps glibc old (2.36), so the binary also runs on SteamOS and
# other distros.
FROM docker.io/library/rust:1.99-slim-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        pkg-config libudev-dev libx11-dev libxi-dev libgl1-mesa-dev libasound2-dev \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy rustfmt

WORKDIR /src
