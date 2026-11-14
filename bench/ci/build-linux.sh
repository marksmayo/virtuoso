#!/bin/sh
# Builds virtuoso's Linux release the way the benchmark measures it, to the path given:
#   sh bench/ci/build-linux.sh out/virtuoso
# Used by both Android bench workflows and, inside WSL, by bench/build.sh. Needs the rustup
# target x86_64-unknown-linux-musl, musl-tools, lld and binutils. Honours CARGO_TARGET_DIR.
#
# Each step measured (docs/install-size-backlog.md):
# - static musl: no dynamic loader, shared libc or start-up relocations, and none of glibc's
#   static NSS and stdio weight (-1.1 MB); non-PIE (relocation-model=static). --version took
#   ~0.3 ms in WSL against ~0.7-0.9 ms for the dynamic glibc PIE;
# - QuickJS compiled without unwind tables, and identical functions folded by lld;
# - no build id, and build paths taken out of panic messages;
# - after linking: .eh_frame (panic=abort unwinds nothing), notes and comments removed.
# --target keeps the flags off build scripts and proc-macros.
set -e
out=${1:?usage: build-linux.sh <output path>}
target=x86_64-unknown-linux-musl
repo=$(pwd)
registry=${CARGO_HOME:-$HOME/.cargo}/registry/src
export CC_x86_64_unknown_linux_musl=musl-gcc
export CFLAGS_x86_64_unknown_linux_musl='-fno-asynchronous-unwind-tables -fno-unwind-tables'
RUSTFLAGS="-C target-feature=+crt-static -C relocation-model=static -C link-arg=-fuse-ld=lld -C link-arg=-Wl,--icf=all -C link-arg=-Wl,--build-id=none --remap-path-prefix=$registry/= --remap-path-prefix=$repo/=" \
  cargo build --release -p virtuoso --target $target
cp "${CARGO_TARGET_DIR:-target}/$target/release/virtuoso" "$out"
objcopy --remove-section .eh_frame --remove-section .eh_frame_hdr --remove-section .comment \
  --remove-section .note.stapsdt --remove-section .note.gnu.build-id "$out"
