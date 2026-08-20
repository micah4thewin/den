//! Small path and naming helpers shared across the intake pipeline.

use std::path::{Path, PathBuf};

/// The lower-cased extension of a path, without the dot ("" if none).
pub fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The file stem (name without extension) of a path.
pub fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed")
        .to_string()
}

/// Remove characters that are not legal in a file or folder name.
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
    s.truncate(120);
    if s.is_empty() {
        "unnamed".to_string()
    } else {
        s.trim().to_string()
    }
}

/// A display-friendly game title from a raw filename: underscores to spaces,
/// trailing `(region)` tags removed, illegal characters stripped.
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

/// Join a raw archive entry name onto a destination, refusing path traversal.
/// Any `..` component rejects the entry outright (the same posture as the zip
/// crate's `enclosed_name`).
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
    fn safe_join_blocks_traversal() {
        assert_eq!(safe_join(Path::new("/d"), "../evil"), None);
        assert_eq!(safe_join(Path::new("/d"), "a/b/../c.txt"), None);
        assert_eq!(
            safe_join(Path::new("/d"), "a/./c.txt"),
            Some(PathBuf::from("/d/a/c.txt"))
        );
    }
}
