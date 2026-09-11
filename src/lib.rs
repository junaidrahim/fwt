mod cli;
mod commands;
mod cone;
mod error;
mod git;
mod listing;
mod registry;
mod settings;

use clap::Parser;

use crate::{
    cli::{Cli, Command},
    error::Result,
    settings::Settings,
};

pub fn main_entry() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = if error.use_stderr() { 1 } else { 0 };
            let _ = error.print();
            return code;
        }
    };
    match run(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("fwt: {error}");
            error.exit_code()
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    if matches!(cli.command, Command::ShellInit) {
        print!("{}", include_str!("../shell/fwt.sh"));
        return Ok(());
    }
    let settings = Settings::from_env()?;
    match cli.command {
        Command::New(args) => commands::new(&settings, args),
        Command::Ls(args) => commands::list(&settings, args),
        Command::Cd(args) | Command::Resolve(args) => commands::cd(&settings, args),
        Command::Rm(args) => commands::remove(&settings, args),
        Command::Cone(args) => commands::cone(&settings, args.command),
        Command::Tune => commands::tune(&settings),
        Command::Skill(args) => commands::skill(&settings, args.command),
        Command::ShellInit => {
            unreachable!("shell initialization is handled before loading settings")
        }
    }
}
