use async_trait::async_trait;
use eas_mail_protocol::ProfileRegistry;

use super::super::accounts::AddRequest;
use super::super::terminal::Terminal;
use super::super::{accounts, clients, doctor};
use crate::{Paths, Result};

#[async_trait(?Send)]
pub(super) trait SetupActions {
    async fn add_account(
        &self,
        paths: &Paths,
        request: AddRequest,
        profiles: &ProfileRegistry,
        terminal: &mut dyn Terminal,
    ) -> Result<serde_json::Value>;

    async fn repair_account(
        &self,
        paths: &Paths,
        account_id: &str,
        profiles: &ProfileRegistry,
        terminal: &mut dyn Terminal,
    ) -> Result<serde_json::Value>;

    async fn update_password(
        &self,
        paths: &Paths,
        account_id: &str,
        profiles: &ProfileRegistry,
        terminal: &mut dyn Terminal,
    ) -> Result<serde_json::Value>;

    async fn set_writes_checked(
        &self,
        paths: &Paths,
        account_id: &str,
        enabled: bool,
        profiles: &ProfileRegistry,
    ) -> Result<serde_json::Value>;

    fn set_verified_writes(&self, paths: &Paths, account_id: &str) -> Result<serde_json::Value>;

    fn configure_clients(
        &self,
        paths: &Paths,
        terminal: &mut dyn Terminal,
    ) -> Result<Vec<serde_json::Value>>;

    async fn doctor(&self, paths: &Paths, profiles: &ProfileRegistry) -> Result<serde_json::Value>;
}

pub(super) struct SystemActions;

#[async_trait(?Send)]
impl SetupActions for SystemActions {
    async fn add_account(
        &self,
        paths: &Paths,
        request: AddRequest,
        profiles: &ProfileRegistry,
        terminal: &mut dyn Terminal,
    ) -> Result<serde_json::Value> {
        accounts::add(paths, request, profiles, Some(terminal)).await
    }

    async fn repair_account(
        &self,
        paths: &Paths,
        account_id: &str,
        profiles: &ProfileRegistry,
        terminal: &mut dyn Terminal,
    ) -> Result<serde_json::Value> {
        accounts::repair(paths, account_id, profiles, terminal).await
    }

    async fn update_password(
        &self,
        paths: &Paths,
        account_id: &str,
        profiles: &ProfileRegistry,
        terminal: &mut dyn Terminal,
    ) -> Result<serde_json::Value> {
        accounts::update_password(paths, account_id, false, profiles, terminal).await
    }

    async fn set_writes_checked(
        &self,
        paths: &Paths,
        account_id: &str,
        enabled: bool,
        profiles: &ProfileRegistry,
    ) -> Result<serde_json::Value> {
        accounts::set_writes_checked(paths, account_id, enabled, profiles).await
    }

    fn set_verified_writes(&self, paths: &Paths, account_id: &str) -> Result<serde_json::Value> {
        accounts::set_writes(paths, account_id, true)
    }

    fn configure_clients(
        &self,
        paths: &Paths,
        terminal: &mut dyn Terminal,
    ) -> Result<Vec<serde_json::Value>> {
        clients::configure_detected_with_terminal(paths, terminal)
    }

    async fn doctor(&self, paths: &Paths, profiles: &ProfileRegistry) -> Result<serde_json::Value> {
        doctor::run(paths, Some(profiles)).await
    }
}
