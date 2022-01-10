use clap::Parser;

#[derive(Parser)]
#[clap(about = "Turtle, the Ninja build system clone written in Rust")]
pub struct Arguments {
    #[clap(short, help = "Root build file")]
    pub file: Option<String>,
}
