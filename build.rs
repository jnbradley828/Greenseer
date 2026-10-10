fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    let mut code = String::new();

    for (file_name, param_struct, var_name) in [
        (
            "src/engine/eval_heuristics.toml",
            "EvalParams",
            "EVAL_PARAMS",
        ),
        (
            "src/engine/search_heuristics.toml",
            "SearchParams",
            "SEARCH_PARAMS",
        ),
    ] {
        println!("cargo:rerun-if-changed={file_name}");
        let toml_text = std::fs::read_to_string(file_name).unwrap();
        let table: toml::Table = toml::from_str(&toml_text).unwrap();

        code += &format!("pub const {var_name}: {param_struct} = {param_struct} {{\n");
        for (key, entry) in &table {
            code += &format!("{key}: {},\n", entry["value"]);
        }
        code += "};\n";
    }

    std::fs::write(format!("{out}/params_const.rs"), code).unwrap();
}
