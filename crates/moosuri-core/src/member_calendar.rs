//! Member Calendar browser/route DTOs. One primary timed event create or move.
//! No invitations, attendees, all-day, recurrence, delete or model-visible event data.
//! Candidate titles/times appear only in the member's own browser response.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberCalendarKind {
    Create,
    Move,
}
impl MemberCalendarKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Move => "move",
        }
    }
}

/// Browser request. Civil values only: the server alone resolves the zone offset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberCalendarRequest {
    /// Stable UUID chosen by the browser before the first submit; reused on retry.
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub connection_id: String,
    pub epoch: i64,
    pub kind: MemberCalendarKind,
    /// Create: new event title. Move: exact title of the event to move.
    pub title: String,
    /// Target civil date `YYYY-MM-DD` and time `HH:MM` (24h) in `time_zone`.
    pub date: String,
    pub time: String,
    /// IANA zone requested by the member (or the browser zone when none was said).
    pub time_zone: String,
    /// Required for create. Move keeps the observed duration when absent.
    #[serde(default)]
    pub duration_minutes: Option<u32>,
    /// Move only: civil date (in `time_zone`) of the current event. Absent = next 30 days.
    #[serde(default)]
    pub from_date: Option<String>,
    /// Move only: explicit selection among current candidates.
    #[serde(default)]
    pub candidate_ref: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberCalendarApply {
    pub request: MemberCalendarRequest,
    /// Digest returned by `prepare`; the member approved exactly this final content.
    pub approved_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberCalendarCandidate {
    pub candidate_ref: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub time_zone: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberCalendarPreview {
    pub kind: MemberCalendarKind,
    pub digest: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub time_zone: String,
    /// Move only: the observed current event and the selected candidate handle.
    pub current: Option<MemberCalendarCandidate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberCalendarEventView {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub time_zone: Option<String>,
    pub html_link: Option<String>,
}

/// `prepare` result. `preview` is local only: nothing was sent to Google.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MemberCalendarPrepared {
    NeedsConnection {
        reason: String,
    },
    NeedsSelection {
        candidates: Vec<MemberCalendarCandidate>,
        partial: bool,
    },
    NotFound {
        partial: bool,
    },
    /// The selected candidate changed since it was listed; list again.
    Stale,
    Preview {
        preview: MemberCalendarPreview,
    },
}

/// `apply` result.
/// `confirmed` is only for a provider response or a readback that matched this
/// request's private marker. `unknown` means no readback proved the effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MemberCalendarReceipt {
    NeedsConnection {
        reason: String,
    },
    InProgress,
    Confirmed {
        confirmed_by: String,
        receipt: String,
        event: MemberCalendarEventView,
    },
    Conflict,
    Failed {
        code: String,
    },
    Unknown {
        code: String,
    },
    Withdrawn,
}

pub fn valid_civil_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7) || c.is_ascii_digit())
}
pub fn valid_clock(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 2 || c.is_ascii_digit())
        && s[..2].parse::<u8>().is_ok_and(|h| h < 24)
        && s[3..].parse::<u8>().is_ok_and(|m| m < 60)
}
impl MemberCalendarRequest {
    /// Shape check only. Zone/date/DST validity is decided by the store resolver.
    pub fn well_formed(&self) -> bool {
        let id = |s: &str| s.len() == 36 && uuid_like(s);
        id(&self.request_id)
            && id(&self.connection_id)
            && self.epoch > 0
            && !self.title.trim().is_empty()
            && self.title.chars().count() <= 200
            && !self.title.chars().any(char::is_control)
            && valid_civil_date(&self.date)
            && valid_clock(&self.time)
            && !self.time_zone.is_empty()
            && self.time_zone.len() <= 64
            && self
                .time_zone
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/_+-".contains(&c))
            && self
                .duration_minutes
                .is_none_or(|m| (1..=1440).contains(&m))
            && (self.kind == MemberCalendarKind::Move || self.duration_minutes.is_some())
            && self.from_date.as_deref().is_none_or(valid_civil_date)
            && (self.kind == MemberCalendarKind::Move
                || (self.from_date.is_none() && self.candidate_ref.is_none()))
            && self
                .candidate_ref
                .as_deref()
                .is_none_or(|c| c.len() == 32 && c.bytes().all(|b| b.is_ascii_hexdigit()))
    }
}
fn uuid_like(s: &str) -> bool {
    s.bytes().enumerate().all(|(i, c)| {
        if matches!(i, 8 | 13 | 18 | 23) {
            c == b'-'
        } else {
            c.is_ascii_hexdigit()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create() -> MemberCalendarRequest {
        MemberCalendarRequest {
            thread_id: None,
            request_id: "00000000-0000-4000-8000-000000000001".into(),
            connection_id: "00000000-0000-4000-8000-000000000002".into(),
            epoch: 1,
            kind: MemberCalendarKind::Create,
            title: "팀 회의".into(),
            date: "2026-11-26".into(),
            time: "15:00".into(),
            time_zone: "Asia/Seoul".into(),
            duration_minutes: Some(60),
            from_date: None,
            candidate_ref: None,
        }
    }
    #[test]
    fn request_shape_is_closed() {
        assert!(create().well_formed());
        let mut r = create();
        r.duration_minutes = None;
        assert!(!r.well_formed(), "create never invents a duration");
        r = create();
        r.time = "24:00".into();
        assert!(!r.well_formed());
        r = create();
        r.candidate_ref = Some("a".repeat(32));
        assert!(!r.well_formed(), "candidates are move-only");
        let unknown = serde_json::json!({"request_id":"x","attendees":["a@b.c"]});
        assert!(serde_json::from_value::<MemberCalendarRequest>(unknown).is_err());
    }
}
