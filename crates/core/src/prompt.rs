/// Turns a raw transcript into the prompt sent to an agent.
pub trait PromptTransformer: Send + Sync {
    fn transform(&self, text: &str) -> String;
}

/// Whitespace cleanup plus case-insensitive whole-word replacements.
#[derive(Debug, Default, Clone)]
pub struct Dictionary {
    entries: Vec<(Vec<String>, String)>,
}

impl Dictionary {
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut entries: Vec<_> = entries
            .into_iter()
            .map(|(from, to)| {
                let words = from.split_whitespace().map(str::to_lowercase).collect();
                (words, to)
            })
            .filter(|(words, _): &(Vec<String>, String)| !words.is_empty())
            .collect();
        entries.sort_by_key(|(words, _)| std::cmp::Reverse(words.len()));
        Self { entries }
    }

    /// Length of the phrase at the start of `words` and its replacement, keeping trailing punctuation.
    fn match_at(&self, words: &[&str]) -> Option<(usize, String)> {
        self.entries.iter().find_map(|(from, to)| {
            let n = from.len();
            if words.len() < n {
                return None;
            }
            let last = words[n - 1];
            let core = last.trim_end_matches(|c: char| c.is_ascii_punctuation());
            let matches = words[..n - 1]
                .iter()
                .zip(from)
                .all(|(w, f)| w.to_lowercase() == *f)
                && core.to_lowercase() == from[n - 1];
            matches.then(|| (n, format!("{to}{}", &last[core.len()..])))
        })
    }
}

impl PromptTransformer for Dictionary {
    fn transform(&self, text: &str) -> String {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut out = Vec::with_capacity(words.len());
        let mut i = 0;
        while i < words.len() {
            match self.match_at(&words[i..]) {
                Some((n, replacement)) => {
                    out.push(replacement);
                    i += n;
                }
                None => {
                    out.push(words[i].to_string());
                    i += 1;
                }
            }
        }
        out.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(pairs: &[(&str, &str)]) -> Dictionary {
        Dictionary::new(pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())))
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(
            Dictionary::default().transform("  проверь \n\t текущий   diff  "),
            "проверь текущий diff"
        );
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(Dictionary::default().transform(" \n "), "");
    }

    #[test]
    fn replaces_words_case_insensitively() {
        let d = dict(&[("пайтон", "Python"), ("клод", "Claude")]);
        assert_eq!(d.transform("Клод, обнови пайтон"), "Claude, обнови Python");
    }

    #[test]
    fn replaces_phrases_and_prefers_longest() {
        let d = dict(&[("гит", "git"), ("гит диф", "git diff")]);
        assert_eq!(
            d.transform("покажи гит диф и гит лог."),
            "покажи git diff и git лог."
        );
    }

    #[test]
    fn keeps_trailing_punctuation() {
        let d = dict(&[("раст", "Rust")]);
        assert_eq!(d.transform("перепиши на раст?!"), "перепиши на Rust?!");
    }

    #[test]
    fn does_not_replace_inside_words() {
        let d = dict(&[("раст", "Rust")]);
        assert_eq!(d.transform("растение"), "растение");
    }
}
