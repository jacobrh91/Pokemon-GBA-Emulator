mod frontend;

const DEFAULT_ROM_PATH: &str = "rom/Pokemon Fire Red (v1.0).gba";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let rom_path = args.next().unwrap_or_else(|| DEFAULT_ROM_PATH.to_owned());

    if args.next().is_some() {
        return Err("usage: gba_emulator [ROM_PATH]".into());
    }

    frontend::run(&rom_path)
}
