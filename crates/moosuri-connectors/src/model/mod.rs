//! Bounded text and client-function model protocols. Not a durable agent runner.
pub mod chatgpt;
pub(crate) mod images;
pub use images::{MAX_IMAGE_BYTES, MAX_IMAGE_REQUEST_BYTES, MAX_IMAGES, NormalizedJpeg};
pub mod protocol;
mod quota;
mod transport;

pub fn public_model_address(ip: std::net::IpAddr) -> bool {
    transport::allowed(&transport::NetworkPolicy::PublicHttps, ip)
}

pub(crate) fn public_audio_address(ip: std::net::IpAddr) -> bool {
    transport::allowed(&transport::NetworkPolicy::PublicHttps, ip)
}

pub use tokio_util::sync::CancellationToken;
pub use transport::{
    Catalog, CatalogModel, ModelClient, NetworkPolicy, RouterPolicy, Secret, TransportError,
    TransportLimits,
};
