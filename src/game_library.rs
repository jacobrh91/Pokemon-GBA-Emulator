//! The per-user game library: installed ROMs, each with its own `saves/`
//! directory, plus the helpers for reading and writing save files.
//!
//! ```text
//! <root>/
//!   <My Pokemon ROM>/
//!     <My Pokemon ROM>.gba
//!     saves/
//!       <My Pokemon ROM>.sav
//! ```
//!
//! Installing never touches an existing `saves/` directory, so reinstalling or
//! replacing a ROM cannot lose progress.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the platform data directory.
pub const GAME_LIBRARY_ENV: &str = "GBA_EMULATOR_LIBRARY";
const SAVES_DIR: &str = "saves";
const ROM_EXTENSION: &str = "gba";
const MIN_ROM_BYTES: u64 = 0xC0; // cartridge header
const MAX_ROM_BYTES: u64 = 32 * 1024 * 1024;
/// Byte 0xB2 of every GBA cartridge header.
const HEADER_FIXED_VALUE: u8 = 0x96;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRom {
    pub name: String,
    pub directory: PathBuf,
    pub rom_path: PathBuf,
    pub saves_directory: PathBuf,
}

impl InstalledRom {
    fn at(root: &Path, name: &str) -> Self {
        let directory = root.join(name);
        Self {
            name: name.to_owned(),
            rom_path: directory.join(format!("{name}.{ROM_EXTENSION}")),
            saves_directory: directory.join(SAVES_DIR),
            directory,
        }
    }

    /// Where this game's battery save (Flash/SRAM/EEPROM image) lives.
    pub fn save_path(&self) -> PathBuf {
        self.saves_directory.join(format!("{}.sav", self.name))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed,
    /// The identical ROM was already present; nothing was changed.
    AlreadyInstalled,
    /// A different ROM with this name was replaced (`--force`); saves kept.
    Replaced,
}

pub struct GameLibrary {
    root: PathBuf,
}

impl GameLibrary {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The default game library: `$GBA_EMULATOR_LIBRARY` if set, otherwise the
    /// platform's per-user data directory.
    pub fn default_location() -> io::Result<Self> {
        if let Some(path) = std::env::var_os(GAME_LIBRARY_ENV).filter(|p| !p.is_empty()) {
            return Ok(Self::new(path));
        }
        let home = || {
            std::env::var_os("HOME")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| io::Error::other("HOME is not set; set GBA_EMULATOR_LIBRARY"))
        };
        let root = if cfg!(target_os = "macos") {
            home()?.join("Library/Application Support/gba_emulator")
        } else if cfg!(windows) {
            std::env::var_os("APPDATA")
                .map(|p| PathBuf::from(p).join("gba_emulator"))
                .ok_or_else(|| io::Error::other("APPDATA is not set; set GBA_EMULATOR_LIBRARY"))?
        } else {
            match std::env::var_os("XDG_DATA_HOME").filter(|p| !p.is_empty()) {
                Some(data) => PathBuf::from(data).join("gba_emulator"),
                None => home()?.join(".local/share/gba_emulator"),
            }
        };
        Ok(Self::new(root))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Copies `source` into the game library under its file name. Existing saves
    /// are never modified. Replacing a different ROM of the same name
    /// requires `force`.
    pub fn install(
        &self,
        source: &Path,
        force: bool,
    ) -> io::Result<(InstalledRom, InstallOutcome)> {
        let metadata = fs::metadata(source).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot read {}: {error}", source.display()),
            )
        })?;
        if !metadata.is_file() {
            return Err(invalid(format!("{} is not a file", source.display())));
        }
        if metadata.len() < MIN_ROM_BYTES || metadata.len() > MAX_ROM_BYTES {
            return Err(invalid(format!(
                "{} is {} bytes; a GBA ROM is between {MIN_ROM_BYTES} and {MAX_ROM_BYTES} bytes",
                source.display(),
                metadata.len()
            )));
        }
        let contents = fs::read(source)?;
        if contents[0xB2] != HEADER_FIXED_VALUE {
            return Err(invalid(format!(
                "{} does not look like a GBA ROM (bad header marker)",
                source.display()
            )));
        }

        let stem = source
            .file_stem()
            .and_then(OsStr::to_str)
            .ok_or_else(|| invalid("ROM file name is not valid UTF-8".into()))?;
        let name = sanitize_name(stem)
            .ok_or_else(|| invalid(format!("cannot derive a name from {stem:?}")))?;
        let installed = InstalledRom::at(&self.root, &name);

        let outcome = match fs::read(&installed.rom_path) {
            Ok(existing) if existing == contents => InstallOutcome::AlreadyInstalled,
            Ok(_) if !force => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "{name:?} is already installed with different contents; \
                         use --force to replace the ROM (saves are kept)"
                    ),
                ));
            }
            Ok(_) => InstallOutcome::Replaced,
            Err(error) if error.kind() == io::ErrorKind::NotFound => InstallOutcome::Installed,
            Err(error) => return Err(error),
        };

        fs::create_dir_all(&installed.saves_directory)?;
        if outcome != InstallOutcome::AlreadyInstalled {
            // Write beside the destination and rename so a crash cannot leave
            // a truncated ROM behind.
            let partial = installed
                .directory
                .join(format!("{name}.{ROM_EXTENSION}.partial"));
            fs::write(&partial, &contents)?;
            fs::rename(&partial, &installed.rom_path)?;
        }
        Ok((installed, outcome))
    }

    /// Installed ROMs, sorted by name.
    pub fn list(&self) -> io::Result<Vec<InstalledRom>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut roms = Vec::new();
        for entry in entries {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let installed = InstalledRom::at(&self.root, &name);
            if entry.file_type()?.is_dir() && installed.rom_path.is_file() {
                roms.push(installed);
            }
        }
        roms.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(roms)
    }

    /// Finds an installed ROM by exact name, falling back to a unique
    /// case-insensitive prefix match.
    pub fn find(&self, query: &str) -> io::Result<InstalledRom> {
        let roms = self.list()?;
        if let Some(exact) = roms.iter().find(|rom| rom.name == query) {
            return Ok(exact.clone());
        }
        let lowered = query.to_lowercase();
        let mut matches = roms
            .into_iter()
            .filter(|rom| rom.name.to_lowercase().starts_with(&lowered));
        match (matches.next(), matches.next()) {
            (Some(only), None) => Ok(only),
            (Some(_), Some(_)) => Err(invalid(format!(
                "{query:?} matches more than one installed ROM; be more specific"
            ))),
            (None, _) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no installed ROM named {query:?} (see `list`)"),
            )),
        }
    }
}

/// Where a ROM that is run straight from a file keeps its save: beside the
/// ROM, with a `.sav` extension.
pub fn sibling_save_path(rom: &Path) -> PathBuf {
    rom.with_extension("sav")
}

/// Reads a backup image, or `None` if there is no save yet.
pub fn read_save(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(image) => Ok(Some(image)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes a backup image through a temporary file so an interrupted write
/// can never replace a good save with a truncated one.
pub fn write_save(path: &Path, image: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    fs::write(&partial, image)?;
    fs::rename(&partial, path)
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Makes a file stem safe to use as a directory name on every platform.
fn sanitize_name(stem: &str) -> Option<String> {
    let cleaned: String = stem
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gba-library-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_rom(directory: &Path, file_name: &str, fill: u8) -> PathBuf {
        let mut rom = vec![fill; 0x200];
        rom[0xB2] = HEADER_FIXED_VALUE;
        let path = directory.join(file_name);
        fs::write(&path, rom).unwrap();
        path
    }

    #[test]
    fn install_creates_rom_and_saves_directories() {
        let work = scratch();
        let library = GameLibrary::new(work.join("library"));
        let source = write_rom(&work, "Test Game (v1.0).gba", 1);

        let (rom, outcome) = library.install(&source, false).unwrap();

        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(rom.name, "Test Game (v1.0)");
        assert_eq!(
            rom.rom_path,
            library.root().join("Test Game (v1.0)/Test Game (v1.0).gba")
        );
        assert!(rom.rom_path.is_file());
        assert!(rom.saves_directory.is_dir());
        assert_eq!(
            rom.save_path(),
            library
                .root()
                .join("Test Game (v1.0)/saves/Test Game (v1.0).sav")
        );
        assert_eq!(fs::read(&rom.rom_path).unwrap(), fs::read(&source).unwrap());
    }

    #[test]
    fn reinstalling_the_same_rom_is_a_no_op_that_keeps_saves() {
        let work = scratch();
        let library = GameLibrary::new(work.join("library"));
        let source = write_rom(&work, "Game.gba", 2);
        let (rom, _) = library.install(&source, false).unwrap();
        fs::write(rom.save_path(), b"progress").unwrap();

        let (_, outcome) = library.install(&source, false).unwrap();

        assert_eq!(outcome, InstallOutcome::AlreadyInstalled);
        assert_eq!(fs::read(rom.save_path()).unwrap(), b"progress");
    }

    #[test]
    fn replacing_a_different_rom_needs_force_and_keeps_saves() {
        let work = scratch();
        let library = GameLibrary::new(work.join("library"));
        let (rom, _) = library
            .install(&write_rom(&work, "Game.gba", 3), false)
            .unwrap();
        fs::write(rom.save_path(), b"progress").unwrap();

        let other = scratch();
        let patched = write_rom(&other, "Game.gba", 4);
        let error = library.install(&patched, false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&rom.rom_path).unwrap()[0], 3);

        let (_, outcome) = library.install(&patched, true).unwrap();
        assert_eq!(outcome, InstallOutcome::Replaced);
        assert_eq!(fs::read(&rom.rom_path).unwrap()[0], 4);
        assert_eq!(fs::read(rom.save_path()).unwrap(), b"progress");
        assert!(!rom.directory.join("Game.gba.partial").exists());
    }

    #[test]
    fn invalid_roms_are_rejected_without_creating_anything() {
        let work = scratch();
        let library = GameLibrary::new(work.join("library"));
        let tiny = work.join("tiny.gba");
        fs::write(&tiny, [0u8; 8]).unwrap();
        assert_eq!(
            library.install(&tiny, false).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );

        let bad_header = work.join("bad.gba");
        fs::write(&bad_header, vec![0u8; 0x200]).unwrap();
        assert!(library.install(&bad_header, false).is_err());

        assert!(library.install(&work.join("missing.gba"), false).is_err());
        assert!(!library.root().exists());
    }

    #[test]
    fn save_files_round_trip_atomically_and_missing_means_none() {
        let work = scratch();
        let path = work.join("saves/Game.sav");
        assert_eq!(read_save(&path).unwrap(), None);

        write_save(&path, &[1, 2, 3]).unwrap();
        assert_eq!(read_save(&path).unwrap(), Some(vec![1, 2, 3]));
        write_save(&path, &[9]).unwrap();
        assert_eq!(read_save(&path).unwrap(), Some(vec![9]));
        assert!(!work.join("saves/Game.sav.partial").exists());
    }

    #[test]
    fn sibling_save_sits_next_to_the_rom() {
        assert_eq!(
            sibling_save_path(Path::new("/roms/Game (v1.0).gba")),
            PathBuf::from("/roms/Game (v1.0).sav")
        );
    }

    #[test]
    fn unsafe_characters_in_names_are_replaced() {
        assert_eq!(sanitize_name("a/b:c").as_deref(), Some("a_b_c"));
        assert_eq!(sanitize_name("  ..hidden  ").as_deref(), Some("hidden"));
        assert_eq!(sanitize_name("..."), None);
    }

    #[test]
    fn list_and_find_use_exact_then_unique_prefix_matches() {
        let work = scratch();
        let library = GameLibrary::new(work.join("library"));
        assert!(library.list().unwrap().is_empty());
        for name in [
            "Pokemon Fire Red.gba",
            "Pokemon Leaf Green.gba",
            "Metroid.gba",
        ] {
            library.install(&write_rom(&work, name, 5), false).unwrap();
        }

        let names: Vec<_> = library
            .list()
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["Metroid", "Pokemon Fire Red", "Pokemon Leaf Green"]);

        assert_eq!(library.find("Metroid").unwrap().name, "Metroid");
        assert_eq!(library.find("pokemon f").unwrap().name, "Pokemon Fire Red");
        assert_eq!(
            library.find("pokemon").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            library.find("zelda").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
