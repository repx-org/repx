use clap::Parser;
use repx_tui::{run, TuiArgs};

fn main() {
    let args = TuiArgs::parse();
    if let Err(e) = run(args) {
        eprintln!("[ERROR] {}", e);
        std::process::exit(1);
    }
}
