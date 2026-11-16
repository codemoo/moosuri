//! A complete current imperative yields an offset, not a guessed new timestamp.
use super::*;

pub(super) fn parse(raw: &str, selected: bool) -> Option<Decision> {
    let lower = raw.to_ascii_lowercase();
    if !selected && !raw.contains("알림") && !lower.contains("reminder") {
        return None;
    }
    for (verb, direction) in [("늦춰", 1), ("미뤄", 1), ("앞당겨", -1)] {
        for ending in [" 줘", "줘", " 주세요", "주세요"] {
            if let Some(head) = raw.strip_suffix(&format!("{verb}{ending}")) {
                let head = head.trim();
                let mut parts = std::iter::once(0).chain(
                    head.char_indices()
                        .filter_map(|(at, c)| c.is_whitespace().then_some(at)),
                );
                let value = parts.find_map(|at| {
                    let target = ko_target(head[..at].trim(), selected)?;
                    let seconds = duration(head[at..].trim())?;
                    Some((target, seconds))
                });
                return Some(decide(raw, value, direction));
            }
        }
    }
    for (prefix, direction) in [("delay ", 1), ("postpone ", 1), ("advance ", -1)] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let value = rest.rfind(" by ").and_then(|cut| {
                let head = &raw[prefix.len()..prefix.len() + cut];
                Some((en_target(head)?, duration(&rest[cut + 4..])?))
            });
            return Some(decide(raw, value, direction));
        }
    }
    None
}

fn decide(raw: &str, value: Option<(Target, i64)>, direction: i64) -> Decision {
    match value.filter(|_| shell(raw)) {
        Some((target, seconds)) => Decision::Command(Parsed {
            action: Action::Edit,
            rule: None,
            shift_seconds: Some(direction * seconds),
            message: None,
            target: Some(target),
        }),
        None => Decision::Clarify(CONTROL),
    }
}

fn duration(value: &str) -> Option<i64> {
    let mut rest = value.strip_suffix("만큼").unwrap_or(value).trim();
    let mut seconds = 0i64;
    let mut previous_unit = i64::MAX;
    while !rest.is_empty() {
        let (n, tail) = number(rest)?;
        let (unit, tail) = [
            ("weeks", 604800),
            ("week", 604800),
            ("주", 604800),
            ("days", 86400),
            ("day", 86400),
            ("일", 86400),
            ("hours", 3600),
            ("hour", 3600),
            ("시간", 3600),
            ("minutes", 60),
            ("minute", 60),
            ("분", 60),
            ("seconds", 1),
            ("second", 1),
            ("초", 1),
        ]
        .into_iter()
        .find_map(|(name, unit)| {
            tail.trim_start()
                .strip_prefix(name)
                .map(|tail| (unit, tail.trim()))
        })?;
        if unit >= previous_unit || n == 0 {
            return None;
        }
        seconds = seconds.checked_add(i64::from(n).checked_mul(unit)?)?;
        if seconds > 366 * 86400 {
            return None;
        }
        previous_unit = unit;
        rest = tail;
    }
    (seconds > 0).then_some(seconds)
}
