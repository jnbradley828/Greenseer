from dataclasses import dataclass
from pathlib import Path
import shutil
import threading
import click
import questionary
import subprocess
from datetime import datetime
import signal
import select
import sys
import time
from rich.live import Live
from rich.table import Table
from rich.console import Console, Group
import re

class InvalidResumeConfig(Exception):
    pass

BENCHES_DIR = Path(__file__).resolve().parent
REPO_ROOT = BENCHES_DIR.parent.parent

# output directories
RESULTS_DIR = BENCHES_DIR / "results"
TEST_SUITES_DIR = BENCHES_DIR / "test_suites" # not autoclearable at startup
CONFIGS_DIR = BENCHES_DIR / "tournament_configs" # not auto-clearable at startup due to accidental deletion wasting hours of compute
LOGS_DIR = BENCHES_DIR / "tournament_logs"
PGNS_DIR = BENCHES_DIR / "tournament_pgns"
STDERRS_DIR = BENCHES_DIR / "tournament_stderrs"
STDOUTS_DIR = BENCHES_DIR / "tournament_stdouts"

CLEARABLE_DIRS = [RESULTS_DIR, LOGS_DIR, PGNS_DIR, STDERRS_DIR, STDOUTS_DIR]

TOURNAMENT_CONFIGS = list(CONFIGS_DIR.rglob("*.json"))
TOURNAMENT_CONFIGS = [str(item.relative_to(CONFIGS_DIR)) for item in TOURNAMENT_CONFIGS]

OPENING_SUITES = list(TEST_SUITES_DIR.rglob("*.epd")) + list(TEST_SUITES_DIR.rglob("*.pgn"))
OPENING_SUITES = [str(item.relative_to(TEST_SUITES_DIR)) for item in OPENING_SUITES]

DEV_ENGINE_BINARY = REPO_ROOT / "target" / "release" / "Greenseer"
MAIN_ENGINE_BINARY = "/tmp/Greenseer_main/target/release/Greenseer"

@dataclass
class TournamentConfig:
    timestamp: str
    elo0: float
    elo1: float
    alpha_beta: float
    opening_suite: str
    concurrency: int
    time_ms: int
    increment_ms: int
    max_depth: int
    max_rounds: int

def prompt_new_config():
    print("Please enter the configuration options for new tournament below:\n")

    return TournamentConfig(
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S"),
        elo0 = click.prompt("Null hypothesis (elo0)", type=float),
        elo1 = click.prompt("Experimental hypothesis (elo1)", type=float),
        alpha_beta = click.prompt("False +/- rate (alpha/beta)", type=click.FloatRange(min=0, max=0.5, min_open=True, max_open=True)),
        opening_suite = TEST_SUITES_DIR / questionary.select("Opening suite", choices=OPENING_SUITES).ask(),
        concurrency = click.prompt("Games concurrency", type=int),
        time_ms = click.prompt("Base time per engine (ms) (enter 0 for no time control)", type=int),
        increment_ms = click.prompt("Increment time per engine (ms) (enter 0 for no time control)", type=int),
        max_depth = click.prompt("Max depth (enter 0 for no max depth)", type=int),
        max_rounds = click.prompt("Max rounds (games = rounds * 2) (enter 0 for max value - i.e. unbounded)", type=int)

    )

def build_engines():
    print("Building engine binaries...")
    subprocess.run(["cargo", "build", "--release"], cwd = REPO_ROOT, check=True, capture_output=True)

    subprocess.run(["rm", "-rf", "/tmp/Greenseer_main"], check=True, capture_output=True)
    subprocess.run(
        ["git", "clone", "--branch", "main", "--depth", "1",
        "git@github.com:jnbradley828/Greenseer.git", "/tmp/Greenseer_main"],
        check=True, capture_output=True,
    )
    subprocess.run(
        ["cargo", "build", "--release", "--manifest-path", "/tmp/Greenseer_main/Cargo.toml"],
        check=True, capture_output=True,
    )
    print("Engine binaries built successfully!")


def build_fastchess_args(config):
    if isinstance(config, TournamentConfig):
        each_args = ["-each", "proto=uci", "option.Ponder=false"]
        if (config.time_ms, config.increment_ms) != (0, 0):
            each_args.append(f"tc={config.time_ms / 1000}+{config.increment_ms / 1000}")
        if config.max_depth > 0:
            each_args.append(f"depth={config.max_depth}")

        return [
            "caffeinate", "-s", "-i", "-m",
            "fastchess",
            "-engine", f"cmd={DEV_ENGINE_BINARY}", "name=dev",
            "-engine", f"cmd={MAIN_ENGINE_BINARY}", "name=main",
            *each_args,
            "-sprt", f"elo0={config.elo0}", f"elo1={config.elo1}", f"alpha={config.alpha_beta}", f"beta={config.alpha_beta}",
            "-rounds", f"{config.max_rounds if config.max_rounds > 0 else 100000}",
            "-config", f"outname={CONFIGS_DIR}/{config.timestamp}.json",
            "-concurrency", f"{config.concurrency}",
            "-log", f"file={LOGS_DIR}/{config.timestamp}.log", "level=info", "engine=true",
            "-pgnout", f"file={PGNS_DIR}/{config.timestamp}.pgn", "notation=san", "nodes=true", "seldepth=true", "nps=true", "hashfull=true", "tbhits=true", "pv=true", "timeleft=true", "latency=true",
            "-openings", f"file={config.opening_suite}", f"format={Path(config.opening_suite).suffix.lstrip('.')}", "order=random", "-repeat",
            "-ratinginterval", "1",
        ]
    else:
        return [
            "caffeinate", "-s", "-i", "-m",
            "fastchess",
            "-config", f"file={config}", "stats=true"
        ]

def get_timestamp(config) -> str:
    return config.timestamp if isinstance(config, TournamentConfig) else Path(config).stem

def run_tournament(config):
    fastchess_args = build_fastchess_args(config)

    proc = subprocess.Popen(
        fastchess_args,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        cwd=REPO_ROOT,
    )

    progress = TournamentProgress()
    pgn_stats = PgnStats()
    status = Status(STATUS_RUNNING)
    live = Live(make_dashboard(progress, pgn_stats, status), refresh_per_second = 2)
    live.start()

    timestamp = get_timestamp(config)
    pgn_path = PGNS_DIR / (timestamp + ".pgn")
    threading.Thread(target=consume_stdout, args=(proc.stdout, STDOUTS_DIR / (timestamp + ".txt"), live, progress, pgn_stats, status)).start()
    threading.Thread(target=write_output, args=(proc.stderr, STDERRS_DIR / (timestamp + ".txt"))).start()
    threading.Thread(target=consume_pgn, args=(pgn_path, live, progress, pgn_stats, status, proc)).start()
    return proc, live, progress, pgn_stats, status

def write_output(pipe, output_path):
    with open(output_path, "a") as f:
        f.writelines(pipe)

def write_results(progress, pgn_stats, output_path):
    with open(output_path, "w") as f:
        console = Console(file=f)
        console.print(make_table(progress))
        console.print(make_pgn_table(pgn_stats))

def run_tournaments(configs: list):
    build_engines()
    for i, config in enumerate(configs):
        proc, live, progress, pgn_stats, status = run_tournament(config)

        paused = False
        while proc.poll() is None or paused:
            if select.select([sys.stdin], [], [], 0.1)[0]:
                key = sys.stdin.readline().strip()[:1]
                if not paused and key == "s":
                    proc.send_signal(signal.SIGINT)
                    proc.wait()
                elif not paused and key == "p":
                    proc.send_signal(signal.SIGINT)
                    proc.wait()
                    paused = True
                    status.text = STATUS_PAUSED
                    live.update(make_dashboard(progress, pgn_stats, status))
                elif paused and key == "s":
                    paused = False
                elif paused and key == "r":
                    config_prepend = CONFIGS_DIR / (get_timestamp(config) + ".json")
                    configs.insert(i + 1, config_prepend)
                    paused = False

        live.stop()
        write_results(progress, pgn_stats, RESULTS_DIR / (get_timestamp(config) + ".txt"))

@dataclass
class TournamentProgress:
    time_depth_control: str = None
    hash_size: str = None
    opening_suite: str = None
    estimated_elo_diff: str = None
    elo_estimation_error: str = None
    los: str = None
    draw_ratio: str = None
    pairs_ratio: str = None
    games: str = None
    wins: str = None
    losses: str = None
    draws: str = None
    points: str = None
    points_pct: str = None
    ptnmls: list = None
    llr: str = None
    llr_pct: str = None
    llr_h0: str = None
    llr_h1: str = None
    h0_elo: str = None
    h1_elo: str = None

    def reset(self):
        self.__dict__.update(TournamentProgress().__dict__)

STATUS_RUNNING = "Press 's' to stop and skip to next tournament, or 'p' to pause this tournament."
STATUS_PAUSED = "Tournament is currently paused. Press 's' to skip to next tournament, or 'r' to resume."

@dataclass
class Status:
    text: str

@dataclass
class PgnStats:
    total_dev_nodes: int = 0
    total_main_nodes: int = 0
    total_dev_time: float = 0.0
    total_main_time: float = 0.0
    total_dev_depths: int = 0
    total_main_depths: int = 0
    total_dev_moves: int = 0
    total_main_moves: int = 0

WHITE_MOVE_RE = re.compile(r'^\d+\.$') # "<any number of digits>."
BLACK_MOVE_START_RE = re.compile(r'^\d+\.{3}') # "<any number of digits>..."

def accumulate_pgn_move(stats: PgnStats, belongs_to_dev: bool, depth_field: str, time_field: str, nodes_field: str):
    depth = int(depth_field.rsplit('/', 1)[1])
    time_delta = float(time_field.replace('s', ''))
    nodes = int(nodes_field.rsplit('=', 1)[1])

    if belongs_to_dev:
        stats.total_dev_moves += 1
        stats.total_dev_nodes += nodes
        stats.total_dev_time += time_delta
        stats.total_dev_depths += depth
    else:
        stats.total_main_moves += 1
        stats.total_main_nodes += nodes
        stats.total_main_time += time_delta
        stats.total_main_depths += depth

def update_pgn_stats(line: str, dev_is_white: bool, stats: PgnStats) -> bool:
    stripped = line.strip()
    # any header line other than [White ...] is irrelevant here - and everything but
    # a movetext line starts with "[", so this also catches Event/Site/Result/FEN/etc.
    if stripped.startswith("[") and not stripped.startswith('[White '):
        return dev_is_white
    if stripped in ("", "1-0", "0-1", "1/2-1/2", "*"):
        return dev_is_white

    l = stripped.replace("[", "").replace("]", "").replace(",", "").replace('"', "")
    l_split = l.split(" ")

    if l_split[0] == "White":
        dev_is_white = (l_split[1] == "dev")
    elif WHITE_MOVE_RE.fullmatch(l_split[0]):
        accumulate_pgn_move(stats, dev_is_white, l_split[2], l_split[3], l_split[6])
    elif BLACK_MOVE_START_RE.fullmatch(l_split[0]):
        accumulate_pgn_move(stats, not dev_is_white, l_split[2], l_split[3], l_split[6])
    else:
        # bare black move (no restated move number) - the only thing left it could be.
        accumulate_pgn_move(stats, not dev_is_white, l_split[1], l_split[2], l_split[5])

    return dev_is_white

def make_table(progress: TournamentProgress):
    table = Table(title = "Tournament Progress")
    table.add_column("Field", style="bold")
    table.add_column("Value")

    for key, value in vars(progress).items():
        if value is not None:
            table.add_row(key, str(value))

    return table

def make_pgn_table(stats: PgnStats):
    table = Table(title = "Engine Metrics")
    table.add_column("Engine Version")
    table.add_column("Avg NPS", justify = "right")
    table.add_column("Avg Depth", justify = "right")
    table.add_column("Total Nodes", justify = "right")
    table.add_column("Total Time (s)", justify = "right")

    if stats.total_dev_moves == 0 or stats.total_main_moves == 0:
        return table

    dev_avg_nps = stats.total_dev_nodes / stats.total_dev_time
    main_avg_nps = stats.total_main_nodes / stats.total_main_time
    dev_avg_depth = stats.total_dev_depths / stats.total_dev_moves
    main_avg_depth = stats.total_main_depths / stats.total_main_moves

    table.add_row("dev", f"{dev_avg_nps:,.0f}", f"{dev_avg_depth:,.2f}", f"{stats.total_dev_nodes:,.0f}", f"{stats.total_dev_time:,.2f}")
    table.add_row("main", f"{main_avg_nps:,.0f}", f"{main_avg_depth:,.2f}", f"{stats.total_main_nodes:,.0f}", f"{stats.total_main_time:,.2f}")
    table.add_row(
        "abs_diff",
        f"{dev_avg_nps - main_avg_nps:+,.0f}",
        f"{dev_avg_depth - main_avg_depth:+,.2f}",
        f"{stats.total_dev_nodes - stats.total_main_nodes:+,.0f}",
        f"{stats.total_dev_time - stats.total_main_time:+,.2f}",
    )
    table.add_row(
        "pct_diff",
        f"{(dev_avg_nps - main_avg_nps) / main_avg_nps * 100:+,.2f}%",
        f"{(dev_avg_depth - main_avg_depth) / main_avg_depth * 100:+,.2f}%",
        f"{(stats.total_dev_nodes - stats.total_main_nodes) / stats.total_main_nodes * 100:+,.2f}%",
        f"{(stats.total_dev_time - stats.total_main_time) / stats.total_main_time * 100:+,.2f}%",
    )

    return table

def make_dashboard(progress: TournamentProgress, pgn_stats: PgnStats, status: Status):
    return Group(make_table(progress), make_pgn_table(pgn_stats), status.text)

def consume_stdout(pipe, output_path, live, progress, pgn_stats, status):
    reading_output = False

    with open(output_path, "a") as f:
        for line in pipe:
            f.write(line)

            if line.startswith("Results of"):
                progress.reset()
                reading_output = True
                tournament_info = re.search(r"\(.+\)", line).group().strip('(').strip(')').split(', ')
                progress.time_depth_control = tournament_info[0]
                progress.hash_size = tournament_info[2]
                progress.opening_suite = tournament_info[3]
            elif reading_output:
                match line:
                    case _ if line.startswith("Elo"):
                        line_s = line.split(" ")
                        progress.estimated_elo_diff = line_s[1]
                        progress.elo_estimation_error = line_s[3].strip(',')
                    case _ if line.startswith("LOS"):
                        line_s = line.split(" ")
                        progress.los = line_s[1]
                        progress.draw_ratio = line_s[4] + '%'
                        progress.pairs_ratio = line_s[7].strip('\n') + '%'
                    case _ if line.startswith("Games"):
                        line_s = line.split(" ")
                        progress.games = line_s[1]
                        progress.wins = line_s[3]
                        progress.losses = line_s[5]
                        progress.draws = line_s[7]
                        progress.points = line_s[9]
                        progress.points_pct = line_s[10].strip('(') + '%'
                    case _ if line.startswith("Ptnml"):
                        ptnmls = re.search(r"\[.+\]", line).group().strip('[').strip(']').split(", ")
                        progress.ptnmls = [int(v) for v in ptnmls]
                    case _ if line.startswith("LLR"):
                        line_s = line.split(" ")
                        progress.llr = line_s[1]
                        progress.llr_pct = line_s[2].strip('(').strip(')')
                        progress.llr_h0 = line_s[3].strip('(').strip(',')
                        progress.llr_h1 = line_s[4].strip(')')
                        progress.h0_elo = line_s[5].strip('[').strip(',')
                        progress.h1_elo = line_s[6].rstrip(']\n')

                        reading_output = False
                        live.update(make_dashboard(progress, pgn_stats, status))

def consume_pgn(pgn_path, live, progress, pgn_stats, status, proc):
    while not pgn_path.exists() and proc.poll() is None:
        time.sleep(0.5)

    if not pgn_path.exists():
        return

    dev_is_white = True
    with open(pgn_path, 'r') as f:
        while True:
            line = f.readline()
            if line:
                dev_is_white = update_pgn_stats(line, dev_is_white, pgn_stats)
                live.update(make_dashboard(progress, pgn_stats, status))
            elif proc.poll() is not None:
                break
            else:
                time.sleep(1)

def main():
    # clear old results if requested
    clear_results = click.confirm("Would you like to clear previous results before proceeding?")
    for dir in CLEARABLE_DIRS:
        if clear_results:
            shutil.rmtree(dir)
        dir.mkdir(parents=True, exist_ok=True)

    end = False
    configs = []
    while not end:
        resume = click.confirm("Would you like to resume a tournament?")
        if resume:
            config_name = questionary.select("Tournament config file", choices=TOURNAMENT_CONFIGS).ask()
            config_path = CONFIGS_DIR / config_name
            if config_path.exists():
                configs.append(str(config_path))
                print(f"Please change any config options directly in {config_name} before starting tournaments.")
        else:
            config = prompt_new_config()
            configs.append(config)

        end = not click.confirm("Would you like to append another tournament?")

    run_tournaments(configs)

    if click.confirm("Would you like to clear old tournament configs?"):
        shutil.rmtree(CONFIGS_DIR)
        CONFIGS_DIR.mkdir(parents=True, exist_ok=True)

if __name__ == "__main__":
    main()
