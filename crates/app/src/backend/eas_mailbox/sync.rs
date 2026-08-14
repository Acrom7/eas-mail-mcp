use std::sync::Arc;

use eas_mail_protocol::{
    CalendarFields, ChangeData, ChangeKind, CollectionKind, EasError, MailFields, Patch, SyncPage,
};
use futures::{StreamExt as _, stream};

use super::super::BackendSync;
use super::session::{CollectionState, EasMailbox, SessionState};
use crate::{AppError, ErrorCode, Result};

const MAX_SYNC_PAGES: usize = 100;
const MAX_CONCURRENT_SYNCS: usize = 8;

struct SyncRequest {
    folder_id: String,
    kind: CollectionKind,
    sync_key: String,
    policy_key: u32,
    filter: u8,
    preview_size: usize,
}

impl EasMailbox {
    pub(super) async fn refresh_folders(&self) -> Result<Vec<eas_mail_protocol::Folder>> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let mut page = self.client.folder_sync(state.policy_key, &state.folder_sync_key).await;
        if matches!(page, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            page = self.client.folder_sync(state.policy_key, &state.folder_sync_key).await;
        }
        if matches!(page, Err(EasError::InvalidFolderSyncKey)) && state.folder_sync_key != "0" {
            state.folder_sync_key = "0".into();
            state.folders.clear();
            page = self.client.folder_sync(state.policy_key, "0").await;
        }
        let page = page.map_err(self.scoped_error())?;
        state.folder_sync_key = page.sync_key;
        for id in page.deleted_ids {
            state.folders.remove(&id);
            state.collections.remove(&id);
        }
        for folder in page.folders {
            state.folders.insert(folder.server_id.clone(), folder);
        }
        Ok(state.folders.values().cloned().collect())
    }

    pub(super) async fn sync_selected(
        &self,
        mail: bool,
        calendar: bool,
        refresh_folders: bool,
    ) -> Result<BackendSync> {
        if !mail && !calendar {
            return Ok(BackendSync { collections: 0, changes: 0 });
        }
        let folders_missing = self.state.lock().await.folders.is_empty();
        if refresh_folders || folders_missing {
            self.refresh_folders().await?;
        }
        let mut state = self.state.lock().await;
        let selected = state
            .folders
            .values()
            .filter_map(|folder| {
                let kind = folder.kind?;
                ((mail && kind == CollectionKind::Mail)
                    || (calendar && kind == CollectionKind::Calendar))
                    .then(|| (folder.server_id.clone(), kind))
            })
            .collect::<Vec<_>>();
        let collection_count = selected.len();
        let mut pending = selected;
        let mut changes = 0;
        for _ in 0..MAX_SYNC_PAGES {
            if pending.is_empty() {
                return Ok(BackendSync { collections: collection_count, changes });
            }
            let requests = prepare_requests(&mut state, pending)?;
            let responses = self.fetch_sync_batch(requests).await;
            let mut next = Vec::new();
            let mut refresh_policy = false;
            for (request, response) in responses {
                match response {
                    Ok(page) => {
                        if page.sync_key.is_empty() {
                            return Err(AppError::new(
                                ErrorCode::ProtocolError,
                                "Exchange returned an empty collection SyncKey",
                            )
                            .account(&self.account.account_id));
                        }
                        let needs_next = request.sync_key == "0" || page.more_available;
                        changes = changes.saturating_add(page.changes.len());
                        let collection = state
                            .collections
                            .get_mut(&request.folder_id)
                            .ok_or_else(state_error)?;
                        apply_page(collection, page)?;
                        if needs_next {
                            next.push((request.folder_id, request.kind));
                        }
                    }
                    Err(EasError::InvalidSyncKey) if request.sync_key != "0" => {
                        state
                            .collections
                            .insert(request.folder_id.clone(), CollectionState::new(request.kind));
                        next.push((request.folder_id, request.kind));
                    }
                    Err(EasError::PolicyRefreshRequired) => {
                        refresh_policy = true;
                        next.push((request.folder_id, request.kind));
                    }
                    Err(error) => return Err(self.scoped_error()(error)),
                }
            }
            if refresh_policy {
                self.refresh_policy(&mut state).await?;
            }
            pending = next;
        }
        Err(AppError::new(
            ErrorCode::ProtocolError,
            "Exchange exceeded the collection pagination limit",
        )
        .account(&self.account.account_id))
    }

    async fn fetch_sync_batch(
        &self,
        requests: Vec<SyncRequest>,
    ) -> Vec<(SyncRequest, eas_mail_protocol::Result<SyncPage>)> {
        stream::iter(requests)
            .map(|request| {
                let client = Arc::clone(&self.client);
                async move {
                    let response = client
                        .sync(
                            request.policy_key,
                            &request.folder_id,
                            &request.sync_key,
                            request.kind,
                            request.filter,
                            request.preview_size,
                        )
                        .await;
                    (request, response)
                }
            })
            .buffered(MAX_CONCURRENT_SYNCS)
            .collect()
            .await
    }
}

fn prepare_requests(
    state: &mut SessionState,
    pending: Vec<(String, CollectionKind)>,
) -> Result<Vec<SyncRequest>> {
    let mut requests = Vec::with_capacity(pending.len());
    for (folder_id, kind) in pending {
        let sync_key = state
            .collections
            .entry(folder_id.clone())
            .or_insert_with(|| CollectionState::new(kind))
            .sync_key
            .clone();
        let (filter, preview_size) = effective_sync_options(state, kind)?;
        requests.push(SyncRequest {
            folder_id,
            kind,
            sync_key,
            policy_key: state.policy_key,
            filter,
            preview_size,
        });
    }
    Ok(requests)
}

fn apply_page(collection: &mut CollectionState, page: SyncPage) -> Result<()> {
    collection.sync_key = page.sync_key;
    for change in page.changes {
        match (collection.kind, change.kind, change.data) {
            (
                CollectionKind::Mail,
                ChangeKind::Add | ChangeKind::Change,
                ChangeData::Mail(fields),
            ) => {
                patch_mail(collection.mail.entry(change.server_id).or_default(), fields);
            }
            (
                CollectionKind::Calendar,
                ChangeKind::Add | ChangeKind::Change,
                ChangeData::Calendar(fields),
            ) => {
                patch_calendar(collection.calendar.entry(change.server_id).or_default(), fields);
            }
            (CollectionKind::Mail, ChangeKind::Delete | ChangeKind::SoftDelete, _) => {
                collection.mail.remove(&change.server_id);
            }
            (CollectionKind::Calendar, ChangeKind::Delete | ChangeKind::SoftDelete, _) => {
                collection.calendar.remove(&change.server_id);
            }
            _ => return Err(state_error()),
        }
    }
    Ok(())
}

fn patch_mail(target: &mut MailFields, patch: MailFields) {
    apply(&mut target.subject, patch.subject);
    apply(&mut target.sender, patch.sender);
    apply(&mut target.recipients, patch.recipients);
    apply(&mut target.cc, patch.cc);
    apply(&mut target.received_at, patch.received_at);
    apply(&mut target.body, patch.body);
    apply(&mut target.body_truncated, patch.body_truncated);
    apply(&mut target.is_read, patch.is_read);
    apply(&mut target.importance, patch.importance);
    apply(&mut target.attachments, patch.attachments);
}

fn patch_calendar(target: &mut CalendarFields, patch: CalendarFields) {
    apply(&mut target.subject, patch.subject);
    apply(&mut target.body, patch.body);
    apply(&mut target.starts_at, patch.starts_at);
    apply(&mut target.ends_at, patch.ends_at);
    apply(&mut target.all_day, patch.all_day);
    apply(&mut target.location, patch.location);
    apply(&mut target.organizer, patch.organizer);
    apply(&mut target.attendees, patch.attendees);
    apply(&mut target.reminder_minutes, patch.reminder_minutes);
    apply(&mut target.recurrence, patch.recurrence);
    apply(&mut target.exceptions, patch.exceptions);
    apply(&mut target.meeting_status, patch.meeting_status);
}

fn apply<T>(target: &mut Patch<T>, patch: Patch<T>) {
    if let Patch::Value(value) = patch {
        *target = Patch::Value(value);
    }
}

fn state_error() -> AppError {
    AppError::new(ErrorCode::ProtocolError, "process-local Exchange state is inconsistent")
}

fn policy(state: &SessionState) -> Result<&eas_mail_protocol::protocol::PolicyDecision> {
    state.policy.as_ref().ok_or_else(state_error)
}

fn effective_sync_options(state: &SessionState, kind: CollectionKind) -> Result<(u8, usize)> {
    let policy = policy(state)?;
    let filter = match kind {
        CollectionKind::Mail => policy.mail_filter_type,
        CollectionKind::Calendar => policy.calendar_filter_type,
    };
    Ok((filter, policy.body_limit.min(500)))
}
