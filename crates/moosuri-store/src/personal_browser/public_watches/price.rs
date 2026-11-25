//! Strict extraction of the one displayed KRW room/item price from bounded normalized visible text.
//!
//! A price counts only right after a clear scoped cue (`객실 요금 125,000원`, `1박 요금: 99,000원`,
//! `판매가 7만원`). Anything else is refused instead of guessed: no cue, two different prices, a cue that
//! is not followed by a clean amount (deposit/shipping only, ranges, fractions, other currencies), and
//! "from/starting/discount/original price" style wording around the cue.

/// Reasons stored on a `failed` check. None of them touches the last band state.
pub(super) const MISSING: &str = "price_missing";
pub(super) const AMBIGUOUS: &str = "price_ambiguous";
pub(super) const UNCLEAR: &str = "price_unclear";

pub(super) const MAX_WON: i64 = 999_999_999_999;

const CUES: &[&str] = &[
    "객실 요금",
    "객실 가격",
    "1박 요금",
    "1박 가격",
    "판매가",
    "판매 가격",
    "상품 가격",
    "객실요금",
    "숙박 요금",
];
/// Words in front of a cue (same clause) that make it something other than the current price.
const BEFORE: &[&str] = &[
    "보증금",
    "예약금",
    "배송",
    "택배",
    "최저",
    "최소",
    "최대",
    "할인",
    "쿠폰",
    "정상",
    "정가",
    "원가",
    "기존",
    "원래",
    "세금",
    "봉사료",
    "from",
    "starting",
    "as low as",
    "up to",
    "추가",
    "이전",
    "지난",
    "전일",
    "어제",
    "종료",
    "previous",
    "expired",
    "regular",
];
/// What may not follow the amount: ranges, "from", discount or comparison wording.
const AFTER: &[&str] = &[
    "부터", "~", "〜", "∼", "-", "–", "—", "이상", "이하", "미만", "대", "→", "에서", "할인",
    "적립", "상당", "off",
];

/// `true` when the condition is met by `won`.
pub(super) fn qualifies(won: i64, limit: i64, inclusive: bool) -> bool {
    if inclusive { won <= limit } else { won < limit }
}

/// `125000` -> `125,000원`.
pub(super) fn display(won: i64) -> String {
    let digits = won.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.push('원');
    out
}

/// The single unambiguous displayed price, or the reason it cannot be trusted.
pub(super) fn parse(text: &str) -> Result<i64, &'static str> {
    let lower = text.to_lowercase();
    if [
        "품절",
        "매진",
        "만실",
        "예약 마감",
        "예약 불가",
        "판매 종료",
        "판매 중지",
        "sold out",
        "unavailable",
    ]
    .iter()
    .any(|label| lower.contains(label))
    {
        return Err(UNCLEAR);
    }
    let mut found = None;
    let mut price_units = Vec::new();
    for &cue in CUES {
        for (at, _) in text.match_indices(cue) {
            let before = &text[..at];
            let after = &text[at + cue.len()..];
            // The cue must start a word and must not just be the head of a longer word (판매가능).
            let after = after
                .strip_prefix(['은', '는', '이', '가'])
                .unwrap_or(after);
            if before
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
                || after.starts_with(char::is_alphabetic)
            {
                continue;
            }
            if disqualified_before(before) {
                return Err(UNCLEAR);
            }
            let (won, rest) = amount(after).ok_or(UNCLEAR)?;
            price_units.push(text.len() - rest.len() - '원'.len_utf8());
            let fraction = rest
                .strip_prefix('.')
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()));
            let rest = rest.trim_start().to_lowercase();
            if fraction
                || rest.starts_with(|c: char| c.is_ascii_digit())
                || AFTER.iter().any(|w| rest.starts_with(w))
            {
                return Err(UNCLEAR);
            }
            if found.replace(won).is_some_and(|previous| previous != won) {
                return Err(AMBIGUOUS);
            }
        }
    }
    let won = found.ok_or(MISSING)?;
    for currency in [
        "₩", "$", "€", "¥", "krw", "usd", "eur", "jpy", "won", "달러", "유로", "엔",
    ] {
        for (at, _) in lower.match_indices(currency) {
            let digit = |c: char| c.is_ascii_digit();
            if lower[..at].trim_end().ends_with(digit)
                || lower[at + currency.len()..].trim_start().starts_with(digit)
            {
                return Err(AMBIGUOUS);
            }
        }
    }
    // A second unlabeled room or product amount is as ambiguous as two labeled prices.
    // Separately identified charges (a deposit, for example) are not room or product prices.
    for (at, _) in text.match_indices('원') {
        let before = text[..at].trim_end();
        if price_units.contains(&at)
            || !before.ends_with(|c: char| c.is_ascii_digit() || matches!(c, '만' | '천'))
        {
            continue;
        }
        let label = before.trim_end_matches(|c: char| {
            c.is_ascii_digit()
                || c.is_whitespace()
                || matches!(c, ',' | '.' | '천' | '만' | ':' | '：')
        });
        if !["보증금", "예약금", "배송비", "주차비"]
            .iter()
            .any(|charge| label.ends_with(charge))
        {
            return Err(AMBIGUOUS);
        }
    }
    Ok(won)
}

fn disqualified_before(before: &str) -> bool {
    let clause = before
        .rsplit(['.', '|', ';', '•', '\n'])
        .next()
        .unwrap_or(before);
    // A previous amount starts a new price context; the word '원래' (originally) does not.
    let clause = clause
        .rmatch_indices('원')
        .find(|(i, _)| {
            clause[..*i]
                .trim_end()
                .ends_with(|c: char| c.is_ascii_digit() || c == '만')
        })
        .map_or(clause, |(i, _)| &clause[i + '원'.len_utf8()..])
        .to_lowercase();
    BEFORE.iter().any(|w| clause.contains(w))
}

/// `: 125,000원` / ` 99000원` / `7만원` -> (won, text after the unit). Nothing else is an amount.
fn amount(after: &str) -> Option<(i64, &str)> {
    let s = after.trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '：');
    let end = s
        .find(|c: char| !c.is_ascii_digit() && c != ',')
        .unwrap_or(s.len());
    let (num, rest) = s.split_at(end);
    let rest = rest.trim_start();
    let digits: String = num.chars().filter(|c| *c != ',').collect();
    if digits.is_empty() || digits.starts_with('0') || !grouping_ok(num) {
        return None;
    }
    let value: i64 = digits.parse().ok()?;
    let (value, rest) = if let Some(rest) = rest.strip_prefix('만') {
        if num.contains(',') {
            return None;
        }
        (
            value.checked_mul(10_000)?,
            rest.trim_start().strip_prefix('원')?,
        )
    } else {
        (value, rest.strip_prefix('원')?)
    };
    (1..=MAX_WON).contains(&value).then_some((value, rest))
}

/// Either plain digits or well-formed thousands groups (`1,250,000`).
fn grouping_ok(num: &str) -> bool {
    if !num.contains(',') {
        return !num.is_empty();
    }
    let mut groups = num.split(',');
    groups.next().is_some_and(|g| (1..=3).contains(&g.len())) && groups.all(|g| g.len() == 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_scoped_cues_are_parsed() {
        for (text, won) in [
            ("디럭스 객실 요금 125,000원 조식 포함", 125_000),
            ("1박 요금: 99,000원", 99_000),
            ("객실요금은 99,000 원입니다. 보증금: 20,000원", 99_000),
            ("상품 가격은 7만 원입니다.", 70_000),
            ("판매가 7만원", 70_000),
            ("현재 판매가 1,250,000원", 1_250_000),
            ("객실 가격 80000원 / 1박", 80_000),
            // The same price repeated is still one price.
            ("객실 요금 90,000원 ... 객실 요금 90,000원", 90_000),
            // A deposit mentioned earlier in another clause does not poison the cue.
            ("보증금 50,000원 판매가 70,000원", 70_000),
        ] {
            assert_eq!(parse(text), Ok(won), "{text}");
        }
    }

    #[test]
    fn missing_ambiguous_and_false_candidates_are_refused() {
        assert_eq!(parse("오늘의 소식 125,000원"), Err(MISSING));
        assert_eq!(parse("배송비 3,000원 보증금 50,000원"), Err(MISSING));
        assert_eq!(parse("객실 요금 99,000원 판매가 88,000원"), Err(AMBIGUOUS));
        for text in [
            "객실 요금 보증금 50,000원",
            "판매가 배송비 3,000원",
            "객실 요금 99,000원~120,000원",
            "객실 요금 99,000원 - 120,000원",
            "객실 요금 99,000원부터",
            "최저 객실 요금 99,000원",
            "정상 판매가 99,000원",
            "판매가 99,000원 할인",
            "지난주 객실 요금 90,000원",
            "원래 판매가 80,000원",
            "품절 판매가 59,000원",
            "객실 요금 79,000원 (예약 마감)",
            "할인 전 일반 디럭스 더블 객실 요금 90,000원",
            "starting 판매가 99,000원",
            "from 판매가 99,000원",
            "판매가 12.5만원",
            "판매가 99,000.50원",
            "판매가 1,00원",
            "판매가 12,34,567원",
            "판매가 ,500원",
            "판매가 0원",
            "판매가 007원",
            "판매가 7만5천원",
            "판매가 1,000만원",
            "판매가 $99",
            "판매가 99,000",
            "판매가 1000000000000원",
            "판매가 99,000원에서 79,000원",
            "판매가 문의",
        ] {
            assert!(
                matches!(parse(text), Err(UNCLEAR)),
                "{text}: {:?}",
                parse(text)
            );
        }
        // A longer word that merely starts with a cue is not a cue.
        assert_eq!(parse("판매가능 수량 3개"), Err(MISSING));
        assert_eq!(parse("정상판매가 99,000원"), Err(MISSING));
        assert_eq!(parse("객실 요금 99,000원 / 158,000원"), Err(AMBIGUOUS));
        assert_eq!(parse("객실 요금 99,000원대"), Err(UNCLEAR));
        for other in ["USD 120", "₩150,000", "120달러"] {
            assert_eq!(
                parse(&format!("객실 요금 99,000원 또는 {other}")),
                Err(AMBIGUOUS)
            );
        }
    }

    #[test]
    fn bounds_display_and_equal_versus_strict() {
        assert_eq!(parse("판매가 999,999,999,999원"), Ok(MAX_WON));
        assert_eq!(parse("판매가 1원"), Ok(1));
        assert_eq!(display(125_000), "125,000원");
        assert_eq!(display(7), "7원");
        assert_eq!(display(1_000_000), "1,000,000원");
        assert!(qualifies(100_000, 100_000, true));
        assert!(!qualifies(100_000, 100_000, false));
        assert!(qualifies(99_999, 100_000, false));
        assert!(!qualifies(100_001, 100_000, true));
    }
}
