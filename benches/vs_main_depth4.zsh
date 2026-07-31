#!/bin/bash
set -e

# --resume TIMESTAMP: continue a previous run's SPRT (same config/stats) instead of starting a
# fresh one. still rebuilds both engines every time - a resumed test is only valid if the code
# under test hasn't changed since the run it's continuing.
RESUME=""
if [[ "$1" == "--resume" ]]; then
    RESUME="$2"
fi

# build dev
cargo build --release

# clone and build main
rm -rf /tmp/Greenseer_main
git clone --branch main --depth 1 git@github.com:jnbradley828/Greenseer.git /tmp/Greenseer_main
cargo build --release --manifest-path /tmp/Greenseer_main/Cargo.toml

mkdir -p benches/diagnostics/vs_main_depth4
mkdir -p benches/diagnostics/pgn/vs_main_depth4
mkdir -p benches/tournament_configs

# ignore SIGINT here so ctrl-c stops the fastchess tournament (which still receives the signal
# directly) without killing this script - lets the analysis step below run on partial results.
trap '' INT
if [[ -n "$RESUME" ]]; then
    TIMESTAMP="$RESUME"
    caffeinate -s -i -m fastchess \
        -config file=benches/tournament_configs/${TIMESTAMP}.json stats=true \
        | grep --line-buffered -v "Warning" | tee /dev/tty | grep --line-buffered -v "Started game" >> benches/diagnostics/vs_main_depth4/${TIMESTAMP}.txt || true
else
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
        -pgnout file=benches/diagnostics/pgn/vs_main_depth4/${TIMESTAMP}.pgn notation=san nodes=true seldepth=true nps=true hashfull=true tbhits=true pv=true timeleft=true latency=true \
        | grep --line-buffered -v "Warning" | tee /dev/tty | grep --line-buffered -v "Started game" > benches/diagnostics/vs_main_depth4/${TIMESTAMP}.txt || true
fi
trap - INT

# analyze pgn output (avg depth / nps for dev vs main)
source python/.venv/bin/activate
python3 python/pgn_analysis.py "$TIMESTAMP"
deactivate

# cleanup
rm -rf /tmp/Greenseer_main
