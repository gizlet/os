fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    std::process::exit(gizlet_launcher::cli::main(&args));
}
