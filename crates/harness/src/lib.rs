//! Deterministic EAS and stdio MCP test harness.

#![deny(missing_docs)]

mod deterministic;
mod fake_backend;
mod memory_journal;
mod scripted_transport;

pub use deterministic::{FixedClock, ManualClock, SequenceIds};
pub use fake_backend::FakeBackend;
pub use memory_journal::MemoryJournal;
pub use scripted_transport::{ExpectedCall, ScriptedFailure, ScriptedTransport};
