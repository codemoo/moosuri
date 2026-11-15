//! Independently written from public structural evidence, not copied scraper code.
//! Exact source revisions and current-provider limits: docs/parcel-tracking.md.
use super::*;
pub const MAX_EVENTS: usize = 64;
pub fn url(number: &str) -> Result<url::Url, WebError> {
    let n = normalize(number)?;
    let mut u = url::Url::parse(&format!("{ENDPOINT}/direct")).map_err(|_| WebError::InvalidUrl)?;
    u.query_pairs_mut()
        .append_pair("reqCodeNo1", &n)
        .append_pair("locale", "ja");
    Ok(u)
}
fn direct_cells(row: ElementRef<'_>, tag: &str) -> Vec<String> {
    row.children()
        .filter_map(ElementRef::wrap)
        .filter(|e| e.value().name() == tag)
        .map(text)
        .collect()
}
fn cells_with_spans(row: ElementRef<'_>, tag: &str, spans: &[u8]) -> bool {
    let cells = row
        .children()
        .filter_map(ElementRef::wrap)
        .collect::<Vec<_>>();
    cells.len() == spans.len()
        && cells.iter().zip(spans).all(|(cell, span)| {
            cell.value().name() == tag
                && cell.value().attr("rowspan").unwrap_or("1") == span.to_string()
                && cell.value().attr("colspan").unwrap_or("1") == "1"
                && cell.select(&selector("table, tr, td, th")).count() == 0
        })
}
fn history_headers(first: ElementRef<'_>, second: ElementRef<'_>) -> bool {
    // Exact Japanese labels/order and spans read from the official explanatory
    // history tables. Do not guess aliases for an unseen current detail layout.
    ["th", "td"].iter().any(|tag| {
        cells_with_spans(first, tag, &[2, 2, 2, 1, 2])
            && cells_with_spans(second, tag, &[1])
            && matches!(
                direct_cells(first, tag)
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice(),
                ["発生日", "状態", "詳細", "取扱局名", "県名等"]
                    | ["状態発生日", "状態", "詳細", "郵便局名", "県名等"]
            )
            && direct_cells(second, tag) == ["郵便番号"]
    })
}
pub(crate) fn parse(
    html: &str,
    number: &str,
    source_url: String,
    at: u64,
) -> Result<ParcelResult, WebError> {
    // Reuse the established English not-found shape only if it exactly matches.
    let mut result = super::parse(html, number, source_url, at)?;
    result.coverage = "history_unavailable";
    if result.state == "not_found" {
        return Ok(result);
    }
    result.state = "provider_changed";
    result.events.clear();
    result.source.visible_excerpt.clear();
    let page = Html::parse_document(html);
    // This exact Japanese not-found row/header shape was publicly observed with
    // the synthetic all-zero number on the direct endpoint on 2026-11-15.
    let summaries = page
        .select(&selector("table[summary='照会結果']"))
        .collect::<Vec<_>>();
    if summaries.len() == 1 {
        let headers = summaries[0]
            .select(&selector("th"))
            .map(text)
            .collect::<Vec<_>>();
        let rows = summaries[0].select(&selector("tr")).collect::<Vec<_>>();
        if headers
            == [
                "お問い合わせ番号",
                "商品種別",
                "最新年月日",
                "最新状態",
                "最新取扱局",
                "県名等",
                "郵便番号",
            ]
            && rows.len() == 3
        {
            let cells = direct_cells(rows[2], "td");
            if cells.len() == 2
                && normalize(&cells[0]).ok().as_deref() == Some(result.tracking_number.as_str())
                && cells[1]
                    == "＊＊ お問い合わせ番号が見つかりません。お問い合わせ番号をご確認の上、お近くの取扱店にお問い合わせください。"
            {
                result.state = "not_found";
                result.source.visible_excerpt = cells.join(" | ");
                return Ok(result);
            }
        }
    }
    let details = page
        .select(&selector(
            "form[name='srv_searchActionForm'] table[summary='配達状況詳細']",
        ))
        .collect::<Vec<_>>();
    let histories = page
        .select(&selector(
            "form[name='srv_searchActionForm'] table[summary='履歴情報']",
        ))
        .collect::<Vec<_>>();
    if details.len() != 1 || histories.len() != 1 {
        return Ok(result);
    }
    let identity_rows = details[0].select(&selector("tr")).collect::<Vec<_>>();
    if identity_rows.len() != 2 {
        return Ok(result);
    }
    let identity = direct_cells(identity_rows[1], "td");
    if !cells_with_spans(identity_rows[0], "th", &[1, 1])
        || direct_cells(identity_rows[0], "th") != ["お問い合わせ番号", "商品種別"]
        || !cells_with_spans(identity_rows[1], "td", &[1, 1])
        || identity.len() != 2
        || normalize(&identity[0]).ok().as_deref() != Some(result.tracking_number.as_str())
    {
        return Ok(result);
    }
    let rows = histories[0]
        .select(&selector("tr"))
        .take(2 + MAX_EVENTS * 2 + 1)
        .collect::<Vec<_>>();
    if rows.len() < 4 || rows.len() > 2 + MAX_EVENTS * 2 || rows.len() % 2 != 0 {
        return Ok(result);
    }
    // Published implementations establish the row pairing; official tables
    // independently establish supported header semantics/order and spans.
    if !history_headers(rows[0], rows[1]) {
        return Ok(result);
    }
    let mut events = Vec::new();
    let mut excerpt = vec![identity.join(" | ")];
    for pair in rows[2..].chunks_exact(2) {
        let cells = direct_cells(pair[0], "td");
        let post = direct_cells(pair[1], "td");
        if !cells_with_spans(pair[0], "td", &[2, 2, 2, 1, 2])
            || !cells_with_spans(pair[1], "td", &[1])
            || cells.len() != 5
            || post.len() != 1
            || cells[0].is_empty()
            || cells[1].is_empty()
            || cells.iter().chain(post.iter()).any(|v| v.len() > 512)
        {
            return Ok(result);
        }
        let raw = &cells[0];
        let local_date = raw
            .split_whitespace()
            .next()
            .and_then(|date| {
                ["%Y/%m/%d", "%Y-%m-%d"]
                    .iter()
                    .find_map(|fmt| chrono::NaiveDate::parse_from_str(date, fmt).ok())
            })
            .map(|d| d.format("%Y-%m-%d").to_string());
        let occurred_at = chrono::DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|v| v.to_rfc3339());
        events.push(ParcelEvent {
            reported_time_text: raw.clone(),
            local_date,
            occurred_at,
            status_text: cells[1].clone(),
            detail_text: Some(cells[2].clone()),
            handling_office: cells[3].clone(),
            prefecture: cells[4].clone(),
            postal_code: Some(post[0].clone()),
        });
        excerpt.push(cells.join(" | "));
        excerpt.push(post[0].clone());
    }
    // Preserve provider order; do not sort ambiguous overseas dates or infer year/zone.
    result.source.visible_excerpt = excerpt.join("\n");
    result.events = events;
    result.state = "observed";
    result.coverage = "official_visible_history";
    // Leave room in the existing16KiB typed tool limit for server provenance and
    // the External wrapper. Oversized histories fail, never silently drop events.
    if serde_json::to_vec(&result)
        .map_err(|_| WebError::InvalidResponse)?
        .len()
        > 12 * 1024
    {
        return Err(WebError::TooLarge);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    const NUMBER: &str = "00000000000";
    fn fixture() -> String {
        include_str!("history.synthetic.html").into()
    }
    #[test]
    fn history_preserves_paired_rows_dates_and_identity_without_inferred_timezone() {
        let r = parse(&fixture(), NUMBER, url(NUMBER).unwrap().to_string(), 123).unwrap();
        assert_eq!(r.state, "observed");
        assert_eq!(r.coverage, "official_visible_history");
        assert_eq!(r.events.len(), 2);
        let alternate = fixture()
            .replace("状態発生日", "発生日")
            .replace("郵便局名", "取扱局名");
        assert_eq!(
            parse(&alternate, NUMBER, String::new(), 1).unwrap().state,
            "observed"
        );
        assert_eq!(r.events[0].local_date.as_deref(), Some("2026-11-14"));
        assert_eq!(r.events[1].reported_time_text, "11/15 03:04");
        assert!(r.events[1].local_date.is_none());
        assert!(r.events.iter().all(|e| e.occurred_at.is_none()));
        assert_eq!(r.events[1].detail_text.as_deref(), Some("Synthetic detail"));
        assert_eq!(r.events[1].postal_code.as_deref(), Some("000-0000"));
        for html in [
            fixture().replace(NUMBER, "00000000001"),
            fixture().replace("<td>000-0000</td>", ""),
            fixture() + &fixture(),
            fixture()
                .replace(
                    "<th rowspan=\"2\">詳細</th><th>郵便局名</th>",
                    "<th rowspan=\"2\">郵便局名</th><th>詳細</th>",
                )
                .replace(
                    "<td rowspan=\"2\">Synthetic detail</td><td>Synthetic office</td>",
                    "<td rowspan=\"2\">Synthetic office</td><td>Synthetic detail</td>",
                ),
            fixture().replace("状態発生日", "Unknown date column"),
            fixture().replace(
                "<td>Synthetic origin</td>",
                "<th>Injected header</th><td>Synthetic origin</td>",
            ),
            fixture().replace(
                "<td rowspan=\"2\">Synthetic transit</td>",
                "<td>Synthetic transit</td>",
            ),
            fixture().replace("<td>000-0000</td>", "<td colspan=\"2\">000-0000</td>"),
        ] {
            let r = parse(&html, NUMBER, String::new(), 1).unwrap();
            assert_eq!(r.state, "provider_changed");
            assert_eq!(r.coverage, "history_unavailable");
            assert!(r.events.is_empty());
        }
        let not_found = "<table summary='照会結果'><tr><th>お問い合わせ番号</th><th>商品種別</th><th>最新年月日</th><th>最新状態</th><th>最新取扱局</th><th>県名等</th></tr><tr><th>郵便番号</th></tr><tr><td>00000000000</td><td>＊＊ お問い合わせ番号が見つかりません。お問い合わせ番号をご確認の上、お近くの取扱店にお問い合わせください。</td></tr></table>";
        let r = parse(not_found, NUMBER, String::new(), 1).unwrap();
        assert_eq!(r.state, "not_found");
        assert_eq!(r.coverage, "history_unavailable");
        assert!(r.events.is_empty());
        let mut html = fixture();
        let pair = "<tr><td>11/15</td><td>Synthetic</td><td></td><td></td><td></td></tr><tr><td></td></tr>";
        html = html.replace(
            "</table></form>",
            &(pair.repeat(MAX_EVENTS) + "</table></form>"),
        );
        assert!(
            parse(&html, NUMBER, String::new(), 1)
                .unwrap()
                .events
                .is_empty()
        );
    }
}
