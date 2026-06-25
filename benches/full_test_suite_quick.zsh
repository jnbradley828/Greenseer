FIRST=${1:-m}

run_endgames() {
    echo "Running endgame performance tournament."
    "$(dirname "$0")/vs_main_endgames_quick.zsh"
}

run_middlegames() {
    echo "Running middlegame performance tournament."
    "$(dirname "$0")/vs_main_middlegames_quick.zsh"
}

if [ "$FIRST" = "e" ]; then
    run_endgames
    run_middlegames
else
    run_middlegames
    run_endgames
fi
