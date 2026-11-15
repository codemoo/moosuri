//! Deliberate mailbox selection phrases. Sender/recipient filters are not selectors.
use super::*;
fn account_word(word: &str) -> bool {
    matches!(
        word,
        "account" | "계정" | "계정의" | "계정에서" | "계정으로" | "계정은" | "계정을" | "계정이"
    )
}
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| {
                matches!(
                    c,
                    '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | ':' | '.' | '!' | '?'
                )
            })
            .to_lowercase()
        })
        .collect::<Vec<_>>()
}
fn selector(words: &[String], i: usize) -> bool {
    let word = &words[i];
    word.is_ascii()
        && word.len() <= 254
        && word.contains('@')
        && !word.contains(':')
        && (i > 0 && account_word(&words[i - 1])
            || words.get(i + 1).is_some_and(|next| account_word(next)))
}
/// Exclude only exact supported selector occurrences. A separate To occurrence
/// (even with the same address) remains a supplied value, never a missing field.
pub(crate) fn has_non_account_address(text: &str) -> Result<bool> {
    requested(text)?;
    let words = words(text);
    Ok(words
        .iter()
        .enumerate()
        .any(|(i, word)| word.contains('@') && !selector(&words, i)))
}
pub(crate) fn requested(text: &str) -> Result<Option<String>> {
    let words = words(text);
    let mut target = None;
    for (i, word) in words.iter().enumerate() {
        if selector(&words, i) {
            if target.as_ref().is_some_and(|old| old != word) {
                return Err(StoreError::Unavailable);
            }
            target = Some(word.clone());
        }
    }
    Ok(target)
}
