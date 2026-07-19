#!/bin/bash
set -e

# build dev
cargo build --release

# clone and build main
rm -rf /tmp/Greenseer_main
git clone --branch main --depth 1 git@github.com:jnbradley828/Greenseer.git /tmp/Greenseer_main
cargo build --release --manifest-path /tmp/Greenseer_main/Cargo.toml

mkdir -p benches/diagnostics/vs_main_depth4
mkdir -p benches/diagnostics/pgn/vs_main_depth4
mkdir -p benches/tournament_configs

TIMESTAMP=$(date +%Y%m%d_%H%M%S)

caffeinate -s -i -m fastchess \
    -engine cmd=./target/release/Greenseer name=dev \
    -engine cmd=/tmp/Greenseer_main/target/release/Greenseer name=main \
    -each proto=uci depth=4 \
    -sprt elo0=0 elo1=10 alpha=0.05 beta=0.05 \
    -rounds 50 \
    -config outname=benches/tournament_configs/${TIMESTAMP}.json \
    -concurrency 2 \
    -log engine=false \
    -pgnout file=benches/diagnostics/pgn/vs_main_depth4/${TIMESTAMP}.pgn notation=uci nodes=true seldepth=true nps=true hashfull=true tbhits=true pv=true timeleft=true latency=true \
    | grep --line-buffered -v "Warning" | tee /dev/tty | grep --line-buffered -v "Started game" > benches/diagnostics/vs_main_depth4/${TIMESTAMP}.txt

# cleanup
rm -rf /tmp/Greenseer_main
