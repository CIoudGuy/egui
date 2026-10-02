//! Which grapheme clusters are emoji, so text layout can give each one the
//! width of a single emoji.
//!
//! A fallback emoji font rarely joins every sequence: it may draw a family
//! (`👨‍👩‍👧‍👦`) as its four people and a keycap (`1️⃣`) as its digit. Laid out
//! glyph by glyph, the first takes four emoji widths and the second a
//! digit's, which leaves no room for an application that paints the colour
//! picture over the text, and puts cursors where nothing is drawn.

/// The emoji whose advance every emoji cluster takes: a single code point
/// every emoji font draws.
pub(crate) const REFERENCE: char = '\u{1F600}';

/// Whether a grapheme cluster is an emoji: one with default emoji
/// presentation, or a sequence that asks for it (U+FE0F, a keycap, a skin
/// tone, a joiner after an emoji, a flag).
pub(crate) fn is_emoji_cluster(cluster: &str) -> bool {
    let Some(first) = cluster.chars().next() else {
        return false;
    };
    if (first as u32) < 0xA9 {
        // Digits, `#` and `*` are emoji only in keycap sequences.
        return cluster.contains('\u{20E3}');
    }
    if is_presentation(first) {
        return true;
    }
    // Variation selectors, skin tones and joiners ask for emoji presentation
    // only after a character that has one. Writing systems that use joiners
    // (Devanagari, Arabic) stay text.
    let capable = matches!(first, '\u{A9}' | '\u{AE}')
        || ('\u{2000}'..='\u{33FF}').contains(&first)
        || ('\u{1F000}'..='\u{1FAFF}').contains(&first);
    capable
        && cluster.chars().any(|c| {
            matches!(c, '\u{FE0F}' | '\u{20E3}' | '\u{200D}')
                || ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
        })
}

/// Characters with default emoji presentation (Unicode's
/// `Emoji_Presentation`), approximated by block outside the BMP.
fn is_presentation(c: char) -> bool {
    let code = c as u32;
    if (0x1F000..=0x1FAFF).contains(&code) {
        // Enclosed letters that are text by default.
        return !matches!(
            code,
            0x1F170 | 0x1F171 | 0x1F17E | 0x1F17F | 0x1F202 | 0x1F237
        );
    }
    matches!(
        code,
        0x231A | 0x231B | 0x23E9..=0x23EC | 0x23F0 | 0x23F3 | 0x25FD | 0x25FE | 0x2614 | 0x2615
            | 0x2648..=0x2653 | 0x267F | 0x2693 | 0x26A1 | 0x26AA | 0x26AB | 0x26BD | 0x26BE
            | 0x26C4 | 0x26C5 | 0x26CE | 0x26D4 | 0x26EA | 0x26F2 | 0x26F3 | 0x26F5 | 0x26FA
            | 0x26FD | 0x2705 | 0x270A | 0x270B | 0x2728 | 0x274C | 0x274E | 0x2753..=0x2755
            | 0x2757 | 0x2795..=0x2797 | 0x27B0 | 0x27BF | 0x2B1B | 0x2B1C | 0x2B50 | 0x2B55
    )
}

#[cfg(test)]
mod tests {
    use super::is_emoji_cluster;

    #[test]
    fn emoji_clusters_are_recognised() {
        for emoji in [
            "😀",
            "👍🏽",
            "👨\u{200D}👩\u{200D}👧\u{200D}👦",
            "👩🏽\u{200D}💻",
            "🏳\u{FE0F}\u{200D}🌈",
            "🇮🇹",
            "1\u{FE0F}\u{20E3}",
            "#\u{FE0F}\u{20E3}",
            "❤\u{FE0F}",
            "\u{2122}\u{FE0F}",
            "🏴\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
        ] {
            assert!(is_emoji_cluster(emoji), "{emoji:?}");
        }
        for text in ["a", "1", "#", "❤", "\u{2122}", "क्\u{200D}", "é", ""] {
            assert!(!is_emoji_cluster(text), "{text:?}");
        }
    }
}
