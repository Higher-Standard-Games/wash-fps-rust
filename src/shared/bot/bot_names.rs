//! wash-fps `shared/bot/BotNames.ts`: the names bots play under, the bot
//! server's and the game server's own. The lobby reserves them, so a bot and
//! a player never share a name in a room, a scoreboard or a demo.
//!
//! When the list runs out a bot takes a name again with `_2`, `_3`, …; those
//! forms are reserved too. Udon and Soba are the team names, so neither is here.

pub const BOT_NAMES: &[&str] = &[
    "Ramen",
    "Washiki!",
    "Noodlesoup",
    "Tonkotsu",
    "Shoyu",
    "Miso",
    "Tempura",
    "Mochi",
    "Wasabi",
    "Onigiri",
    "Takoyaki",
    "Gyoza",
    "Narutomaki",
    "Chashu",
    "Menma",
    "Ajitama",
    "Kaedama",
    "Nori",
    "Katsudon",
    "Dango",
    "Edamame",
    "Furikake",
    "Umeboshi",
    "Natto",
    "Bento",
    "Karaage",
    "Tamago",
    "Taiyaki",
    "Oden",
    "Dashi",
    "Slurp-san",
    "Extra Spicy",
    "Noodle Ninja",
    "Chopsticks",
    "Kamaboko",
    "Broth Lord",
];

/// `isBotName`: whether `name` is a bot name, in any capitals, with or
/// without a `_<digits>` suffix.
#[must_use]
pub fn is_bot_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    // `replace(/_\d+$/, "")`.
    let stem = match lower.rfind('_') {
        Some(at) if at + 1 < lower.len() && lower[at + 1..].bytes().all(|b| b.is_ascii_digit()) => &lower[..at],
        _ => &lower,
    };
    BOT_NAMES.iter().any(|bot| bot.to_lowercase() == stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_names_are_reserved_in_any_capitals_and_numbered() {
        assert!(is_bot_name("ramen"));
        assert!(is_bot_name("BROTH LORD"));
        assert!(is_bot_name("Miso_12"));
        assert!(!is_bot_name("Miso_"));
        assert!(!is_bot_name("Misox"));
        assert!(!is_bot_name("Udon"));
    }
}
