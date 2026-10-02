#!/bin/bash
set -e
cd /root/rust-ft8
tar -xzf rust-ft8-src.tar.gz
export RUSTFLAGS="-C target-cpu=cortex-a55 -C opt-level=3"
/root/.cargo/bin/cargo build --release
echo "BUILD_SUCCESS"
