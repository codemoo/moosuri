//! Japan Post's fixed public summary endpoint; no cookies, keys or invented history.
use crate::web::WebError;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
pub const HOST: &str = "trackings.post.japanpost.jp";
pub const ENDPOINT: &str = "https://trackings.post.japanpost.jp/services/srv/search";
#[derive(Serialize)]
pub struct ParcelEvent {
    pub reported_time_text: String,
    pub local_date: Option<String>,
    pub occurred_at: Option<String>,
    pub status_text: String,
    pub handling_office: String,
    pub prefecture: String,
}
#[derive(Serialize)]
pub struct ParcelSource {
    pub url: String,
    pub title: String,
    pub retrieved_at_unix_ms: u64,
    pub truncated: bool,
    pub visible_excerpt: String,
}
#[derive(Serialize)]
pub struct ParcelResult {
    pub carrier: &'static str,
    pub tracking_number: String,
    pub state: &'static str,
    pub events: Vec<ParcelEvent>,
    pub source: ParcelSource,
    pub coverage: &'static str,
}
pub fn normalize(raw: &str) -> Result<String, WebError> {
    if raw.len() > 32
        || raw.is_empty()
        || !raw.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(WebError::InvalidUrl);
    }
    let n = raw.replace('-', "").to_ascii_uppercase();
    let b = n.as_bytes();
    let domestic = matches!(b.len(), 11 | 12) && b.iter().all(u8::is_ascii_digit);
    let international = b.len() == 13
        && b[..2].iter().all(u8::is_ascii_uppercase)
        && b[2..11].iter().all(u8::is_ascii_digit)
        && b[11..].iter().all(u8::is_ascii_uppercase)
        && b[0] != b'U';
    if !domestic && !international {
        return Err(WebError::InvalidUrl);
    }
    Ok(n)
}
pub fn url(number: &str) -> Result<url::Url, WebError> {
    let n = normalize(number)?;
    let mut url = url::Url::parse(ENDPOINT).map_err(|_| WebError::InvalidUrl)?;
    url.query_pairs_mut()
        .append_pair("requestNo1", &n)
        .append_pair("locale", "en")
        .append_pair("search.x", "1")
        .append_pair("search.y", "1");
    Ok(url)
}
fn text(e: ElementRef<'_>) -> String {
    e.text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn selector(s: &str) -> Selector {
    Selector::parse(s).expect("static parcel selector")
}
/// Header/cell shape was observed from the official English form/not-found page.
/// Unknown successful markup fails closed; no full-history selectors are guessed.
pub(crate) fn parse(
    html: &str,
    number: &str,
    source_url: String,
    at: u64,
) -> Result<ParcelResult, WebError> {
    let n = normalize(number)?;
    if html.len() > 1024 * 1024 {
        return Err(WebError::TooLarge);
    }
    let page = Html::parse_document(html);
    let title = page
        .select(&selector("title"))
        .next()
        .map(text)
        .unwrap_or_default();
    if title.len() > 512 {
        return Err(WebError::InvalidResponse);
    }
    let mut result = ParcelResult {
        carrier: "japan_post",
        tracking_number: n.clone(),
        state: "provider_changed",
        events: vec![],
        source: ParcelSource {
            url: source_url,
            title,
            retrieved_at_unix_ms: at,
            truncated: false,
            visible_excerpt: String::new(),
        },
        coverage: "latest_summary_only",
    };
    let expected = [
        "Item number",
        "Class of goods",
        "Latest date",
        "Latest status",
        "Latest handling office",
        "Prefecture",
        "ZIP code（Postal code number）",
    ];
    let mut matching = vec![];
    for table in page.select(&selector("table")).take(32) {
        let headers = table.select(&selector("th")).map(text).collect::<Vec<_>>();
        if headers == expected {
            matching.push(table)
        }
    }
    if matching.len() != 1 {
        return Ok(result);
    }
    let rows = matching[0].select(&selector("tr")).collect::<Vec<_>>();
    if rows.len() > 8 {
        return Ok(result);
    }
    let mut candidates = vec![];
    for row in rows.iter().skip(2) {
        let cells = row.select(&selector("td")).map(text).collect::<Vec<_>>();
        if cells
            .first()
            .is_some_and(|s| normalize(s).ok().as_deref() == Some(n.as_str()))
        {
            candidates.push(cells)
        }
    }
    if candidates.len() != 1 {
        return Ok(result);
    }
    let cells = &candidates[0];
    if cells.iter().any(|v| v.len() > 512) {
        return Err(WebError::TooLarge);
    }
    result.source.visible_excerpt = cells.join(" | ");
    if cells.len() == 2
        && cells[1]
            == "** Your item was not found. Confirm your item number and ask at your local office."
    {
        result.state = "not_found";
        return Ok(result);
    }
    if cells.len() != 6 || cells[2].is_empty() || cells[3].is_empty() || cells[3].starts_with("**")
    {
        return Ok(result);
    }
    let raw = cells[2].clone();
    let local_date = ["%Y/%m/%d", "%Y-%m-%d"]
        .iter()
        .find_map(|fmt| chrono::NaiveDate::parse_from_str(&raw, fmt).ok())
        .map(|d| d.format("%Y-%m-%d").to_string());
    // Only an explicit provider RFC3339 offset is an absolute event timestamp.
    let occurred_at = chrono::DateTime::parse_from_rfc3339(&raw)
        .ok()
        .map(|v| v.to_rfc3339());
    result.events.push(ParcelEvent {
        reported_time_text: raw,
        local_date,
        occurred_at,
        status_text: cells[3].clone(),
        handling_office: cells[4].clone(),
        prefecture: cells[5].clone(),
    });
    result.state = "observed";
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parcel_tracking_formats_fixed_endpoint_and_unestablished_markup_fail_closed() {
        assert_eq!(normalize("0000-0000-000").unwrap(), "00000000000");
        assert_eq!(normalize("aa000000000jp").unwrap(), "AA000000000JP");
        for value in [
            "https://evil.invalid/12345678901",
            "0000000000000",
            "UL000000000JP",
            "CNG00000000000000",
        ] {
            assert!(normalize(value).is_err());
        }
        let u = url("00000000000").unwrap();
        assert_eq!(u.host_str(), Some(HOST));
        assert_eq!(u.path(), "/services/srv/search");
        let changed = parse(
            "<html><title>Changed</title><table><tr><td>Delivered</td></tr></table></html>",
            "00000000000",
            u.to_string(),
            123,
        )
        .unwrap();
        assert_eq!(changed.state, "provider_changed");
        assert!(changed.events.is_empty());
        assert_eq!(changed.source.retrieved_at_unix_ms, 123);
    }
}
