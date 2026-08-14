use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use eas_mail_protocol::{Command, EasError, RequestSafety, Result, Transport, TransportResponse};

/// Scripted transport failure injected at a precise request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptedFailure {
    /// Retry-safe network failure.
    Network,
    /// Ambiguous mutation disconnect.
    OutcomeUnknown,
}

/// One exact OPTIONS or EAS command expectation.
#[derive(Debug)]
pub enum ExpectedCall {
    /// OPTIONS request.
    Options {
        /// Response status.
        status: u16,
        /// Lowercase response headers.
        headers: BTreeMap<String, String>,
    },
    /// ActiveSync command request.
    Command {
        /// Expected command.
        command: Command,
        /// Expected raw WBXML bytes.
        body: Vec<u8>,
        /// Expected policy-key header state.
        policy_key: Option<u32>,
        /// Expected retry classification.
        safety: RequestSafety,
        /// Response status.
        status: u16,
        /// Response body.
        response: Vec<u8>,
        /// Optional deterministic delay.
        delay: Duration,
        /// Optional injected failure.
        failure: Option<ScriptedFailure>,
    },
}

/// Queue-based transport that verifies exact EAS request ordering and bytes.
#[derive(Debug)]
pub struct ScriptedTransport {
    calls: Mutex<VecDeque<ExpectedCall>>,
    active_commands: AtomicUsize,
    max_concurrent_commands: AtomicUsize,
}

impl ScriptedTransport {
    /// Creates a transport from ordered expectations.
    #[must_use]
    pub fn new(calls: Vec<ExpectedCall>) -> Self {
        Self {
            calls: Mutex::new(calls.into()),
            active_commands: AtomicUsize::new(0),
            max_concurrent_commands: AtomicUsize::new(0),
        }
    }

    /// Returns the greatest number of command futures active at once.
    #[must_use]
    pub fn max_concurrent_commands(&self) -> usize {
        self.max_concurrent_commands.load(Ordering::Relaxed)
    }

    /// Returns an error unless every expected call was consumed.
    pub fn verify_complete(&self) -> Result<()> {
        let calls = self.calls.lock().map_err(|_| protocol("script lock failed"))?;
        if calls.is_empty() { Ok(()) } else { Err(protocol("script has unconsumed calls")) }
    }

    fn pop(&self) -> Result<ExpectedCall> {
        self.calls
            .lock()
            .map_err(|_| protocol("script lock failed"))?
            .pop_front()
            .ok_or_else(|| protocol("received an unexpected EAS request"))
    }
}

#[async_trait]
impl Transport for ScriptedTransport {
    async fn options(&self) -> Result<TransportResponse> {
        match self.pop()? {
            ExpectedCall::Options { status, headers } => {
                Ok(TransportResponse { status, body: Vec::new(), headers })
            }
            ExpectedCall::Command { .. } => Err(protocol("expected an EAS command, got OPTIONS")),
        }
    }

    async fn command(
        &self,
        command: Command,
        body: &[u8],
        policy_key: Option<u32>,
        safety: RequestSafety,
    ) -> Result<TransportResponse> {
        let ExpectedCall::Command {
            command: expected_command,
            body: expected_body,
            policy_key: expected_policy,
            safety: expected_safety,
            status,
            response,
            delay,
            failure,
        } = self.pop()?
        else {
            return Err(protocol("expected OPTIONS, got an EAS command"));
        };
        if command != expected_command
            || body != expected_body
            || policy_key != expected_policy
            || safety != expected_safety
        {
            return Err(protocol("EAS request did not match the scripted expectation"));
        }
        let active = self.active_commands.fetch_add(1, Ordering::Relaxed) + 1;
        self.max_concurrent_commands.fetch_max(active, Ordering::Relaxed);
        let _active_call = ActiveCall { active: &self.active_commands };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        match failure {
            Some(ScriptedFailure::Network) => Err(EasError::Network("scripted disconnect".into())),
            Some(ScriptedFailure::OutcomeUnknown) => Err(EasError::OutcomeUnknown),
            None => Ok(TransportResponse { status, body: response, headers: BTreeMap::new() }),
        }
    }

    async fn purge_secrets(&self) {}
}

struct ActiveCall<'a> {
    active: &'a AtomicUsize,
}

impl Drop for ActiveCall<'_> {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::Relaxed);
    }
}

fn protocol(message: &str) -> EasError {
    EasError::Protocol(message.into())
}
