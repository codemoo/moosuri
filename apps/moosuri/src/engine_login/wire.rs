use super::{Owner, Route};
use moosuri_connectors::model::protocol::{
    FinishOutcome, ModelEvent, TextMessage, TextRequest, TextRole, Usage,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;
use zeroize::Zeroizing;

pub(super) const HEADER_LIMIT: usize = 2048;
pub(super) const PROMPT_LIMIT: usize = 512 * 1024;
pub(super) const EVENT_LIMIT: usize = 512 * 1024;

pub(super) async fn send<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
    limit: usize,
) -> Result<(), String> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| "engine_protocol_invalid")?);
    if bytes.len() > limit {
        return Err("engine_protocol_limit".into());
    }
    writer
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|_| "engine_transport_lost")?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|_| "engine_transport_lost".into())
}
pub(super) async fn receive<T: DeserializeOwned>(
    reader: &mut (impl AsyncRead + Unpin),
    limit: usize,
) -> Result<T, String> {
    let size = reader
        .read_u32()
        .await
        .map_err(|_| "engine_transport_lost")? as usize;
    if size == 0 || size > limit {
        return Err("engine_protocol_limit".into());
    }
    let mut bytes = Zeroizing::new(vec![0; size]);
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|_| "engine_transport_lost")?;
    serde_json::from_slice(&bytes).map_err(|_| "engine_protocol_invalid".into())
}

#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Hello {
    pub version: u8,
    pub route: Route,
    pub owner: Owner,
    pub revision: Uuid,
    pub model: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Role {
    System,
    User,
    Assistant,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    role: Role,
    text: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prompt {
    messages: Vec<Message>,
    max_output_tokens: u32,
}
impl Prompt {
    pub(super) fn from_request(r: &TextRequest) -> Result<Self, String> {
        if r.effort.is_some() || r.messages.iter().any(|m| m.phase.is_some()) {
            return Err("engine_request_unsupported".into());
        }
        let messages = r
            .messages
            .iter()
            .map(|m| Message {
                role: match m.role {
                    TextRole::System => Role::System,
                    TextRole::Assistant => Role::Assistant,
                    TextRole::User => Role::User,
                },
                text: m.text.clone(),
            })
            .collect();
        let prompt = Self {
            messages,
            max_output_tokens: r.max_output_tokens,
        };
        prompt.validate()?;
        Ok(prompt)
    }
    fn validate(&self) -> Result<(), String> {
        if self.messages.is_empty()
            || self.messages.len() > 32
            || !(1..=2048).contains(&self.max_output_tokens)
            || self.messages.iter().any(|m| m.text.is_empty())
            || self.messages.iter().map(|m| m.text.len()).sum::<usize>() > 65536
        {
            return Err("engine_request_invalid".into());
        }
        Ok(())
    }
    pub(super) fn into_request(self, model: &str) -> Result<TextRequest, String> {
        self.validate()?;
        Ok(TextRequest {
            model: model.to_owned(),
            messages: self
                .messages
                .into_iter()
                .map(|m| TextMessage {
                    role: match m.role {
                        Role::System => TextRole::System,
                        Role::Assistant => TextRole::Assistant,
                        Role::User => TextRole::User,
                    },
                    text: m.text,
                    phase: None,
                })
                .collect(),
            effort: None,
            max_output_tokens: self.max_output_tokens,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event", deny_unknown_fields)]
pub(super) enum Event {
    Text {
        text: String,
    },
    Refusal {
        text: String,
    },
    Usage {
        input: Option<u64>,
        output: Option<u64>,
    },
    Finished {
        outcome: Outcome,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Completed,
    Incomplete,
    Refused,
    ContinuationRequired,
}
impl Event {
    pub(super) fn text_len(&self) -> usize {
        match self {
            Self::Text { text } | Self::Refusal { text } => text.len(),
            _ => 0,
        }
    }
    pub(super) fn finished(&self) -> bool {
        matches!(self, Self::Finished { .. })
    }
    pub(super) fn from_model(event: ModelEvent) -> Option<Self> {
        Some(match event {
            ModelEvent::TextDelta { text, .. } => Self::Text { text },
            ModelEvent::RefusalDelta { text, .. } => Self::Refusal { text },
            ModelEvent::UsageSnapshot(u) => Self::Usage {
                input: u.input_tokens,
                output: u.output_tokens,
            },
            ModelEvent::Finished { outcome, .. } => Self::Finished {
                outcome: match outcome {
                    FinishOutcome::Completed => Outcome::Completed,
                    FinishOutcome::Refused => Outcome::Refused,
                    FinishOutcome::Incomplete => Outcome::Incomplete,
                    FinishOutcome::ContinuationRequired => Outcome::ContinuationRequired,
                },
            },
            ModelEvent::Started { .. }
            | ModelEvent::MessageMetadata { .. }
            | ModelEvent::UsageBreakdown(_) => return None,
        })
    }
    pub(super) fn into_model(self) -> Result<ModelEvent, String> {
        Ok(match self {
            Self::Text { text } => ModelEvent::TextDelta {
                text,
                item_id: None,
                phase: None,
            },
            Self::Refusal { text } => ModelEvent::RefusalDelta {
                text,
                item_id: None,
                phase: None,
            },
            Self::Usage { input, output } => {
                if input
                    .into_iter()
                    .chain(output)
                    .any(|n| n > 1_000_000_000_000)
                {
                    return Err("engine_usage_invalid".into());
                }
                ModelEvent::UsageSnapshot(Usage {
                    input_tokens: input,
                    output_tokens: output,
                    ..Usage::default()
                })
            }
            Self::Finished { outcome } => ModelEvent::Finished {
                outcome: match outcome {
                    Outcome::Completed => FinishOutcome::Completed,
                    Outcome::Refused => FinishOutcome::Refused,
                    Outcome::Incomplete => FinishOutcome::Incomplete,
                    Outcome::ContinuationRequired => FinishOutcome::ContinuationRequired,
                },
                native_reason: "engine_terminal".into(),
            },
        })
    }
}
