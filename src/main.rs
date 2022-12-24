use anyhow::Result;
use lib::{level::Level, render, search};
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Args {
    #[structopt(name = "world dir", parse(from_os_str))]
    world: PathBuf,

    #[structopt(name = "output dir", parse(from_os_str))]
    output: PathBuf,
}

#[paw::main]
fn main(Args { output, world }: Args) -> Result<()> {
    let generator = format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    let level = Level::from_world_path(&world)?;
    let map_ids = search(env!("CARGO_PKG_NAME"), &world, &output, false, false, None)?;
    render(&generator, &world, &output, false, false, &level, map_ids)
}
