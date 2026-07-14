#!/bin/bash
set -e

# build dev
cargo build --release

# clone and build main
rm -rf /tmp/Greenseer_main
git clone --branch main --depth 1 git@github.com:jnbradley828/Greenseer.git /tmp/Greenseer_main
cargo build --release --manifest-path /tmp/Greenseer_main/Cargo.toml

mkdir -p benches/results/vs_main_middlegames
mkdir -p benches/tournament_configs

TIMESTAMP=$(date +%Y%m%d_%H%M%S)

caffeinate -s -i -m fastchess \
    -engine cmd=./target/release/Greenseer name=dev \
    -engine cmd=/tmp/Greenseer_main/target/release/Greenseer name=main \
    -each proto=uci tc=1+0.01 \
    -sprt elo0=0 elo1=10 alpha=0.05 beta=0.05 \
    -rounds 250 \
    -config outname=benches/tournament_configs/${TIMESTAMP}_quick.json \
    -concurrency 1 \
    -log engine=false \
    -pgnout file=benches/results/vs_main_middlegames/${TIMESTAMP}_quick.pgn notation=uci nodes=true seldepth=true nps=true hashfull=true tbhits=true pv=true timeleft=true latency=true \
    -openings file=/Users/joshbradley/Desktop/Projects/Current/Greenseer/benches/8moves_v3.pgn format=pgn order=random -repeat \
    | grep --line-buffered -v "Warning" | tee /dev/tty | grep --line-buffered -v "Started game" > benches/results/vs_main_middlegames/${TIMESTAMP}_quick.txt

# cleanup
rm -rf /tmp/Greenseer_main
