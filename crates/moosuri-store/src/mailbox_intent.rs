//! Deliberate mailbox selection phrases. Sender/recipient filters are not selectors.
use super::*;
fn account_word(word: &str) -> bool {
    matches!(
        word,
        "account" | "계정" | "계정의" | "계정에서" | "계정으로" | "계정은" | "계정을" | "계정이"
    )
}
pub(crate) fn requested(text: &str) -> Result<Option<String>> {
    let words = text
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| {
                matches!(
                    c,
                    '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | ':' | '.' | '!' | '?'
                )
            })
            .to_lowercase()
        })
        .collect::<Vec<_>>();
    let mut target = None;
    for (i, word) in words.iter().enumerate() {
        if word.is_ascii()
            && word.len() <= 254
            && word.contains('@')
            && !word.contains(':')
            && (i > 0 && account_word(&words[i - 1])
                || words.get(i + 1).is_some_and(|next| account_word(next)))
        {
            if target.as_ref().is_some_and(|old| old != word) {
                return Err(StoreError::Unavailable);
            }
            target = Some(word.clone());
        }
    }
    Ok(target)
}
