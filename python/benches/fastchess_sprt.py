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
from rich.live import Live
from rich.table import Table
from rich.console import Group
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
    status = Status(STATUS_RUNNING)
    live = Live(make_dashboard(progress, status), refresh_per_second = 0.5)
    live.start()

    timestamp = get_timestamp(config)
    threading.Thread(target=consume_stdout, args=(proc.stdout, STDOUTS_DIR / (timestamp + ".txt"), live, progress, status)).start()
    threading.Thread(target=write_output, args=(proc.stderr, STDERRS_DIR / (timestamp + ".txt"))).start()
    return proc, live, progress, status

def write_output(pipe, output_path):
    with open(output_path, "a") as f:
        f.writelines(pipe)

def run_tournaments(configs: list):
    build_engines()
    for i, config in enumerate(configs):
        proc, live, progress, status = run_tournament(config)

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
                    live.update(make_dashboard(progress, status))
                elif paused and key == "s":
                    paused = False
                elif paused and key == "r":
                    config_prepend = CONFIGS_DIR / (get_timestamp(config) + ".json")
                    configs.insert(i + 1, config_prepend)
                    paused = False

        live.stop()

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
    llr_min_h0: str = None
    llr_max_h1: str = None
    h0_elo: str = None
    h1_elo: str = None

    def reset(self):
        self.__dict__.update(TournamentProgress().__dict__)

STATUS_RUNNING = "Press 's' to stop and skip to next tournament, or 'p' to pause this tournament."
STATUS_PAUSED = "Tournament is currently paused. Press 's' to skip to next tournament, or 'r' to resume."

@dataclass
class Status:
    text: str

def make_table(progress: TournamentProgress):
    table = Table(title = "Tournament Progress")
    table.add_column("Field", style="bold")
    table.add_column("Value")

    for key, value in vars(progress).items():
        if value is not None:
            table.add_row(key, str(value))

    return table

def make_dashboard(progress: TournamentProgress, status: Status):
    return Group(make_table(progress), status.text)

def consume_stdout(pipe, output_path, live, progress, status):
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
                        progress.llr_min_h0 = line_s[3].strip('(').strip(',')
                        progress.llr_max_h1 = line_s[4].strip(')')
                        progress.h0_elo = line_s[5].strip('[').strip(',')
                        progress.h1_elo = line_s[6].rstrip(']\n')

                        reading_output = False
                        live.update(make_dashboard(progress, status))




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
