#!/bin/sh
# Builds the two virtuoso binaries the benchmark compares, embedding one agent jar:
#   target/release/virtuoso.exe   native Windows build (builds the agent from agent/android)
#   /opt/bench/virtuoso           Linux build in the WSL distribution: static musl
# Two builds with different agents on one device delete each other's jar and push their own on
# every run, so the Linux build takes the jar the Windows build just made. The Linux binary is
# then checked against its size budget (crates/virtuoso/tests/size.rs).
#
# Needs musl-tools and lld in the distribution. Run from the repository root in Git Bash:
#   sh bench/build.sh
set -e
distro=${VIRTUOSO_BENCH_WSL:-VirtuosoUbuntu}

cargo build --release -p virtuoso
jar=$(ls -t target/release/build/virtuoso-*/out/virtuoso-agent.jar | head -n 1)
cp "$jar" bench/tools/virtuoso-agent.jar

# Linux: the static musl release, built as the bench workflows build it (bench/ci/build-linux.sh,
# read without the CRLF a Windows checkout gives it).
repo=$(wsl -d "$distro" -u root -- wslpath -a "$(pwd -W)" | tr -d '\r')
MSYS_NO_PATHCONV=1 wsl -d "$distro" -u root -- sh -c "
  set -e
  . /root/.cargo/env
  cd '$repo'
  export VIRTUOSO_AGENT_JAR='$repo/bench/tools/virtuoso-agent.jar'
  tr -d '\r' < bench/ci/build-linux.sh | CARGO_TARGET_DIR=/root/virtuoso-target sh -s /opt/bench/virtuoso
  CARGO_TARGET_DIR=/root/virtuoso-target-test VIRTUOSO_SIZE_BIN=/opt/bench/virtuoso \
    cargo test -q --release -p virtuoso --test size --target x86_64-unknown-linux-musl
"
target/release/virtuoso.exe --version
MSYS_NO_PATHCONV=1 wsl -d "$distro" -u root -- /opt/bench/virtuoso --version
