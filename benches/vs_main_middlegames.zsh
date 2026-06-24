#!/bin/bash
set -e

# build dev
cargo build --release

# clone and build main
rm -rf /tmp/Greenseer_main
git clone --branch main --depth 1 git@github.com:jnbradley828/Greenseer.git /tmp/Greenseer_main
cargo build --release --manifest-path /tmp/Greenseer_main/Cargo.toml

mkdir -p benches/results/vs_main_middlegames

caffeinate -s -i -m fastchess \
    -engine cmd=./target/release/Greenseer name=dev \
    -engine cmd=/tmp/Greenseer_main/target/release/Greenseer name=main \
    -each proto=uci tc=10+0.1 \
    -sprt elo0=0 elo1=10 alpha=0.05 beta=0.05 \
    -rounds 1000 \
    -config outname=/dev/null \
    -concurrency 1 \
    -log engine=false \
    -openings file=/Users/joshbradley/Desktop/Projects/Current/Greenseer/benches/8moves_v3.pgn format=pgn order=random -repeat \
    | grep --line-buffered -v "Warning" | tee /dev/tty | grep -v "Started game" > benches/results/vs_main_middlegames/$(date +%Y%m%d_%H%M%S).txt

# cleanup
rm -rf /tmp/Greenseer_main
