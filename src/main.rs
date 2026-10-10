mod audio_output;
mod draw;
mod frontend;
mod hud;
mod volume_dock;

use gba_emulator::library::{InstallOutcome, LIBRARY_ENV, Library, sibling_save_path};
use std::path::{Path, PathBuf};

/// Used when no argument is given and the file exists (development default).
const DEFAULT_ROM_PATH: &str = "rom/Pokemon Fire Red (v1.0).gba";

const USAGE: &str = "\
usage:
  gba_emulator [ROM_PATH | INSTALLED_NAME]   run a ROM file or an installed ROM
  gba_emulator install ROM_PATH [--force]    copy a ROM into the library
  gba_emulator list                          show installed ROMs

The library lives in your user data directory (override with GBA_EMULATOR_LIBRARY).
Each ROM gets its own directory containing the ROM and a `saves/` folder.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run(Option<String>),
    Install { rom: PathBuf, force: bool },
    List,
    Help,
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    match args {
        [] => Ok(Command::Run(None)),
        [flag] if flag == "-h" || flag == "--help" || flag == "help" => Ok(Command::Help),
        [command] if command == "list" => Ok(Command::List),
        [command, rest @ ..] if command == "install" => {
            let mut rom = None;
            let mut force = false;
            for arg in rest {
                match arg.as_str() {
                    "--force" | "-f" => force = true,
                    flag if flag.starts_with('-') => {
                        return Err(format!("unknown option {flag:?}"));
                    }
                    path if rom.is_none() => rom = Some(PathBuf::from(path)),
                    _ => return Err("install takes exactly one ROM path".into()),
                }
            }
            rom.map(|rom| Command::Install { rom, force })
                .ok_or_else(|| "install needs a ROM path".into())
        }
        [target] => Ok(Command::Run(Some(target.clone()))),
        _ => Err("too many arguments".into()),
    }
}

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
            let library = Library::default_location()?;
            let roms = library.list()?;
            if roms.is_empty() {
                println!("no ROMs installed in {}", library.root().display());
            }
            for rom in roms {
                println!("{}", rom.name);
            }
        }
        Command::Install { rom, force } => {
            let library = Library::default_location()?;
            let (installed, outcome) = library.install(&rom, force)?;
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
            let (rom_path, save_path) = resolve_rom(target.as_deref())?;
            frontend::run(&rom_path.to_string_lossy(), &save_path)?;
        }
    }
    Ok(())
}

/// Returns `(rom, save file)`. A path to an existing file wins and keeps its
/// save beside it; otherwise the argument names an installed ROM, whose save
/// lives in its `saves/` directory.
fn resolve_rom(target: Option<&str>) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let beside = |rom: &str| (PathBuf::from(rom), sibling_save_path(Path::new(rom)));
    match target {
        None if Path::new(DEFAULT_ROM_PATH).is_file() => Ok(beside(DEFAULT_ROM_PATH)),
        None => Err(format!("no ROM given and {DEFAULT_ROM_PATH} was not found\n\n{USAGE}").into()),
        Some(target) if Path::new(target).is_file() => Ok(beside(target)),
        Some(target) => {
            let library = Library::default_location()?;
            let installed = library.find(target).map_err(|error| {
                format!(
                    "{error}\n(looked in {}; set {LIBRARY_ENV} to use another library)",
                    library.root().display()
                )
            })?;
            let save_path = installed.save_path();
            Ok((installed.rom_path, save_path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_run_install_list_and_help() {
        assert_eq!(parse(&[]), Ok(Command::Run(None)));
        assert_eq!(
            parse(&["game.gba"]),
            Ok(Command::Run(Some("game.gba".into())))
        );
        assert_eq!(parse(&["list"]), Ok(Command::List));
        assert_eq!(parse(&["--help"]), Ok(Command::Help));
        assert_eq!(
            parse(&["install", "a.gba"]),
            Ok(Command::Install {
                rom: "a.gba".into(),
                force: false
            })
        );
        assert_eq!(
            parse(&["install", "--force", "a.gba"]),
            Ok(Command::Install {
                rom: "a.gba".into(),
                force: true
            })
        );
    }

    #[test]
    fn rejects_malformed_install_commands() {
        assert!(parse(&["install"]).is_err());
        assert!(parse(&["install", "a.gba", "b.gba"]).is_err());
        assert!(parse(&["install", "--bogus", "a.gba"]).is_err());
        assert!(parse(&["a.gba", "b.gba"]).is_err());
    }
}
