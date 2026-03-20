cd "$(dirname "$0")"

read "iter_name?name of iteration: greenseer-"
read "max_depth?maximum depth: "
rm -f ../results/${iter_name}_vs_main.pgn
rm -f ../results/${iter_name}_vs_main.txt

for depth in {1..$max_depth}; do
    printf "depth = $depth\n\n" >> ../results/${iter_name}_vs_main.pgn
    printf "depth = $depth\n\n" >> ../results/${iter_name}_vs_main.txt
    fastchess \
    -engine name=greenseer-$iter_name cmd=../../target/release/Greenseer \
    -engine name=greenseer-main cmd=../../../Greenseer_main/target/release/Greenseer \
    -each proto=uci depth=$depth \
    -concurrency 4 \
    -rounds 50 \
    -sprt elo0=0 elo1=10 alpha=0.05 beta=0.05 \
    -pgnout file=../results/${iter_name}_vs_main.pgn append=true \
    | grep -Ev "Warning|Started|Finished" | tee -a ../results/${iter_name}_vs_main.txt
done
