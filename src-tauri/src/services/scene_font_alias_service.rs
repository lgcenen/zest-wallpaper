use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneSystemFontAlias {
    windows_key: &'static str,
    mac_family_candidates: &'static [&'static str],
}

const WINDOWS_SYSTEM_FONT_ALIASES: &[SceneSystemFontAlias] = &[
    SceneSystemFontAlias {
        windows_key: "arial",
        mac_family_candidates: &["Arial", "Helvetica Neue", "Helvetica"],
    },
    SceneSystemFontAlias {
        windows_key: "arialblack",
        mac_family_candidates: &[
            "Arial Black",
            "Arial Bold",
            "Helvetica Neue Bold",
            "Helvetica Neue",
            "Helvetica",
        ],
    },
    SceneSystemFontAlias {
        windows_key: "comicsans",
        mac_family_candidates: &["Comic Sans MS", "Comic Sans"],
    },
    SceneSystemFontAlias {
        windows_key: "consolas",
        mac_family_candidates: &["Consolas", "Menlo", "Monaco", "Courier New", "Courier"],
    },
    SceneSystemFontAlias {
        windows_key: "couriernew",
        mac_family_candidates: &["Courier New", "Courier"],
    },
    SceneSystemFontAlias {
        windows_key: "georgia",
        mac_family_candidates: &["Georgia", "Times New Roman", "Times", "Times Roman"],
    },
    SceneSystemFontAlias {
        windows_key: "impact",
        mac_family_candidates: &[
            "Impact",
            "Helvetica Neue Condensed Bold",
            "Helvetica Neue Condensed",
            "Helvetica Neue",
        ],
    },
    SceneSystemFontAlias {
        windows_key: "lucidaconsole",
        mac_family_candidates: &[
            "Lucida Console",
            "Monaco",
            "Menlo",
            "Courier New",
            "Courier",
        ],
    },
    SceneSystemFontAlias {
        windows_key: "microsoftsansserif",
        mac_family_candidates: &[
            "Microsoft Sans Serif",
            "Geneva",
            "Helvetica Neue",
            "Helvetica",
        ],
    },
    SceneSystemFontAlias {
        windows_key: "palatinolinotype",
        mac_family_candidates: &["Palatino Linotype", "Palatino", "Book Antiqua"],
    },
    SceneSystemFontAlias {
        windows_key: "segoeui",
        mac_family_candidates: &["Segoe UI", "Helvetica Neue", "Helvetica"],
    },
    SceneSystemFontAlias {
        windows_key: "tahoma",
        mac_family_candidates: &["Tahoma", "Verdana", "Geneva", "Helvetica Neue", "Helvetica"],
    },
    SceneSystemFontAlias {
        windows_key: "timesnewroman",
        mac_family_candidates: &["Times New Roman", "Times", "Times Roman"],
    },
    SceneSystemFontAlias {
        windows_key: "trebuchetms",
        mac_family_candidates: &["Trebuchet MS", "Helvetica Neue", "Helvetica"],
    },
    SceneSystemFontAlias {
        windows_key: "verdana",
        mac_family_candidates: &["Verdana", "Geneva", "Helvetica Neue", "Helvetica"],
    },
];

pub fn mac_family_candidates_for_system_font_reference(font_reference: &str) -> Vec<String> {
    let trimmed = font_reference.trim();
    let Some(system_font) = trimmed.strip_prefix("systemfont_") else {
        return Vec::new();
    };

    let normalized_key = normalized_system_font_key(system_font);
    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();
    let register = |candidate: String, ordered: &mut Vec<String>, seen: &mut BTreeSet<String>| {
        let candidate = candidate.trim().to_string();
        if !candidate.is_empty() && seen.insert(candidate.clone()) {
            ordered.push(candidate);
        }
    };

    register(trimmed.to_string(), &mut ordered, &mut seen);
    register(system_font.to_string(), &mut ordered, &mut seen);

    if let Some(alias) = WINDOWS_SYSTEM_FONT_ALIASES
        .iter()
        .find(|alias| alias.windows_key == normalized_key)
    {
        for family in alias.mac_family_candidates {
            register((*family).to_string(), &mut ordered, &mut seen);
        }
    }

    for variant in prettified_system_font_variants(system_font) {
        register(variant, &mut ordered, &mut seen);
    }

    ordered
}

fn normalized_system_font_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn prettified_system_font_variants(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let words = trimmed
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(title_case_font_word)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    if words.is_empty() {
        return Vec::new();
    }

    let joined = words.join(" ");
    if joined == trimmed {
        Vec::new()
    } else {
        vec![joined]
    }
}

fn title_case_font_word(word: &str) -> String {
    let mut characters = word.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };

    let mut titled = String::new();
    titled.push(first.to_ascii_uppercase());
    titled.push_str(characters.as_str().to_ascii_lowercase().as_str());
    titled
}

#[cfg(test)]
mod tests {
    use super::mac_family_candidates_for_system_font_reference;

    #[test]
    fn maps_comicsans_to_macos_family_names() {
        let candidates = mac_family_candidates_for_system_font_reference("systemfont_comicsans");

        assert_eq!(
            candidates.first().map(String::as_str),
            Some("systemfont_comicsans")
        );
        assert!(candidates.contains(&"Comic Sans MS".to_string()));
        assert!(candidates.contains(&"Comic Sans".to_string()));
    }

    #[test]
    fn maps_verdana_to_windows_and_macos_fallback_chain() {
        let candidates = mac_family_candidates_for_system_font_reference("systemfont_verdana");

        assert!(candidates.contains(&"Verdana".to_string()));
        assert!(candidates.contains(&"Geneva".to_string()));
        assert!(candidates.contains(&"Helvetica Neue".to_string()));
    }

    #[test]
    fn keeps_unknown_system_fonts_resolvable_as_generic_candidates() {
        let candidates = mac_family_candidates_for_system_font_reference("systemfont_future_font");

        assert!(candidates.contains(&"systemfont_future_font".to_string()));
        assert!(candidates.contains(&"future_font".to_string()));
        assert!(candidates.contains(&"Future Font".to_string()));
    }

    #[test]
    fn ignores_non_system_font_references() {
        assert!(mac_family_candidates_for_system_font_reference("Comic Sans MS").is_empty());
    }
}
