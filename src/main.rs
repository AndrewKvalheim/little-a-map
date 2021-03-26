use anyhow::Result;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Args {
    #[structopt(name = "world dir", parse(from_os_str))]
    world_path: PathBuf,

    #[structopt(name = "output dir", parse(from_os_str))]
    output_path: PathBuf,
}

#[paw::main]
fn main(args: Args) -> Result<()> {
    let world_path = args.world_path;
    let output_path = args.output_path;

    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");

    lib::run(&name, &version, &world_path, &output_path, false, false)
}
