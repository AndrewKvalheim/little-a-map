use anyhow::Result;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Args {
    #[structopt(name = "level dir", parse(from_os_str))]
    level_path: PathBuf,

    #[structopt(name = "output dir", parse(from_os_str))]
    output_path: PathBuf,
}

#[paw::main]
fn main(args: Args) -> Result<()> {
    let level_path = args.level_path;
    let output_path = args.output_path;

    let generator = format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    lib::run(&generator, &level_path, &output_path, false)
}
