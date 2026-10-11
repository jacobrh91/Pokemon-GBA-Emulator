//! Command-line parsing.

use std::path::PathBuf;

pub const USAGE: &str = "\
usage:
  gba_emulator ROM_PATH | INSTALLED_NAME     run a ROM file or an installed ROM
  gba_emulator install ROM_PATH [--force]    copy a ROM into the library
  gba_emulator list                          show installed ROMs

The library lives in your user data directory (override with GBA_EMULATOR_LIBRARY).
Each ROM gets its own directory containing the ROM and a `saves/` folder.";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(String),
    Install { rom: PathBuf, force: bool },
    List,
    Help,
}

pub fn parse_args(args: &[String]) -> Result<Command, String> {
    match args {
        [] => Ok(Command::Help),
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
        [target] => Ok(Command::Run(target.clone())),
        _ => Err("too many arguments".into()),
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
        assert_eq!(parse(&[]), Ok(Command::Help));
        assert_eq!(parse(&["game.gba"]), Ok(Command::Run("game.gba".into())));
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
