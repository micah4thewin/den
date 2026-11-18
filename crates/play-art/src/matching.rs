//! Choosing the box art name that fits a game.
//!
//! The names on both sides are No-Intro and Redump style:
//! `Title (Region) (Other tags)`. A file on someone's disk is usually that
//! name, or that name with the tags cut off, the punctuation lost, or `The`
//! moved. Matching compares titles with all of that taken out, then uses the
//! tags to pick among the versions of the same game.

use crate::thumbnail_safe;
use std::collections::HashSet;

/// A title with everything that varies between copies of the same game
/// removed: tags, case, punctuation, and where `The` goes.
pub fn base_key(name: &str) -> String {
    let cut = name
        .find(" (")
        .into_iter()
        .chain(name.find(" ["))
        .chain(name.find('('))
        .min()
        .unwrap_or(name.len());
    let mut title = name[..cut].trim().to_lowercase().replace('&', " and ");
    for article in [", the", ", a", ", an"] {
        if let Some(pos) = title.find(article) {
            let after = &title[pos + article.len()..];
            if after.is_empty() || after.starts_with(' ') || after.starts_with(':') {
                let word = &article[2..];
                title = format!("{word} {}{}", &title[..pos], after);
                break;
            }
        }
    }
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let words: Vec<&str> = words
        .iter()
        .map(String::as_str)
        .skip_while(|w| *w == "the")
        .collect();
    words.concat()
}

fn tags(name: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for c in name.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                current.clear();
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                for part in current.split(',') {
                    let t = part.trim().to_lowercase();
                    if !t.is_empty() {
                        out.insert(t);
                    }
                }
                current.clear();
            }
            c if depth > 0 => current.push(c),
            _ => {}
        }
    }
    out
}

const UNWANTED: [&str; 9] = [
    "beta", "proto", "demo", "sample", "unl", "pirate", "hack", "kiosk", "debug",
];

fn score(candidate: &str, wanted_tags: &HashSet<String>) -> i32 {
    let mut s = 0;
    for t in tags(candidate) {
        if wanted_tags.contains(&t) {
            s += 10;
        } else if UNWANTED.iter().any(|u| t.starts_with(u)) {
            s -= 30;
        } else {
            s += match t.as_str() {
                "usa" => 6,
                "world" => 5,
                "europe" => 4,
                "japan" => 2,
                // A later revision of the same release looks the same.
                t if t.starts_with("rev ") || t.starts_with("disc 1") => -1,
                // Any other tag is a version nobody asked for.
                _ => -7,
            };
        }
    }
    s
}

/// The best of `available` for a game known by the names in `wanted`, or
/// nothing when no name in `available` is plainly the same game.
pub fn choose(wanted: &[&str], available: &[String]) -> Option<String> {
    let wanted: Vec<&str> = wanted
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .collect();
    // The exact name first, as the set spells it.
    for w in &wanted {
        let safe = thumbnail_safe(w);
        if let Some(hit) = available.iter().find(|a| a.eq_ignore_ascii_case(&safe)) {
            return Some(hit.clone());
        }
    }
    let keys: HashSet<String> = wanted
        .iter()
        .map(|w| base_key(w))
        .filter(|k| k.len() >= 2)
        .collect();
    if keys.is_empty() {
        return None;
    }
    let mut wanted_tags = HashSet::new();
    for w in &wanted {
        wanted_tags.extend(tags(w));
    }
    available
        .iter()
        .filter(|a| keys.contains(&base_key(a)))
        .map(|a| (score(a, &wanted_tags), a))
        // Among equals, the shorter name, then the first in order.
        .max_by(|(sa, a), (sb, b)| {
            sa.cmp(sb)
                .then_with(|| b.len().cmp(&a.len()))
                .then_with(|| b.cmp(a))
        })
        .map(|(_, a)| a.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn keys_ignore_tags_punctuation_and_the() {
        assert_eq!(base_key("Super Mario Bros. (World)"), "supermariobros");
        assert_eq!(base_key("Super_Mario_Bros (USA)"), "supermariobros");
        assert_eq!(
            base_key("Legend of Zelda, The - A Link to the Past (USA)"),
            base_key("The Legend of Zelda - A Link to the Past")
        );
        assert_eq!(base_key("Sonic & Knuckles (World)"), "sonicandknuckles");
    }

    #[test]
    fn the_region_asked_for_wins_and_then_usa() {
        let available = set(&[
            "Tetris (Japan)",
            "Tetris (World) (Rev 1)",
            "Tetris (Europe) (Beta)",
        ]);
        assert_eq!(
            choose(&["Tetris"], &available).as_deref(),
            Some("Tetris (World) (Rev 1)")
        );
        assert_eq!(
            choose(&["Tetris (Japan)"], &available).as_deref(),
            Some("Tetris (Japan)")
        );
    }

    #[test]
    fn a_different_game_is_not_a_match() {
        let available = set(&["Super Mario Bros. 3 (USA)"]);
        assert_eq!(choose(&["Super Mario Bros (USA)"], &available), None);
    }

    #[test]
    fn the_disc_asked_for_is_the_disc_given() {
        let available = set(&[
            "Final Fantasy VII (USA) (Disc 1)",
            "Final Fantasy VII (USA) (Disc 2)",
        ]);
        assert_eq!(
            choose(&["Final Fantasy VII (USA) (Disc 2)"], &available).as_deref(),
            Some("Final Fantasy VII (USA) (Disc 2)")
        );
        assert_eq!(
            choose(&["Final Fantasy VII"], &available).as_deref(),
            Some("Final Fantasy VII (USA) (Disc 1)")
        );
    }
}
