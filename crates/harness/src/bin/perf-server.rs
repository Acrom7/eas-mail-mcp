use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::mcp::serve_stdio;
use eas_mail_mcp::{Clock, IdGenerator, OperationJournal, Runtime};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let now = chrono::DateTime::from_timestamp(1_700_000_000, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid harness time"))?;
    let backends: Vec<Arc<dyn AccountBackend>> =
        vec![Arc::new(FakeBackend::new("example").with_mail_count(100))];
    let journal: Arc<dyn OperationJournal> = Arc::new(MemoryJournal::default());
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(now));
    let ids: Arc<dyn IdGenerator> = Arc::new(SequenceIds::default());
    let runtime = Arc::new(Runtime::with_dependencies(
        backends,
        journal,
        clock,
        ids,
        vec![7; 32],
        temporary.path().join("attachments"),
    )?);
    serve_stdio(runtime).await
}
