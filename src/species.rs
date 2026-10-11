//! Species names for the original 251 Pokémon. In the Game Boy Advance
//! games these are numbered by national dex number (Bulbasaur is 1, Dratini
//! is 147, Celebi is 251).

/// `NAMES[n - 1]` is the name of species `n`.
const NAMES: [&str; 251] = [
    "Bulbasaur",
    "Ivysaur",
    "Venusaur",
    "Charmander",
    "Charmeleon",
    "Charizard",
    "Squirtle",
    "Wartortle",
    "Blastoise",
    "Caterpie",
    "Metapod",
    "Butterfree",
    "Weedle",
    "Kakuna",
    "Beedrill",
    "Pidgey",
    "Pidgeotto",
    "Pidgeot",
    "Rattata",
    "Raticate",
    "Spearow",
    "Fearow",
    "Ekans",
    "Arbok",
    "Pikachu",
    "Raichu",
    "Sandshrew",
    "Sandslash",
    "Nidoran-F",
    "Nidorina",
    "Nidoqueen",
    "Nidoran-M",
    "Nidorino",
    "Nidoking",
    "Clefairy",
    "Clefable",
    "Vulpix",
    "Ninetales",
    "Jigglypuff",
    "Wigglytuff",
    "Zubat",
    "Golbat",
    "Oddish",
    "Gloom",
    "Vileplume",
    "Paras",
    "Parasect",
    "Venonat",
    "Venomoth",
    "Diglett",
    "Dugtrio",
    "Meowth",
    "Persian",
    "Psyduck",
    "Golduck",
    "Mankey",
    "Primeape",
    "Growlithe",
    "Arcanine",
    "Poliwag",
    "Poliwhirl",
    "Poliwrath",
    "Abra",
    "Kadabra",
    "Alakazam",
    "Machop",
    "Machoke",
    "Machamp",
    "Bellsprout",
    "Weepinbell",
    "Victreebel",
    "Tentacool",
    "Tentacruel",
    "Geodude",
    "Graveler",
    "Golem",
    "Ponyta",
    "Rapidash",
    "Slowpoke",
    "Slowbro",
    "Magnemite",
    "Magneton",
    "Farfetch'd",
    "Doduo",
    "Dodrio",
    "Seel",
    "Dewgong",
    "Grimer",
    "Muk",
    "Shellder",
    "Cloyster",
    "Gastly",
    "Haunter",
    "Gengar",
    "Onix",
    "Drowzee",
    "Hypno",
    "Krabby",
    "Kingler",
    "Voltorb",
    "Electrode",
    "Exeggcute",
    "Exeggutor",
    "Cubone",
    "Marowak",
    "Hitmonlee",
    "Hitmonchan",
    "Lickitung",
    "Koffing",
    "Weezing",
    "Rhyhorn",
    "Rhydon",
    "Chansey",
    "Tangela",
    "Kangaskhan",
    "Horsea",
    "Seadra",
    "Goldeen",
    "Seaking",
    "Staryu",
    "Starmie",
    "Mr. Mime",
    "Scyther",
    "Jynx",
    "Electabuzz",
    "Magmar",
    "Pinsir",
    "Tauros",
    "Magikarp",
    "Gyarados",
    "Lapras",
    "Ditto",
    "Eevee",
    "Vaporeon",
    "Jolteon",
    "Flareon",
    "Porygon",
    "Omanyte",
    "Omastar",
    "Kabuto",
    "Kabutops",
    "Aerodactyl",
    "Snorlax",
    "Articuno",
    "Zapdos",
    "Moltres",
    "Dratini",
    "Dragonair",
    "Dragonite",
    "Mewtwo",
    "Mew",
    "Chikorita",
    "Bayleef",
    "Meganium",
    "Cyndaquil",
    "Quilava",
    "Typhlosion",
    "Totodile",
    "Croconaw",
    "Feraligatr",
    "Sentret",
    "Furret",
    "Hoothoot",
    "Noctowl",
    "Ledyba",
    "Ledian",
    "Spinarak",
    "Ariados",
    "Crobat",
    "Chinchou",
    "Lanturn",
    "Pichu",
    "Cleffa",
    "Igglybuff",
    "Togepi",
    "Togetic",
    "Natu",
    "Xatu",
    "Mareep",
    "Flaaffy",
    "Ampharos",
    "Bellossom",
    "Marill",
    "Azumarill",
    "Sudowoodo",
    "Politoed",
    "Hoppip",
    "Skiploom",
    "Jumpluff",
    "Aipom",
    "Sunkern",
    "Sunflora",
    "Yanma",
    "Wooper",
    "Quagsire",
    "Espeon",
    "Umbreon",
    "Murkrow",
    "Slowking",
    "Misdreavus",
    "Unown",
    "Wobbuffet",
    "Girafarig",
    "Pineco",
    "Forretress",
    "Dunsparce",
    "Gligar",
    "Steelix",
    "Snubbull",
    "Granbull",
    "Qwilfish",
    "Scizor",
    "Shuckle",
    "Heracross",
    "Sneasel",
    "Teddiursa",
    "Ursaring",
    "Slugma",
    "Magcargo",
    "Swinub",
    "Piloswine",
    "Corsola",
    "Remoraid",
    "Octillery",
    "Delibird",
    "Mantine",
    "Skarmory",
    "Houndour",
    "Houndoom",
    "Kingdra",
    "Phanpy",
    "Donphan",
    "Porygon2",
    "Stantler",
    "Smeargle",
    "Tyrogue",
    "Hitmontop",
    "Smoochum",
    "Elekid",
    "Magby",
    "Miltank",
    "Blissey",
    "Raikou",
    "Entei",
    "Suicune",
    "Larvitar",
    "Pupitar",
    "Tyranitar",
    "Lugia",
    "Ho-Oh",
    "Celebi",
];

/// Highest species number understood.
pub const LAST_SPECIES: u16 = 251;

/// The name of species `number`, if it is one of the first 251.
pub fn name(number: u16) -> Option<&'static str> {
    NAMES.get(usize::from(number).checked_sub(1)?).copied()
}

/// Reads a species written as a number (`147`, `0x93`) or a name (`Dratini`).
/// Names ignore case, spaces and punctuation, so `mr mime` and `Mr. Mime`
/// both work.
pub fn parse(text: &str) -> Option<u16> {
    let text = text.trim();
    let number = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).ok()
    } else {
        text.parse::<u16>().ok()
    };
    if let Some(number) = number {
        return (1..=LAST_SPECIES).contains(&number).then_some(number);
    }
    let key = normalize(text);
    if key.is_empty() {
        return None;
    }
    NAMES
        .iter()
        .position(|name| normalize(name) == key)
        .map(|index| index as u16 + 1)
}

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_species_have_their_national_dex_numbers() {
        for (number, expected) in [
            (1, "Bulbasaur"),
            (19, "Rattata"),
            (25, "Pikachu"),
            (56, "Mankey"),
            (147, "Dratini"),
            (151, "Mew"),
            (152, "Chikorita"),
            (249, "Lugia"),
            (251, "Celebi"),
        ] {
            assert_eq!(name(number), Some(expected));
            assert_eq!(parse(expected), Some(number));
        }
        assert_eq!(name(0), None);
        assert_eq!(name(252), None);
    }

    #[test]
    fn numbers_and_awkward_names_parse() {
        assert_eq!(parse("147"), Some(147));
        assert_eq!(parse("0x93"), Some(147));
        assert_eq!(parse("  dratini "), Some(147));
        assert_eq!(parse("MR. MIME"), Some(122));
        assert_eq!(parse("mr mime"), Some(122));
        assert_eq!(parse("Farfetch'd"), Some(83));
        assert_eq!(parse("farfetchd"), Some(83));
        assert_eq!(parse("Ho-Oh"), Some(250));
        assert_eq!(parse("nidoran-f"), Some(29));
        assert_eq!(parse("Porygon2"), Some(233));
    }

    #[test]
    fn unknown_or_out_of_range_species_are_rejected() {
        for text in [
            "",
            "0",
            "252",
            "0x0",
            "0xFFF",
            "Missingno",
            "Nidoran",
            "Dratin",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn names_are_unique_after_normalising() {
        let mut keys: Vec<String> = NAMES.iter().map(|n| normalize(n)).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 251);
    }
}
