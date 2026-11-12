//! Bounded text and client-function model protocols. Not a durable agent runner.
pub mod protocol;
mod transport;

pub use tokio_util::sync::CancellationToken;
pub use transport::{
    Catalog, CatalogModel, ModelClient, NetworkPolicy, RouterPolicy, Secret, TransportError,
    TransportLimits,
};
