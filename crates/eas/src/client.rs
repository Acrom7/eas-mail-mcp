use std::sync::Arc;

use crate::protocol::{self, ComposeSource, PolicyDecision};
use crate::{
    CollectionKind, Command, EasError, FolderPage, ItemResult, MutationResult, RequestSafety,
    Result, SearchMail, SyncPage, Transport,
};

/// Successfully acknowledged Exchange policy and its final key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedPolicy {
    /// Final policy key to persist in Keychain.
    pub key: u32,
    /// Enforceable policy limits.
    pub decision: PolicyDecision,
}

/// Stateless EAS command client over an injected transport.
pub struct EasClient {
    transport: Arc<dyn Transport>,
}

impl EasClient {
    /// Creates a client over a strict production or scripted test transport.
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    /// Verifies EAS 14.1 and required server commands.
    pub async fn options(&self) -> Result<()> {
        let response = self.transport.options().await?;
        require_http_success(response.status)?;
        let versions =
            response.headers.get("ms-asprotocolversions").map(String::as_str).unwrap_or_default();
        if !versions.split(',').any(|value| value.trim() == "14.1") {
            return Err(EasError::Protocol("Exchange does not advertise EAS 14.1".into()));
        }
        let commands =
            response.headers.get("ms-asprotocolcommands").map(String::as_str).unwrap_or_default();
        for required in [
            "Provision",
            "FolderSync",
            "Sync",
            "Search",
            "ItemOperations",
            "SendMail",
            "SmartReply",
            "SmartForward",
        ] {
            if !commands.split(',').any(|value| value.trim() == required) {
                return Err(EasError::Protocol(format!(
                    "Exchange does not advertise required command {required}"
                )));
            }
        }
        Ok(())
    }

    /// Negotiates and acknowledges only policy requirements the client can enforce.
    pub async fn provision(&self) -> Result<NegotiatedPolicy> {
        let body = protocol::build_initial_provision()?;
        let response = self
            .transport
            .command(Command::Provision, &body, None, RequestSafety::RetrySafe)
            .await?;
        require_http_success(response.status)?;
        let initial = protocol::parse_provision(&response.body)?;
        if initial.remote_wipe || initial.account_only_remote_wipe {
            let acknowledgement = protocol::build_wipe_ack(initial.account_only_remote_wipe)?;
            let _ = self
                .transport
                .command(
                    Command::Provision,
                    &acknowledgement,
                    initial.policy_key,
                    RequestSafety::Mutation,
                )
                .await;
            self.transport.purge_secrets().await;
            return Err(EasError::AccountRemoteWipe);
        }
        if initial.status != 1 {
            return Err(EasError::Protocol(format!("Provision status is {}", initial.status)));
        }
        let temporary_key = initial
            .policy_key
            .ok_or_else(|| EasError::Protocol("Provision returned no policy key".into()))?;
        let decision = protocol::evaluate_policy(&initial.policy);
        let acknowledgement = protocol::build_policy_ack(temporary_key, decision.supported)?;
        let response = self
            .transport
            .command(Command::Provision, &acknowledgement, Some(0), RequestSafety::RetrySafe)
            .await?;
        require_http_success(response.status)?;
        let acknowledged = protocol::parse_provision(&response.body)?;
        if !decision.supported {
            return Err(EasError::UnsupportedDevicePolicy(decision.reasons.join("; ")));
        }
        if acknowledged.status != 1 || acknowledged.policy_status.is_some_and(|status| status != 1)
        {
            return Err(EasError::Protocol("Exchange rejected policy acknowledgement".into()));
        }
        let key = acknowledged
            .policy_key
            .ok_or_else(|| EasError::Protocol("Provision returned no final policy key".into()))?;
        Ok(NegotiatedPolicy { key, decision })
    }

    /// Synchronizes folder hierarchy once.
    pub async fn folder_sync(&self, key: u32, sync_key: &str) -> Result<FolderPage> {
        let body = protocol::build_folder_sync(sync_key)?;
        let response = self.read_command(Command::FolderSync, &body, key).await?;
        let page = protocol::parse_folder_sync(&response.body)?;
        match page.status {
            1 => Ok(page),
            9 => Err(EasError::InvalidFolderSyncKey),
            status => Err(EasError::Protocol(format!("FolderSync status is {status}"))),
        }
    }

    /// Synchronizes one collection page.
    pub async fn sync(
        &self,
        key: u32,
        collection_id: &str,
        sync_key: &str,
        kind: CollectionKind,
        filter_type: u8,
        preview_size: usize,
    ) -> Result<SyncPage> {
        let body = protocol::build_sync(collection_id, sync_key, kind, filter_type, preview_size)?;
        let response = self.read_command(Command::Sync, &body, key).await?;
        if response.body.is_empty() && sync_key != "0" {
            return Ok(SyncPage {
                account_status: 1,
                collection_status: 1,
                sync_key: sync_key.to_owned(),
                more_available: false,
                changes: Vec::new(),
            });
        }
        let page = protocol::parse_sync(&response.body, kind)?;
        match page.collection_status {
            1 => Ok(page),
            3 => Err(EasError::InvalidSyncKey),
            status => Err(EasError::Protocol(format!("Sync status is {status}"))),
        }
    }

    /// Searches mail on Exchange instead of a local cache.
    pub async fn search(
        &self,
        key: u32,
        query: &str,
        start: usize,
        limit: usize,
        preview_size: usize,
    ) -> Result<Vec<SearchMail>> {
        let body = protocol::build_search(query, start, limit, preview_size)?;
        let response = self.read_command(Command::Search, &body, key).await?;
        protocol::parse_search(&response.body)
    }

    /// Fetches a full mail item on demand.
    pub async fn fetch_item(
        &self,
        key: u32,
        long_id: Option<&str>,
        collection_id: Option<&str>,
        server_id: Option<&str>,
        body_limit: usize,
    ) -> Result<ItemResult> {
        let body =
            protocol::build_item_fetch(long_id, collection_id, server_id, body_limit.min(50_000))?;
        let response = self.read_command(Command::ItemOperations, &body, key).await?;
        protocol::parse_item_fetch(&response.body)
    }

    /// Downloads one attachment on demand.
    pub async fn fetch_attachment(&self, key: u32, reference: &str) -> Result<Vec<u8>> {
        let body = protocol::build_attachment_fetch(reference)?;
        let response = self.read_command(Command::ItemOperations, &body, key).await?;
        protocol::parse_attachment_fetch(&response.body)
    }

    /// Changes read state with no automatic network retry.
    pub async fn mark_read(
        &self,
        key: u32,
        collection_id: &str,
        server_id: &str,
        sync_key: &str,
        is_read: bool,
    ) -> Result<MutationResult> {
        let body = protocol::build_mark_read(collection_id, server_id, sync_key, is_read)?;
        let response = self.mutation_command(Command::Sync, &body, key).await?;
        protocol::parse_mutation_sync(&response.body)
    }

    /// Sends a new MIME message with an EAS ClientId.
    pub async fn send(&self, key: u32, client_id: &str, mime: Vec<u8>) -> Result<MutationResult> {
        let body = protocol::build_send(client_id, mime)?;
        let response = self.mutation_command(Command::SendMail, &body, key).await?;
        protocol::parse_compose(&response.body)
    }

    /// Replies to or forwards a referenced message.
    pub async fn smart_compose(
        &self,
        key: u32,
        forward: bool,
        client_id: &str,
        source: ComposeSource<'_>,
        mime: Vec<u8>,
    ) -> Result<MutationResult> {
        let body = protocol::build_smart(forward, client_id, source, mime)?;
        let command = if forward { Command::SmartForward } else { Command::SmartReply };
        let response = self.mutation_command(command, &body, key).await?;
        protocol::parse_compose(&response.body)
    }

    async fn read_command(
        &self,
        command: Command,
        body: &[u8],
        key: u32,
    ) -> Result<crate::TransportResponse> {
        let response =
            self.transport.command(command, body, Some(key), RequestSafety::RetrySafe).await?;
        normalize_command_response(response)
    }

    async fn mutation_command(
        &self,
        command: Command,
        body: &[u8],
        key: u32,
    ) -> Result<crate::TransportResponse> {
        let response =
            self.transport.command(command, body, Some(key), RequestSafety::Mutation).await?;
        normalize_command_response(response)
    }
}

fn normalize_command_response(
    response: crate::TransportResponse,
) -> Result<crate::TransportResponse> {
    if response.status == 449 {
        return Err(EasError::PolicyRefreshRequired);
    }
    require_http_success(response.status)?;
    Ok(response)
}

fn require_http_success(status: u16) -> Result<()> {
    match status {
        200 | 201 | 204 => Ok(()),
        401 | 403 => Err(EasError::Authentication),
        status => Err(EasError::Protocol(format!("Exchange returned HTTP {status}"))),
    }
}
