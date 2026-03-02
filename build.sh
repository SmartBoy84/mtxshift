#!/bin/bash
# set -e

export ZIG_GLOBAL_CACHE_DIR=$PWD/target/zig-cache # [src](https://github.com/ziglang/zig/issues/19400) - global cache in home by default for Zig
cargo zigbuild --release --target aarch64-unknown-linux-gnu
ssh hamdan@rasso.local "killall mtxshift"
scp target/aarch64-unknown-linux-gnu/release/mtxshift hamdan@rasso.local:~
ssh hamdan@rasso.local "~/mtxshift"