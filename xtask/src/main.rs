use clap::Parser;

fn main() -> anyhow::Result<()> {
    xtask::run(xtask::Cli::parse())
}
