#!/bin/sh
# Builds the two virtuoso binaries the benchmark compares, embedding one agent jar:
#   target/release/virtuoso.exe   native Windows build (builds the agent from agent/android)
#   /opt/bench/virtuoso           Linux build in the WSL distribution, statically linked
# Two builds with different agents on one device delete each other's jar and push their own on
# every run, so the Linux build takes the jar the Windows build just made.
#
# Run from the repository root in Git Bash: sh bench/build.sh
set -e
distro=${VIRTUOSO_BENCH_WSL:-VirtuosoUbuntu}

cargo build --release -p virtuoso
jar=$(ls -t target/release/build/virtuoso-*/out/virtuoso-agent.jar | head -n 1)
cp "$jar" bench/tools/virtuoso-agent.jar

# Static glibc and non-PIE: no dynamic loader, shared libc or start-up relocations, which
# saves ~1.2 MB of resident memory. --target keeps the flags off build scripts and proc-macros.
repo=$(wsl -d "$distro" -u root -- wslpath -a "$(pwd -W)" | tr -d '\r')
MSYS_NO_PATHCONV=1 wsl -d "$distro" -u root -- sh -c "
  set -e
  . /root/.cargo/env
  cd '$repo'
  VIRTUOSO_AGENT_JAR='$repo/bench/tools/virtuoso-agent.jar' CARGO_TARGET_DIR=/root/virtuoso-target \
    RUSTFLAGS='-C target-feature=+crt-static -C relocation-model=static' \
    cargo build --release -p virtuoso --target x86_64-unknown-linux-gnu
  cp /root/virtuoso-target/x86_64-unknown-linux-gnu/release/virtuoso /opt/bench/virtuoso
"
target/release/virtuoso.exe --version
MSYS_NO_PATHCONV=1 wsl -d "$distro" -u root -- /opt/bench/virtuoso --version
