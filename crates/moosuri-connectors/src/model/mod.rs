//! Text-only model protocol and transport foundation. Not a durable agent runner.
pub mod protocol;
mod transport;

pub use tokio_util::sync::CancellationToken;
pub use transport::{
    Catalog, CatalogModel, ModelClient, NetworkPolicy, RouterPolicy, Secret, TransportError,
    TransportLimits,
};
