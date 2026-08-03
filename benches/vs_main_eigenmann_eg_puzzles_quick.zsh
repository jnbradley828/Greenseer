#!/bin/bash
set -e

# --resume TIMESTAMP: continue a previous run's SPRT (same config/stats) instead of starting a
# fresh one. still rebuilds both engines every time - a resumed test is only valid if the code
# under test hasn't changed since the run it's continuing.
# --bounds ELO0 ELO1: sprt indifference region, e.g. --bounds -5 0 for a non-regression test.
# defaults to 0 10 (looking for a clear improvement) if omitted.
RESUME=""
ELO0=0
ELO1=10
while [[ $# -gt 0 ]]; do
    case "$1" in
        --resume)
            RESUME="$2"
            shift 2
            ;;
        --bounds)
            ELO0="$2"
            ELO1="$3"
            shift 3
            ;;
        *)
            shift
            ;;
    esac
done

# build dev
cargo build --release

# clone and build main
rm -rf /tmp/Greenseer_main
git clone --branch main --depth 1 git@github.com:jnbradley828/Greenseer.git /tmp/Greenseer_main
cargo build --release --manifest-path /tmp/Greenseer_main/Cargo.toml

mkdir -p benches/diagnostics/vs_main_eigenmann_eg_puzzles
mkdir -p benches/diagnostics/pgn/vs_main_eigenmann_eg_puzzles
mkdir -p benches/tournament_configs

# ignore SIGINT here so ctrl-c stops the fastchess tournament (which still receives the signal
# directly) without killing this script - lets the analysis step below run on partial results.
trap '' INT
if [[ -n "$RESUME" ]]; then
    TIMESTAMP="$RESUME"
    caffeinate -s -i -m fastchess \
        -config file=benches/tournament_configs/${TIMESTAMP}_quick.json stats=true \
        | grep --line-buffered -v "Warning" | tee /dev/tty | grep --line-buffered -v "Started game" >> benches/diagnostics/vs_main_eigenmann_eg_puzzles/${TIMESTAMP}_quick.txt || true
else
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    caffeinate -s -i -m fastchess \
        -engine cmd=./target/release/Greenseer name=dev \
        -engine cmd=/tmp/Greenseer_main/target/release/Greenseer name=main \
        -each proto=uci tc=1+0.01 \
        -sprt elo0=${ELO0} elo1=${ELO1} alpha=0.05 beta=0.05 \
        -rounds 200 \
        -config outname=benches/tournament_configs/${TIMESTAMP}_quick.json \
        -concurrency 2 \
        -log engine=false \
        -pgnout file=benches/diagnostics/pgn/vs_main_eigenmann_eg_puzzles/${TIMESTAMP}_quick.pgn notation=san nodes=true seldepth=true nps=true hashfull=true tbhits=true pv=true timeleft=true latency=true \
        -openings file=/Users/joshbradley/Desktop/Projects/Current/Greenseer/benches/test_suites/EigenmannEndgames.epd format=epd order=random -repeat \
        | grep --line-buffered -v "Warning" | tee /dev/tty | grep --line-buffered -v "Started game" > benches/diagnostics/vs_main_eigenmann_eg_puzzles/${TIMESTAMP}_quick.txt || true
fi
trap - INT

# analyze pgn output (avg depth / nps for dev vs main)
source python/.venv/bin/activate
python3 python/pgn_analysis.py "$TIMESTAMP" q
deactivate

# cleanup
rm -rf /tmp/Greenseer_main
