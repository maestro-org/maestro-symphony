use gasket::framework::*;

pub type DownstreamPort = gasket::messaging::tokio::OutputPort<()>;

#[derive(Stage)]
#[stage(name = "health", unit = "()", worker = "Worker")]
pub struct Stage {}

impl Stage {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct Worker {}

impl Worker {}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        unimplemented!()
    }

    async fn schedule(&mut self, stage: &mut Stage) -> Result<WorkSchedule<()>, WorkerError> {
        unimplemented!()
    }

    async fn execute(&mut self, unit: &(), stage: &mut Stage) -> Result<(), WorkerError> {
        unimplemented!()
    }

    async fn teardown(&mut self) -> Result<(), WorkerError> {
        Ok(())
    }
}
