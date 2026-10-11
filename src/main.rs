mod cli;
mod frontend;

use cli::{Command, USAGE, parse_args};
use gba_emulator::game_library::{
    GAME_LIBRARY_ENV, GameLibrary, InstallOutcome, sibling_save_path,
};
use std::path::{Path, PathBuf};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match parse_args(&args) {
        Ok(command) => command,
        Err(message) => return Err(format!("{message}\n\n{USAGE}").into()),
    };

    match command {
        Command::Help => println!("{USAGE}"),
        Command::List => {
            let games = GameLibrary::default_location()?;
            let roms = games.list()?;
            if roms.is_empty() {
                println!("no ROMs installed in {}", games.root().display());
            }
            for rom in roms {
                println!("{}", rom.name);
            }
        }
        Command::Install { rom, force } => {
            let games = GameLibrary::default_location()?;
            let (installed, outcome) = games.install(&rom, force)?;
            let verb = match outcome {
                InstallOutcome::Installed => "installed",
                InstallOutcome::AlreadyInstalled => "already installed",
                InstallOutcome::Replaced => "replaced (saves kept)",
            };
            println!("{verb}: {}", installed.name);
            println!("  rom:   {}", installed.rom_path.display());
            println!("  saves: {}", installed.saves_directory.display());
        }
        Command::Run(target) => {
            let (rom_path, save_path) = resolve_rom(&target)?;
            frontend::run(&rom_path.to_string_lossy(), &save_path)?;
        }
    }
    Ok(())
}

/// Returns `(rom, save file)`. A path to an existing file wins and keeps its
/// save beside it; otherwise the argument names an installed ROM, whose save
/// lives in its `saves/` directory.
fn resolve_rom(target: &str) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    if Path::new(target).is_file() {
        return Ok((PathBuf::from(target), sibling_save_path(Path::new(target))));
    }
    let games = GameLibrary::default_location()?;
    let installed = games.find(target).map_err(|error| {
        format!(
            "{error}\n(looked in {}; set {GAME_LIBRARY_ENV} to use another game library)",
            games.root().display()
        )
    })?;
    let save_path = installed.save_path();
    Ok((installed.rom_path, save_path))
}
