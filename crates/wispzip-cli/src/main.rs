fn main() {
    let code = wispzip_cli::run_cli(std::env::args().collect());
    std::process::exit(code);
}
