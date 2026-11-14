//! Immutable public Korean text aliases v1. No identity or morphology expansion.
use super::normalize;
const FAMILIES: &[&[&str]] = &[
    &["휴대전화", "휴대폰", "핸드폰"],
    &["전자우편", "이메일"],
    &["비밀번호", "패스워드"],
];
fn family(token: &str) -> Option<&'static [&'static str]> {
    FAMILIES
        .iter()
        .copied()
        .find(|words| words.contains(&token))
}
pub(super) fn variants(raw: &str) -> Vec<String> {
    let normalized = normalize(raw);
    // This also preserves the established whitespace-insensitive whole query.
    if let Some(words) = family(&normalized) {
        return words.iter().map(|word| (*word).into()).collect();
    }
    let tokens: Vec<_> = raw.split_whitespace().collect();
    let matches: Vec<_> = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| family(token).map(|words| (index, words)))
        .collect();
    let [(index, words)] = matches.as_slice() else {
        return vec![normalized];
    };
    words
        .iter()
        .map(|word| {
            let mut substituted = tokens.clone();
            substituted[*index] = word;
            normalize(&substituted.join(" "))
        })
        .collect()
}
