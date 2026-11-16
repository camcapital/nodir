use clap::Parser;

/// Browse and sync Piffs: directories with their own markdown content.
#[derive(Parser)]
#[command(name = "nodir", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
    println!(
        "nodir: Piff content lives in {}",
        nodir_core::CONTENT_FILE_NAME
    );
}
