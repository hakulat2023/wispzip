#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // When invoked by Finder Quick Actions (or a terminal) with a CLI
    // subcommand — e.g. `wispzip c file.txt` — run the CLI instead of
    // launching the GUI.  The Tauri GUI binary is the same binary that
    // the Automator workflow references via `__WISPZIP_BIN__`.
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 {
        match args[1].as_str() {
            "c" | "compress" | "x" | "extract" | "l" | "list" | "-h" | "--help" | "-V"
            | "--version" => {
                let code = wispzip_cli::run_cli(args);
                std::process::exit(code);
            }
            _ => {}
        }
    }

    wispzip_lib::run();
}
