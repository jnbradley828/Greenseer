#!/bin/bash
set -e

# build dev
cargo build --release

# clone and build main
rm -rf /tmp/Greenseer_main
git clone --branch main --depth 1 git@github.com:jnbradley828/Greenseer.git /tmp/Greenseer_main
cargo build --release --manifest-path /tmp/Greenseer_main/Cargo.toml

mkdir -p benches/results/vs_main

fastchess \
    -engine cmd=./target/release/Greenseer name=dev \
    -engine cmd=/tmp/Greenseer_main/target/release/Greenseer name=main \
    -each proto=uci tc=1+0.1 \
    -rounds 50 \
    -config outname=/dev/null \
    -concurrency 1 \
    -log engine=false \
    | grep --line-buffered -v "Warning" | tee /dev/tty | grep -v "Started game" > benches/results/vs_main/$(date +%Y%m%d_%H%M%S).txt

# cleanup
rm -rf /tmp/Greenseer_main
