use super::oracle::Observation;
use crate::{EvalResult, model::Script, runner::World};
pub(super) async fn execute(
    memory: bool,
    admitted: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> EvalResult<Observation> {
    let mut world = World::setup(if memory {
        Script::PairedMemory
    } else {
        Script::PairedCalculation
    })
    .await?;
    let result = world.paired(memory, &admitted).await;
    world.shutdown().await;
    result
}
