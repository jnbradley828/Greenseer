# e/m: which tournament runs first. --bounds ELO0 ELO1: sprt indifference region, forwarded to
# both sub-tournaments, e.g. --bounds -5 0 for a non-regression test. defaults to 0 10 if omitted.
FIRST=m
ELO0=0
ELO1=10
while [[ $# -gt 0 ]]; do
    case "$1" in
        --bounds)
            ELO0="$2"
            ELO1="$3"
            shift 3
            ;;
        e | m)
            FIRST="$1"
            shift
            ;;
        *)
            shift
            ;;
    esac
done

run_endgames() {
    echo "Running quick endgame performance tournament."
    "$(dirname "$0")/vs_main_endgames_quick.zsh" --bounds "$ELO0" "$ELO1"
}

run_middlegames() {
    echo "Running quick middlegame performance tournament."
    "$(dirname "$0")/vs_main_middlegames_quick.zsh" --bounds "$ELO0" "$ELO1"
}

if [ "$FIRST" = "e" ]; then
    run_endgames
    run_middlegames
else
    run_middlegames
    run_endgames
fi
