use anyhow::Result;
use little_a_map::{render, search};
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
    env_logger::init();

    let world = world.try_into()?;
    let map_ids = search(&world, &output, false, false, None)?;
    render(&world, &output, false, false, &map_ids)
}
