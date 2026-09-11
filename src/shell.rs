use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;

use crate::{
    cli::{InitArgs, Shell},
    error::{FwtError, Result},
};

const START: &str = "# >>> fwt shell integration >>>";
const END: &str = "# <<< fwt shell integration <<<";
const LOADER: &str = "eval \"$(git-fwt init --print)\"";

fn print() {
    print!("{}", include_str!("../shell/fwt.sh"));
}

pub fn init(args: &InitArgs) -> Result<()> {
    if args.print {
        print();
        return Ok(());
    }
    let shell = match args.shell {
        Some(shell) => shell,
        None => match env::var_os("SHELL")
            .as_deref()
            .and_then(|value| Path::new(value).file_name())
            .and_then(|value| value.to_str())
        {
            Some("bash") => Shell::Bash,
            Some("zsh") => Shell::Zsh,
            _ => {
                return Err(FwtError::Validation(
                    "cannot detect a supported shell from SHELL; use --shell bash or --shell zsh"
                        .to_owned(),
                ));
            }
        },
    };
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| FwtError::Validation("HOME is not set".to_owned()))?;
    let config = match shell {
        Shell::Bash => home.join(".bashrc"),
        Shell::Zsh => env::var_os("ZDOTDIR")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join(".zshrc"),
    };
    if !config.is_absolute() {
        return Err(FwtError::Validation(
            "shell config directory must be absolute; check HOME and ZDOTDIR".to_owned(),
        ));
    }
    let installed = append_integration(&config)?;
    println!(
        "{} {}",
        if installed {
            "Updated"
        } else {
            "Already configured:"
        },
        config.display()
    );
    println!("To enable fwt cd in this shell, run: eval \"$(git-fwt init --print)\"");
    println!("Future interactive shells that load this config will enable it automatically.");
    if matches!(shell, Shell::Bash) {
        println!("Bash login shells must source ~/.bashrc from their login profile.");
    }
    Ok(())
}

fn append_integration(config: &Path) -> Result<bool> {
    let context = || format!("configure {}", config.display());
    if let Some(parent) = config.parent() {
        fs::create_dir_all(parent).map_err(|error| FwtError::io(context(), error))?;
    }
    let mut options = OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // Append through the existing file, preserving permissions and dotfile symlinks.
    let mut file = options
        .open(config)
        .map_err(|error| FwtError::io(context(), error))?;
    FileExt::lock_exclusive(&file).map_err(|error| FwtError::io(context(), error))?;
    let mut existing = Vec::new();
    file.read_to_end(&mut existing)
        .map_err(|error| FwtError::io(context(), error))?;
    let lines: Vec<_> = existing.split(|byte| *byte == b'\n').collect();
    let has_start = lines.contains(&START.as_bytes());
    let has_end = lines.contains(&END.as_bytes());
    if has_start || has_end {
        if has_start && has_end && lines.contains(&LOADER.as_bytes()) {
            return Ok(false);
        }
        return Err(FwtError::Validation(format!(
            "incomplete or edited fwt integration in {}; review the marked block before retrying",
            config.display()
        )));
    }
    // Do not duplicate a loader the user has already added manually.
    if lines.iter().any(|line| {
        let line = String::from_utf8_lossy(line);
        matches!(
            line.trim(),
            "eval \"$(git-fwt init --print)\"" | "eval \"$(fwt init --print)\""
        )
    }) {
        return Ok(false);
    }
    let separator = if existing.is_empty() || existing.ends_with(b"\n") {
        ""
    } else {
        "\n"
    };
    let block = format!("{separator}\n{START}\n{LOADER}\n{END}\n");
    file.write_all(block.as_bytes())
        .map_err(|error| FwtError::io(context(), error))?;
    file.sync_all()
        .map_err(|error| FwtError::io(context(), error))?;
    Ok(true)
}
