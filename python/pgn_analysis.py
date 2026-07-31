import sys
from pathlib import Path
import re
from rich.table import Table
from rich.console import Console

def main():
    pgn_timestamp = sys.argv[1]
    quick = len(sys.argv) > 2 and sys.argv[2] == 'q'
    benches_dir = Path(__file__).resolve().parent.parent / "benches"
    matches = list(benches_dir.rglob(f"*{pgn_timestamp}*.pgn"))

    if len(matches) == 0:
        raise FileNotFoundError(f"No PGN matching timestamp {pgn_timestamp!r}")
    elif len(matches) > 1:
        raise ValueError(f"Multiple PGNs matching timestamp {pgn_timestamp!r}")

    pgn_path = matches[0]

    with open(pgn_path, 'r') as f:
        contents = f.read()
        contents = contents.splitlines()

        # True denotes dev plays white, false as black.
        dev = True
        total_dev_nodes = 0
        total_main_nodes = 0
        total_dev_time = 0
        total_main_time = 0
        total_dev_depths = 0
        total_main_depths = 0
        total_dev_moves = 0
        total_main_moves = 0

        white_move = re.compile(r'^\d+\.$') # checks for string of format "<any number of digits>."
        black_move_start = re.compile(r'^\d+\.{3}') # checks for string of format "<any number of digits>..."
        black_move = re.compile(r'^[a-h][1-8][a-h][1-8]$') # checks for string of UCI move format "<from_file><from_rank><to_file><to_rank>"

        for line in contents:
            l = line.replace("[", "")
            l = l.replace("]", "")
            l = l.replace(",", "")
            l = l.replace('"', "")
            l_split = l.split(" ")

            if l_split == "":
                continue
            elif l_split[0] == "White":
                if l_split[1] == "dev":
                    dev = True
                else:
                    dev = False
            elif bool(white_move.fullmatch(l_split[0])):
                depth = int(l_split[2].rsplit('/', 1)[1])
                time_delta = float(l_split[3].replace('s', ''))
                nodes = int(l_split[6].rsplit('=', 1)[1])

                if dev:
                    total_dev_moves += 1
                    total_dev_nodes += nodes
                    total_dev_time += time_delta
                    total_dev_depths += depth
                else:
                    total_main_moves += 1
                    total_main_nodes += nodes
                    total_main_time += time_delta
                    total_main_depths += depth

            elif bool(black_move_start.fullmatch(l_split[0])):
                depth = int(l_split[2].rsplit('/', 1)[1])
                time_delta = float(l_split[3].replace('s', ''))
                nodes = int(l_split[6].rsplit('=', 1)[1])

                if not dev:
                    total_dev_moves += 1
                    total_dev_nodes += nodes
                    total_dev_time += time_delta
                    total_dev_depths += depth
                else:
                    total_main_moves += 1
                    total_main_nodes += nodes
                    total_main_time += time_delta
                    total_main_depths += depth

            elif bool(black_move.fullmatch(l_split[0])) :
                depth = int(l_split[1].rsplit('/', 1)[1])
                time_delta = float(l_split[2].replace('s', ''))
                nodes = int(l_split[5].rsplit('=', 1)[1])

                if not dev:
                    total_dev_moves += 1
                    total_dev_nodes += nodes
                    total_dev_time += time_delta
                    total_dev_depths += depth
                else:
                    total_main_moves += 1
                    total_main_nodes += nodes
                    total_main_time += time_delta
                    total_main_depths += depth

        dev_avg_nps = total_dev_nodes / total_dev_time
        main_avg_nps = total_main_nodes / total_main_time
        dev_avg_depth = total_dev_depths / total_dev_moves
        main_avg_depth = total_main_depths / total_main_moves

        table = Table()
        table.add_column("Engine Version")
        table.add_column("Avg NPS", justify = "right")
        table.add_column("Avg Depth", justify = "right")

        table.add_row("dev", f"{dev_avg_nps:,.0f}", f"{dev_avg_depth:,.2f}")
        table.add_row("main", f"{main_avg_nps:,.0f}", f"{main_avg_depth:,.2f}")
        table.add_row("abs_diff", f"{dev_avg_nps - main_avg_nps:+,.0f}", f"{dev_avg_depth - main_avg_depth:+,.2f}")
        table.add_row(
            "pct_diff",
            f"{(dev_avg_nps - main_avg_nps) / main_avg_nps * 100:+,.2f}%",
            f"{(dev_avg_depth - main_avg_depth) / main_avg_depth * 100:+,.2f}%",
        )

        output_path = str(pgn_path.parent).replace("pgn/", "")
        suffix = "_quick_analysis.txt" if quick else "_analysis.txt"
        output_file = f"{output_path}/{pgn_timestamp}{suffix}"
        with open(output_file, "w") as out:
            console = Console(file=out)
            console.print(table)

        console1 = Console()
        console1.print(table)


if __name__ == "__main__":
    main()
