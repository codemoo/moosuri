//! Bounded text and client-function model protocols. Not a durable agent runner.
pub mod chatgpt;
pub mod protocol;
mod transport;

pub(crate) fn public_audio_address(ip: std::net::IpAddr) -> bool {
    transport::allowed(&transport::NetworkPolicy::PublicHttps, ip)
}

pub use tokio_util::sync::CancellationToken;
pub use transport::{
    Catalog, CatalogModel, ModelClient, NetworkPolicy, RouterPolicy, Secret, TransportError,
    TransportLimits,
};
