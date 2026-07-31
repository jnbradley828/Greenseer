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

mkdir -p benches/results/vs_main_middlegames
mkdir -p benches/results/pgn/vs_main_middlegames
mkdir -p benches/tournament_configs

if [[ -n "$RESUME" ]]; then
    TIMESTAMP="$RESUME"
    caffeinate -s -i -m fastchess \
        -config file=benches/tournament_configs/${TIMESTAMP}_quick.json stats=true \
        | grep --line-buffered -v "Warning" | tee /dev/tty | grep --line-buffered -v "Started game" >> benches/results/vs_main_middlegames/${TIMESTAMP}_quick.txt
else
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    caffeinate -s -i -m fastchess \
        -engine cmd=./target/release/Greenseer name=dev \
        -engine cmd=/tmp/Greenseer_main/target/release/Greenseer name=main \
        -each proto=uci tc=1+0.01 \
        -sprt elo0=0 elo1=10 alpha=0.05 beta=0.05 \
        -rounds 100000 \
        -config outname=benches/tournament_configs/${TIMESTAMP}_quick.json \
        -concurrency 2 \
        -log engine=false \
        -pgnout file=benches/results/pgn/vs_main_middlegames/${TIMESTAMP}_quick.pgn notation=san nodes=true seldepth=true nps=true hashfull=true tbhits=true pv=true timeleft=true latency=true \
        -openings file=/Users/joshbradley/Desktop/Projects/Current/Greenseer/benches/test_suites/UHO_Lichess_4852_v1.epd format=epd order=random -repeat \
        | grep --line-buffered -v "Warning" | tee /dev/tty | grep --line-buffered -v "Started game" > benches/results/vs_main_middlegames/${TIMESTAMP}_quick.txt
fi

# analyze pgn output (avg depth / nps for dev vs main)
python3 python/pgn_analysis.py "$TIMESTAMP" q

# cleanup
rm -rf /tmp/Greenseer_main
