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

# Linux, each step measured (docs/install-size-backlog.md):
# - static musl: no dynamic loader, shared libc or start-up relocations, and none of glibc's
#   static NSS and stdio weight (-1.1 MB); non-PIE (relocation-model=static);
# - QuickJS compiled without unwind tables, and identical functions folded by lld;
# - no build id, and build paths taken out of panic messages;
# - after linking: .eh_frame (panic=abort unwinds nothing), notes and comments removed.
# --target keeps the flags off build scripts and proc-macros.
repo=$(wsl -d "$distro" -u root -- wslpath -a "$(pwd -W)" | tr -d '\r')
MSYS_NO_PATHCONV=1 wsl -d "$distro" -u root -- sh -c "
  set -e
  . /root/.cargo/env
  cd '$repo'
  export VIRTUOSO_AGENT_JAR='$repo/bench/tools/virtuoso-agent.jar' CC_x86_64_unknown_linux_musl=musl-gcc
  export CFLAGS_x86_64_unknown_linux_musl='-fno-asynchronous-unwind-tables -fno-unwind-tables'
  CARGO_TARGET_DIR=/root/virtuoso-target \
    RUSTFLAGS='-C target-feature=+crt-static -C relocation-model=static -C link-arg=-fuse-ld=lld -C link-arg=-Wl,--icf=all -C link-arg=-Wl,--build-id=none --remap-path-prefix=/root/.cargo/registry/src/= --remap-path-prefix=$repo/=' \
    cargo build --release -p virtuoso --target x86_64-unknown-linux-musl
  cp /root/virtuoso-target/x86_64-unknown-linux-musl/release/virtuoso /opt/bench/virtuoso
  objcopy --remove-section .eh_frame --remove-section .eh_frame_hdr --remove-section .comment \
    --remove-section .note.stapsdt --remove-section .note.gnu.build-id /opt/bench/virtuoso
  CARGO_TARGET_DIR=/root/virtuoso-target-test VIRTUOSO_SIZE_BIN=/opt/bench/virtuoso \
    cargo test -q --release -p virtuoso --test size --target x86_64-unknown-linux-musl
"
target/release/virtuoso.exe --version
MSYS_NO_PATHCONV=1 wsl -d "$distro" -u root -- /opt/bench/virtuoso --version
