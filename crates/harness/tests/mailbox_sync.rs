#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use std::time::Duration;

use eas_mail_mcp::backend::AccountBackend as _;
use eas_mail_mcp_harness::ExpectedCall;
use eas_mail_protocol::protocol::{build_folder_sync, build_sync};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{CollectionKind, Command, Patch, RequestSafety};

use support::{
    calendar_change, default_policy, folder_response, mail_change, mailbox, options, read,
    sync_response,
};

#[tokio::test]
async fn mail_and_calendar_snapshots_apply_ordered_changes() -> anyhow::Result<()> {
    let calls = vec![
        options(),
        read(Command::FolderSync, build_folder_sync("0")?, folder_response("1", true)?),
        read(
            Command::Sync,
            build_sync("inbox", "0", CollectionKind::Mail, 5, 500)?,
            sync_response("mail-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("inbox", "mail-1", CollectionKind::Mail, 5, 500)?,
            sync_response(
                "mail-2",
                1,
                false,
                vec![
                    mail_change("Add", "message-1", Some("Initial")),
                    mail_change("Change", "message-1", Some("Changed")),
                    mail_change("Add", "message-2", Some("Deleted")),
                    mail_change("Delete", "message-2", None),
                    mail_change("Add", "message-3", Some("Soft deleted")),
                    mail_change("SoftDelete", "message-3", None),
                ],
            )?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 500)?,
            sync_response("calendar-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "calendar-1", CollectionKind::Calendar, 6, 500)?,
            sync_response("calendar-2", 1, false, vec![calendar_change("event-1", "Planning")])?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;

    let mail = mailbox.list_mail(None).await?;
    assert_eq!(mail.len(), 1);
    assert_eq!(
        mail.first().map(|item| &item.fields.subject),
        Some(&Patch::Value("Changed".into()))
    );
    let events = mailbox.list_calendar(None).await?;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events.first().map(|item| &item.fields.subject),
        Some(&Patch::Value("Planning".into()))
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn invalid_sync_key_bootstraps_then_fetches_changes() -> anyhow::Result<()> {
    let calls = vec![
        options(),
        read(Command::FolderSync, build_folder_sync("0")?, folder_response("1", true)?),
        read(
            Command::Sync,
            build_sync("inbox", "0", CollectionKind::Mail, 5, 500)?,
            sync_response("mail-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("inbox", "mail-1", CollectionKind::Mail, 5, 500)?,
            sync_response("mail-2", 1, false, vec![mail_change("Add", "old", Some("Old"))])?,
        ),
        read(
            Command::Sync,
            build_sync("inbox", "mail-2", CollectionKind::Mail, 5, 500)?,
            sync_response("mail-2", 3, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("inbox", "0", CollectionKind::Mail, 5, 500)?,
            sync_response("reset-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("inbox", "reset-1", CollectionKind::Mail, 5, 500)?,
            sync_response("reset-2", 1, false, vec![mail_change("Add", "new", Some("New"))])?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    assert_eq!(mailbox.list_mail(None).await?.len(), 1);
    let refreshed = mailbox.list_mail(None).await?;
    assert_eq!(refreshed.len(), 1);
    assert_eq!(
        refreshed.first().map(|item| &item.fields.subject),
        Some(&Patch::Value("New".into()))
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn explicit_empty_sync_is_a_noop() -> anyhow::Result<()> {
    let (mailbox, transport) = mailbox(Vec::new(), default_policy())?;
    let result = mailbox.sync(false, false).await?;
    assert_eq!(result.collections, 0);
    assert_eq!(result.changes, 0);
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn independent_collections_sync_with_bounded_concurrency() -> anyhow::Result<()> {
    let delay = Duration::from_millis(50);
    let calls = vec![
        options(),
        read(Command::FolderSync, build_folder_sync("0")?, two_mail_folders()?),
        delayed_sync("inbox", "0", "inbox-1", delay)?,
        delayed_sync("sent", "0", "sent-1", delay)?,
        delayed_sync("inbox", "inbox-1", "inbox-2", delay)?,
        delayed_sync("sent", "sent-1", "sent-2", delay)?,
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;

    assert!(mailbox.list_mail(None).await?.is_empty());
    assert!(transport.max_concurrent_commands() >= 2);
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn default_mail_list_syncs_only_inbox_and_sent() -> anyhow::Result<()> {
    let calls = vec![
        options(),
        read(Command::FolderSync, build_folder_sync("0")?, three_mail_folders()?),
        empty_mail_sync("inbox", "0", "inbox-1")?,
        empty_mail_sync("sent", "0", "sent-1")?,
        empty_mail_sync("inbox", "inbox-1", "inbox-2")?,
        empty_mail_sync("sent", "sent-1", "sent-2")?,
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;

    assert!(mailbox.list_mail(None).await?.is_empty());
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn explicit_mail_folder_syncs_only_that_collection() -> anyhow::Result<()> {
    let calls = vec![
        options(),
        read(Command::FolderSync, build_folder_sync("0")?, three_mail_folders()?),
        empty_mail_sync("archive", "0", "archive-1")?,
        empty_mail_sync("archive", "archive-1", "archive-2")?,
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;

    assert!(mailbox.list_mail(Some(&["archive".into()])).await?.is_empty());
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn explicit_sync_still_refreshes_every_mail_collection() -> anyhow::Result<()> {
    let calls = vec![
        options(),
        read(Command::FolderSync, build_folder_sync("0")?, three_mail_folders()?),
        empty_mail_sync("archive", "0", "archive-1")?,
        empty_mail_sync("inbox", "0", "inbox-1")?,
        empty_mail_sync("sent", "0", "sent-1")?,
        empty_mail_sync("archive", "archive-1", "archive-2")?,
        empty_mail_sync("inbox", "inbox-1", "inbox-2")?,
        empty_mail_sync("sent", "sent-1", "sent-2")?,
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;

    assert_eq!(mailbox.sync(true, false).await?.collections, 3);
    transport.verify_complete()?;
    Ok(())
}

fn delayed_sync(
    folder_id: &str,
    sync_key: &str,
    response_key: &str,
    delay: Duration,
) -> anyhow::Result<ExpectedCall> {
    Ok(ExpectedCall::Command {
        command: Command::Sync,
        body: build_sync(folder_id, sync_key, CollectionKind::Mail, 5, 500)?,
        policy_key: Some(123),
        safety: RequestSafety::RetrySafe,
        status: 200,
        response: sync_response(response_key, 1, false, Vec::new())?,
        delay,
        failure: None,
    })
}

fn empty_mail_sync(
    folder_id: &str,
    sync_key: &str,
    response_key: &str,
) -> anyhow::Result<ExpectedCall> {
    Ok(read(
        Command::Sync,
        build_sync(folder_id, sync_key, CollectionKind::Mail, 5, 500)?,
        sync_response(response_key, 1, false, Vec::new())?,
    ))
}

fn two_mail_folders() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("FolderHierarchy", "FolderSync");
    root.push(Element::text("FolderHierarchy", "Status", "1"));
    root.push(Element::text("FolderHierarchy", "SyncKey", "folders-1"));
    let mut changes = Element::new("FolderHierarchy", "Changes");
    for (server_id, display_name, folder_type) in [("inbox", "Inbox", "2"), ("sent", "Sent", "5")] {
        let mut add = Element::new("FolderHierarchy", "Add");
        add.push(Element::text("FolderHierarchy", "ServerId", server_id));
        add.push(Element::text("FolderHierarchy", "ParentId", "0"));
        add.push(Element::text("FolderHierarchy", "DisplayName", display_name));
        add.push(Element::text("FolderHierarchy", "Type", folder_type));
        changes.push(add);
    }
    root.push(changes);
    encode(&root)
}

fn three_mail_folders() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("FolderHierarchy", "FolderSync");
    root.push(Element::text("FolderHierarchy", "Status", "1"));
    root.push(Element::text("FolderHierarchy", "SyncKey", "folders-1"));
    let mut changes = Element::new("FolderHierarchy", "Changes");
    for (server_id, display_name, folder_type) in
        [("archive", "Archive", "12"), ("inbox", "Inbox", "2"), ("sent", "Sent", "5")]
    {
        let mut add = Element::new("FolderHierarchy", "Add");
        add.push(Element::text("FolderHierarchy", "ServerId", server_id));
        add.push(Element::text("FolderHierarchy", "ParentId", "0"));
        add.push(Element::text("FolderHierarchy", "DisplayName", display_name));
        add.push(Element::text("FolderHierarchy", "Type", folder_type));
        changes.push(add);
    }
    root.push(changes);
    encode(&root)
}
