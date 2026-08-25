use std::path::{Path, PathBuf};

pub fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed")
        .to_string()
}

pub fn sanitize(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let mut s = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    s.truncate(floor_char_boundary(&s, MAX_NAME));
    let s = s.trim();
    if s.is_empty() {
        "unnamed".to_string()
    } else {
        s.to_string()
    }
}

const MAX_NAME: usize = 120;

fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut i = max;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

pub fn clean_title(raw: &str) -> String {
    let mut s = raw.replace('_', " ");
    loop {
        s = s.trim_end().to_string();
        let Some(open) = s.rfind('(') else { break };
        let tail = &s[open..];
        let Some(close) = tail.find(')') else { break };
        let close = open + close;
        if close == s.len() - 1 {
            s.truncate(open);
        } else {
            break;
        }
    }
    sanitize(&s)
}

pub fn safe_join(dest: &Path, raw_name: &str) -> Option<PathBuf> {
    let normalized = raw_name.replace('\\', "/");
    let mut out = dest.to_path_buf();
    for comp in normalized.split('/') {
        match comp {
            "" | "." => continue,
            ".." => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_illegal_chars() {
        assert_eq!(sanitize("a/b\\c:d*e?f\"g<h>i|j"), "a b c d e f g h i j");
    }

    #[test]
    fn clean_title_strips_region_tags() {
        assert_eq!(
            clean_title("Sonic_the_Hedgehog_(USA,_Europe)"),
            "Sonic the Hedgehog"
        );
        assert_eq!(
            clean_title("Final Fantasy VII (USA) (Disc 1)"),
            "Final Fantasy VII"
        );
        assert_eq!(clean_title("Zelda"), "Zelda");
    }

    #[test]
    fn sanitize_truncates_on_a_character_boundary() {
        let raw = format!("{}\u{00e9}{}", "a".repeat(119), "b".repeat(40));
        let out = sanitize(&raw);
        assert!(out.len() <= MAX_NAME);
        assert_eq!(out, "a".repeat(119));

        let wide = "\u{65e5}".repeat(200);
        assert!(sanitize(&wide).len() <= MAX_NAME);
    }

    #[test]
    fn sanitize_of_only_illegal_characters_is_named() {
        assert_eq!(sanitize("///"), "unnamed");
        assert_eq!(sanitize("   "), "unnamed");
    }

    #[test]
    fn safe_join_blocks_traversal() {
        assert_eq!(safe_join(Path::new("/d"), "../evil"), None);
        assert_eq!(safe_join(Path::new("/d"), "a/b/../c.txt"), None);
        assert_eq!(
            safe_join(Path::new("/d"), "a/./c.txt"),
            Some(PathBuf::from("/d/a/c.txt"))
        );
    }
}
