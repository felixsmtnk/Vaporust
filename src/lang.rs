//! Language in which Vaporust asks Steam for texts (descriptions, news, reviews, item
//! names). While the interface is Russian only, Steam texts are Russian too. When interface
//! language selection appears, this will be the single switch point.
//!
//! Some pages are intentionally requested in English (l=english): their parsing relies on
//! Steam's English words (badges, licenses, discussions, reviews on the profile page) - their
//! language must not be changed.

/// Language of Steam texts (the l= and language parameters).
pub const STEAM: &str = "russian";
