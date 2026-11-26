//! Private authentication worker. No raw CDP, page data or credentials leave it.
//! This API is for the installation launcher, never the model browser protocol.
mod binding;
mod chrome;
mod consent;
mod crypto;
mod display;
mod worker;
pub use binding::{Binding, Envelope, PROTOCOL, Recipient};
pub use crypto::Receiver;
pub use worker::{serve, installation_probe};
#[cfg(test)]
mod tests;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("google_runtime_invalid")]
    Invalid,
    #[error("google_runtime_unavailable")]
    Unavailable,
    #[error("google_runtime_blocked")]
    Blocked,
    #[error("google_runtime_needs_human")]
    NeedsHuman,
}
