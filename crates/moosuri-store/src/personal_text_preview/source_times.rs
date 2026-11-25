use std::borrow::Cow;

/// Keep a member's explicitly requested source times visible when the answer
/// only addresses a conditional follow-up action. All labels and values come
/// from this request. The answer's final line and action state stay intact.
pub(super) fn include_requested_times<'a>(question: &str, answer: &'a str) -> Cow<'a, str> {
    let lower = question.to_lowercase();
    let per_source = ["각각", "자료별", "출처별", "each source", "each end time"]
        .iter()
        .any(|phrase| lower.contains(phrase));
    let end_time = [
        "종료 시각",
        "종료 시간",
        "끝나는 시간",
        "끝나는 시각",
        "몇 시에 끝",
        "end time",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase));
    if !end_time {
        return Cow::Borrowed(answer);
    }
    let mut sources = Vec::new();
    for section in question.split('[').skip(1) {
        let Some((label, body)) = section.split_once(']') else {
            return Cow::Borrowed(answer);
        };
        let label = label.trim();
        if label.is_empty()
            || label.chars().count() > 40
            || !label.chars().all(|c| c.is_alphanumeric() || c == ' ')
        {
            return Cow::Borrowed(answer);
        }
        let mut ranges = body
            .match_indices(['–', '—', '-', '~', '～'])
            .filter_map(|(at, dash)| {
                let left = body[..at].trim_end();
                let right = body[at + dash.len()..].trim_start();
                let start = left
                    .rsplit(|c: char| !(c.is_ascii_digit() || c == ':'))
                    .next()?;
                let end = right
                    .split(|c: char| !(c.is_ascii_digit() || c == ':'))
                    .next()?;
                Some((clock(start)?, clock(end)?))
            });
        if let Some((_, end)) = ranges.next() {
            if ranges.next().is_some() || sources.iter().any(|(seen, _)| *seen == label) {
                return Cow::Borrowed(answer);
            }
            sources.push((label, end));
        }
    }
    if (per_source && !(2..=6).contains(&sources.len()))
        || (!per_source && (sources.len() != 1 || answer.contains(sources[0].1)))
        || sources.iter().all(|(label, end)| {
            answer
                .lines()
                .any(|line| line.contains(label) && line.contains(end))
        })
    {
        return Cow::Borrowed(answer);
    }
    let mut cited = String::new();
    for (label, end) in sources {
        cited.push_str(&format!("- {label} · {end}\n"));
    }
    cited.push('\n');
    cited.push_str(answer);
    if cited.len() > 64 * 1024 {
        return Cow::Borrowed(answer);
    }
    Cow::Owned(cited)
}

fn clock(value: &str) -> Option<&str> {
    let (hour, minute) = value.split_once(':')?;
    (hour.len() <= 2
        && minute.len() == 2
        && hour.parse::<u8>().ok()? < 24
        && minute.parse::<u8>().ok()? < 60)
        .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_only_verbatim_end_times_for_explicit_comparisons() {
        let question = "[안내문] 08:00–09:10 [보충 자료] 08:40 - 10:25. 종료 시각을 각각 알려줘.";
        assert_eq!(
            include_requested_times(question, "확정 전입니다."),
            "- 안내문 · 09:10\n- 보충 자료 · 10:25\n\n확정 전입니다."
        );
        assert_eq!(
            include_requested_times(question, "안내문 09:10, 보충 자료 10:25"),
            "안내문 09:10, 보충 자료 10:25"
        );
        assert!(
            include_requested_times(question, "09:10과 10:25입니다.")
                .starts_with("- 안내문 · 09:10")
        );
        assert!(
            include_requested_times("[정정 공지] 09:00–10:05. 몇 시에 끝나?", "겹치지 않습니다.")
                .starts_with("- 정정 공지 · 10:05")
        );
        for q in [
            "[A] 11:00-12:00 [B] 10:00-11:30. 더 늦은 자료 이름만.",
            "[A] 11:00-25:08 [B] 10:00-11:30. 종료 시각을 각각 비교.",
            "[A] 08:00-09:00, 11:00-12:00 [B] 10:00-11:30. 종료 시각을 각각 비교.",
        ] {
            assert_eq!(include_requested_times(q, "answer"), "answer");
        }
    }
}
