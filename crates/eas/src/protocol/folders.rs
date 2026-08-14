use crate::wbxml::{decode, encode};
use crate::{CollectionKind, EasError, Folder, FolderPage, Result};

use super::tree::{descendant_text, direct_text, element, integer, push_text};

/// Builds a FolderSync request.
pub fn build_folder_sync(sync_key: &str) -> Result<Vec<u8>> {
    let mut root = element("FolderHierarchy", "FolderSync");
    push_text(&mut root, "FolderHierarchy", "SyncKey", sync_key);
    encode(&root)
}

/// Parses added, updated, and deleted folders from FolderSync.
pub fn parse_folder_sync(data: &[u8]) -> Result<FolderPage> {
    let root = decode(data)?
        .ok_or_else(|| EasError::Protocol("Exchange returned an empty FolderSync".into()))?;
    let status = integer(descendant_text(&root, "FolderHierarchy", "Status"), 0);
    let sync_key = descendant_text(&root, "FolderHierarchy", "SyncKey").unwrap_or_default();
    let mut folders = Vec::new();
    for command in root
        .descendants("FolderHierarchy", "Add")
        .into_iter()
        .chain(root.descendants("FolderHierarchy", "Update"))
    {
        let folder_type = integer(direct_text(command, "FolderHierarchy", "Type"), 0);
        let kind = match folder_type {
            2 | 3 | 4 | 5 | 6 | 12 => Some(CollectionKind::Mail),
            8 | 13 => Some(CollectionKind::Calendar),
            _ => None,
        };
        let server_id = direct_text(command, "FolderHierarchy", "ServerId").unwrap_or_default();
        if !server_id.is_empty() {
            folders.push(Folder {
                server_id,
                parent_id: direct_text(command, "FolderHierarchy", "ParentId")
                    .unwrap_or_else(|| "0".into()),
                display_name: direct_text(command, "FolderHierarchy", "DisplayName")
                    .unwrap_or_default(),
                folder_type,
                kind,
            });
        }
    }
    let deleted_ids = root
        .descendants("FolderHierarchy", "Delete")
        .into_iter()
        .filter_map(|item| direct_text(item, "FolderHierarchy", "ServerId"))
        .filter(|id| !id.is_empty())
        .collect();
    Ok(FolderPage { status, sync_key, folders, deleted_ids })
}
