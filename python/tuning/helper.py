import numpy as np
from pathlib import Path
import tomllib
import tomli_w
import copy

ENGINE_DIR = Path(__file__).resolve().parent.parent.parent / "src" / "engine"

# perturbation size for each parameter, by its type in the toml.
STEP_SIZES = {int: 1, float: 0.01}

def load_eval_params(dir_path = ENGINE_DIR):
    eval_h_path = dir_path / "eval_heuristics.toml"
    return load_parameters(eval_h_path)

def load_search_params(dir_path = ENGINE_DIR):
    search_h_path = dir_path / "search_heuristics.toml"
    return load_parameters(search_h_path)

def load_parameters(file_path):
    with open(file_path, "rb") as f:
        data = tomllib.load(f)
        return data

def flatten_params(data) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    values = []
    steps = []
    mins = []
    maxes = []

    for param in data.values():
        try:
            val_length = len(param["value"])
            values.extend(param["value"])
        except TypeError:
            val_length = 1
            values.append(param["value"])
        steps.extend([param["step"]] * val_length)
        mins.extend([param["min"]] * val_length)
        maxes.extend([param["max"]] * val_length)

    return (
        np.array(values, dtype=object),
        np.array(steps, dtype=np.float64),
        np.array(mins, dtype=np.float64),
        np.array(maxes, dtype=np.float64),
    )

def perturb_one_parameter(params: np.ndarray, index: int):
    step = STEP_SIZES[type(params[index])]
    params_minus = params.copy()
    params_minus[index] -= step
    params_plus = params.copy()
    params_plus[index] += step
    return (params_minus, params_plus)

def unflatten_params(data, values):
    i = 0
    datac = copy.deepcopy(data)
    for k in datac:
        orig_vals = datac[k]["value"]
        if isinstance(orig_vals, list):
            for j in range(len(orig_vals)):
                datac[k]["value"][j] = _cast_like(datac[k]["value"][j], values[i])
                i += 1
        else:
            datac[k]["value"] = _cast_like(datac[k]["value"], values[i])
            i += 1
    return datac

def _cast_like(orig, val):
    return int(round(val)) if isinstance(orig, int) else float(val)

def save_params(data, path):
    with open(path, 'wb') as f:
        tomli_w.dump(data, f)

def main():
    # example usage to load, flatten, unflatten, and save parameters
    np.set_printoptions(suppress = True) # not necessary unless viewing the output
    ep = load_eval_params()
    flat_ep = flatten_params(ep)
    ep_copy = unflatten_params(ep, flat_ep[0])
    output_file = ENGINE_DIR / "DUMMY_PARAMS.toml"
    save_params(ep_copy, output_file)

if __name__ == "__main__":
    main()
