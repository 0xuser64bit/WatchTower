//! A curated, compiled-in directory of well-known Solana mints.
//!
//! WatchTower is Solana-only and a mint address is immutable, so the addresses of the
//! tokens people actually want to watch are a fixed, reviewable fact rather than
//! something each user should have to look up and paste. This module holds that list
//! so the common case — "alert me when SOL drops below $100" — is reachable by tapping.
//!
//! Two properties make this safe to trust:
//!
//! * **The list is data, not a lookup.** Nothing here is fetched at runtime, so no
//!   third party can rename an entry or point a familiar symbol at a different mint.
//!   Changing an address requires a reviewed commit.
//! * **It is a shortcut, not a bypass.** A catalog pick enters the same add-token
//!   flow a pasted address does: the mint is still verified against the price provider
//!   and still confirmed by the user before anything is stored. The catalog only
//!   supplies the address and a display symbol.
//!
//! Callback data is capped at 64 bytes, so buttons carry a [`Entry`] index rather than
//! a mint. Indices are therefore part of the wire format between two taps: entries are
//! only ever appended or corrected in place, never reordered, and every index arriving
//! from a button is bounds-checked (see [`entry`]) instead of trusted.

/// A section of the catalog. Groups exist to keep any one screen short enough to read
/// on a phone without paging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// SOL itself and the dollar stablecoins.
    Core,
    /// Liquid staking tokens, which track SOL rather than moving independently.
    Staking,
    /// Solana-native DeFi and infrastructure governance tokens.
    Defi,
    /// Memecoins. The most volatile section, and the most requested.
    Meme,
    /// Assets bridged onto Solana from another chain, plus DePIN.
    Bridged,
}

impl Group {
    /// Short code carried in callback data.
    pub fn code(self) -> &'static str {
        match self {
            Group::Core => "c",
            Group::Staking => "s",
            Group::Defi => "d",
            Group::Meme => "m",
            Group::Bridged => "b",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "c" => Some(Group::Core),
            "s" => Some(Group::Staking),
            "d" => Some(Group::Defi),
            "m" => Some(Group::Meme),
            "b" => Some(Group::Bridged),
            _ => None,
        }
    }

    /// Button label for the group index screen.
    pub fn label(self) -> &'static str {
        match self {
            Group::Core => "💵 SOL & stablecoins",
            Group::Staking => "🥩 Liquid staking",
            Group::Defi => "⚙️ DeFi & infra",
            Group::Meme => "🐕 Memecoins",
            Group::Bridged => "🌉 Bridged & DePIN",
        }
    }

    /// Display order on the group index screen.
    pub fn all() -> [Group; 5] {
        [
            Group::Core,
            Group::Staking,
            Group::Defi,
            Group::Meme,
            Group::Bridged,
        ]
    }
}

/// One catalog entry: a mint plus how to present it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Stored as the token's symbol, so alerts read `BONK` rather than an address.
    pub symbol: &'static str,
    /// Longer name, shown only while browsing.
    pub name: &'static str,
    pub mint: &'static str,
    pub group: Group,
}

/// The catalog.
///
/// Every mint was read from mainnet as an initialised SPL mint account and confirmed
/// to have a USD listing on CoinGecko when added. Append new entries at the end of
/// their group; do not reorder existing ones.
pub const ENTRIES: &[Entry] = &[
    // ── SOL & stablecoins ─────────────────────────────────────────────────────────
    Entry {
        symbol: "SOL",
        name: "Solana (wrapped SOL mint)",
        mint: "So11111111111111111111111111111111111111112",
        group: Group::Core,
    },
    Entry {
        symbol: "USDC",
        name: "USD Coin",
        mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        group: Group::Core,
    },
    Entry {
        symbol: "USDT",
        name: "Tether USD",
        mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
        group: Group::Core,
    },
    Entry {
        symbol: "PYUSD",
        name: "PayPal USD",
        mint: "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo",
        group: Group::Core,
    },
    Entry {
        symbol: "USDS",
        name: "Sky Dollar",
        mint: "USDSwr9ApdHk5bvJKMjzff41FfuX8bSxdKcR81vTwcA",
        group: Group::Core,
    },
    // ── Liquid staking ────────────────────────────────────────────────────────────
    Entry {
        symbol: "mSOL",
        name: "Marinade staked SOL",
        mint: "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
        group: Group::Staking,
    },
    Entry {
        symbol: "jitoSOL",
        name: "Jito staked SOL",
        mint: "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn",
        group: Group::Staking,
    },
    Entry {
        symbol: "bSOL",
        name: "BlazeStake staked SOL",
        mint: "bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1",
        group: Group::Staking,
    },
    Entry {
        symbol: "jupSOL",
        name: "Jupiter staked SOL",
        mint: "jupSoLaHXQiZZTSfEWMTRRgpnyFm8f6sZdosWBjx93v",
        group: Group::Staking,
    },
    // ── DeFi & infra ──────────────────────────────────────────────────────────────
    Entry {
        symbol: "JUP",
        name: "Jupiter",
        mint: "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN",
        group: Group::Defi,
    },
    Entry {
        symbol: "JTO",
        name: "Jito",
        mint: "jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL",
        group: Group::Defi,
    },
    Entry {
        symbol: "RAY",
        name: "Raydium",
        mint: "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R",
        group: Group::Defi,
    },
    Entry {
        symbol: "ORCA",
        name: "Orca",
        mint: "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE",
        group: Group::Defi,
    },
    Entry {
        symbol: "PYTH",
        name: "Pyth Network",
        mint: "HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3",
        group: Group::Defi,
    },
    Entry {
        symbol: "JLP",
        name: "Jupiter Perps LP",
        mint: "27G8MtK7VtTcCHkpASjSDdkWWYfoqT6ggEuKidVJidD4",
        group: Group::Defi,
    },
    Entry {
        symbol: "DRIFT",
        name: "Drift Protocol",
        mint: "DriFtupJYLTosbwoN8koMbEYSx54aFAVLddWsbksjwg7",
        group: Group::Defi,
    },
    Entry {
        symbol: "KMNO",
        name: "Kamino Finance",
        mint: "KMNo3nJsBXfcpJTVhZcXLW7RmTwTt4GVFE7suUBo9sS",
        group: Group::Defi,
    },
    Entry {
        symbol: "W",
        name: "Wormhole",
        mint: "85VBFQZC9TZkfaptBWjvUw7YbZjy52A6mjtPGjstQAmQ",
        group: Group::Defi,
    },
    // ── Memecoins ─────────────────────────────────────────────────────────────────
    Entry {
        symbol: "BONK",
        name: "Bonk",
        mint: "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",
        group: Group::Meme,
    },
    Entry {
        symbol: "WIF",
        name: "dogwifhat",
        mint: "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm",
        group: Group::Meme,
    },
    Entry {
        symbol: "TRUMP",
        name: "OFFICIAL TRUMP",
        mint: "6p6xgHyF7AeE6TZkSmFsko444wqoP15icUSqi2jfGiPN",
        group: Group::Meme,
    },
    Entry {
        symbol: "FARTCOIN",
        name: "Fartcoin",
        mint: "9BB6NFEcjBCtnNLFko2FqVQBq8HHM13kCyYcdQbgpump",
        group: Group::Meme,
    },
    Entry {
        symbol: "POPCAT",
        name: "Popcat",
        mint: "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr",
        group: Group::Meme,
    },
    Entry {
        symbol: "MEW",
        name: "cat in a dogs world",
        mint: "MEW1gQWJ3nEXg2qgERiKu7FAFj79PHvQVREQUzScPP5",
        group: Group::Meme,
    },
    Entry {
        symbol: "PENGU",
        name: "Pudgy Penguins",
        mint: "2zMMhcVQEXDtdE6vsFS7S7D5oUodfJHE8vd1gnBouauv",
        group: Group::Meme,
    },
    Entry {
        symbol: "PUMP",
        name: "Pump.fun",
        mint: "pumpCmXqMfrsAkQ5r49WcJnRayYRqmXz6ae8H7H9Dfn",
        group: Group::Meme,
    },
    // ── Bridged & DePIN ───────────────────────────────────────────────────────────
    Entry {
        symbol: "cbBTC",
        name: "Coinbase Wrapped BTC",
        mint: "cbbtcf3aa214zXHbiAZQwf4122FBYbraNdFqgw4iMij",
        group: Group::Bridged,
    },
    Entry {
        symbol: "WBTC",
        name: "Wrapped BTC (Wormhole)",
        mint: "3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh",
        group: Group::Bridged,
    },
    Entry {
        symbol: "ETH",
        name: "Ether (Wormhole)",
        mint: "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs",
        group: Group::Bridged,
    },
    Entry {
        symbol: "RENDER",
        name: "Render",
        mint: "rndrizKT3MK1iimdxRdWabcF7Zg7AR5T4nud4EkHBof",
        group: Group::Bridged,
    },
    Entry {
        symbol: "HNT",
        name: "Helium",
        mint: "hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux",
        group: Group::Bridged,
    },
    Entry {
        symbol: "GRASS",
        name: "Grass",
        mint: "Grass7B4RdKfBCjTKgSqnXkqjwiGvQyFbuSCUJr3XXjs",
        group: Group::Bridged,
    },
];

/// Resolves an index from a button back to its entry.
///
/// Returns `None` for anything out of range, so a stale or hand-crafted callback
/// cannot be used to reach past the end of the catalog.
pub fn entry(index: usize) -> Option<&'static Entry> {
    ENTRIES.get(index)
}

/// The entries of one group, paired with their catalog index for button data.
pub fn in_group(group: Group) -> Vec<(usize, &'static Entry)> {
    ENTRIES
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.group == group)
        .collect()
}

/// The catalog's symbol for a mint, if it holds one.
///
/// Lets a pasted address still be labelled with the reviewed symbol rather than left
/// unnamed, so `SOL` and `So11…112` cannot end up as two differently-named things.
pub fn symbol_for(mint: &str) -> Option<&'static str> {
    ENTRIES
        .iter()
        .find(|entry| entry.mint == mint)
        .map(|entry| entry.symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_mint_is_a_well_formed_solana_address() {
        // A typo here would ship a permanently unusable button, and the price provider
        // rejection would look like an outage rather than a bad address.
        for entry in ENTRIES {
            assert!(
                crate::providers::solana::is_valid_address(entry.mint),
                "{} has an invalid mint: {}",
                entry.symbol,
                entry.mint
            );
        }
    }

    #[test]
    fn mints_and_symbols_are_unique() {
        // A duplicate mint would offer the same token twice, and the second pick would
        // dead-end on "already tracking". A duplicate symbol would make two different
        // tokens indistinguishable in an alert.
        let mut mints = HashSet::new();
        let mut symbols = HashSet::new();
        for entry in ENTRIES {
            assert!(mints.insert(entry.mint), "duplicate mint {}", entry.mint);
            assert!(
                symbols.insert(entry.symbol.to_ascii_uppercase()),
                "duplicate symbol {}",
                entry.symbol
            );
        }
    }

    #[test]
    fn symbols_fit_the_length_the_add_flow_accepts() {
        // The typed-name step caps symbols at 32 characters; a catalog entry must not
        // be able to store something the manual path would reject.
        for entry in ENTRIES {
            assert!(
                (1..=32).contains(&entry.symbol.chars().count()),
                "{} has an unusable symbol length",
                entry.symbol
            );
            assert!(!entry.name.is_empty(), "{} has no name", entry.symbol);
        }
    }

    #[test]
    fn every_group_has_entries_and_none_are_orphaned() {
        let mut counted = 0;
        for group in Group::all() {
            let entries = in_group(group);
            assert!(
                !entries.is_empty(),
                "{group:?} would render an empty screen"
            );
            counted += entries.len();
        }
        // Guards against adding a Group variant and forgetting it in `all()`, which
        // would silently hide its tokens from the UI.
        assert_eq!(counted, ENTRIES.len(), "some entries are unreachable");
    }

    #[test]
    fn group_codes_round_trip_and_are_distinct() {
        let mut codes = HashSet::new();
        for group in Group::all() {
            assert_eq!(Group::parse(group.code()), Some(group), "{group:?}");
            assert!(
                codes.insert(group.code()),
                "duplicate code {}",
                group.code()
            );
        }
        assert_eq!(Group::parse("zz"), None);
        assert_eq!(Group::parse(""), None);
    }

    #[test]
    fn indices_are_bounds_checked() {
        assert_eq!(entry(0).map(|e| e.symbol), Some("SOL"));
        assert!(entry(ENTRIES.len()).is_none());
        assert!(entry(usize::MAX).is_none());
    }

    #[test]
    fn a_group_screen_stays_short_enough_to_read_on_a_phone() {
        // Rendered two per row plus navigation; more than this needs paging rather
        // than a taller keyboard.
        for group in Group::all() {
            let rows = in_group(group).len().div_ceil(2);
            assert!(rows <= 6, "{group:?} needs {rows} rows of buttons");
        }
    }

    #[test]
    fn a_known_mint_resolves_to_its_reviewed_symbol() {
        assert_eq!(
            symbol_for("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
            Some("USDC")
        );
        assert_eq!(symbol_for("not a mint"), None);
    }

    #[test]
    fn button_data_fits_the_telegram_callback_limit() {
        // Indices rather than mints are carried precisely because of this cap; assert
        // it rather than assume it.
        for index in 0..ENTRIES.len() {
            assert!(format!("at:p:{index}").len() <= 64);
        }
        for group in Group::all() {
            assert!(format!("at:g:{}", group.code()).len() <= 64);
        }
    }
}
