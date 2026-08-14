use futures::future::join_all;

use super::Runtime;
use super::convert::calendar_event;
use crate::model::{CalendarGetInput, CalendarListInput, CalendarPage, CalendarSearchInput};
use crate::sanitize::limit;
use crate::{ApiResponse, AppError, ErrorCode, Result};

impl Runtime {
    /// Performs a fresh calendar list or advances its immutable snapshot.
    pub async fn calendar_list(&self, input: CalendarListInput) -> ApiResponse<CalendarPage> {
        Self::response(self.calendar_list_result(input).await)
    }

    /// Refreshes calendars and searches safe event fields in process memory.
    pub async fn calendar_search(&self, input: CalendarSearchInput) -> ApiResponse<CalendarPage> {
        Self::response(self.calendar_search_result(input).await)
    }

    /// Resolves one process-local calendar event reference.
    pub fn calendar_get(&self, input: CalendarGetInput) -> ApiResponse<crate::CalendarEvent> {
        Self::response(self.calendar_get_result(input))
    }

    async fn calendar_list_result(
        &self,
        input: CalendarListInput,
    ) -> Result<(CalendarPage, Vec<crate::Warning>)> {
        let page_limit = limit(input.limit.map(u32::from), 50, 100)?;
        if let Some(cursor) = input.cursor {
            let (items, next_cursor) = self.references.next_calendar_page(&cursor, page_limit)?;
            return Ok((CalendarPage { items, next_cursor }, Vec::new()));
        }
        let (events, warnings) =
            self.fresh_events(input.account_ids.as_deref(), input.folder_ids).await?;
        let (items, next_cursor) = self.references.first_calendar_page(events, page_limit)?;
        Ok((CalendarPage { items, next_cursor }, warnings))
    }

    async fn calendar_search_result(
        &self,
        input: CalendarSearchInput,
    ) -> Result<(CalendarPage, Vec<crate::Warning>)> {
        let page_limit = limit(input.limit.map(u32::from), 50, 100)?;
        if let Some(cursor) = input.cursor {
            let (items, next_cursor) = self.references.next_calendar_page(&cursor, page_limit)?;
            return Ok((CalendarPage { items, next_cursor }, Vec::new()));
        }
        let query = input.query.trim().to_lowercase();
        if query.is_empty() {
            return Err(AppError::new(ErrorCode::ValidationFailed, "search query is empty"));
        }
        let (mut events, warnings) = self.fresh_events(input.account_ids.as_deref(), None).await?;
        events.retain(|event| {
            [
                event.subject.as_str(),
                event.body.as_str(),
                event.location.as_str(),
                event.organizer.as_str(),
            ]
            .iter()
            .any(|value| value.to_lowercase().contains(&query))
        });
        let (items, next_cursor) = self.references.first_calendar_page(events, page_limit)?;
        Ok((CalendarPage { items, next_cursor }, warnings))
    }

    fn calendar_get_result(
        &self,
        input: CalendarGetInput,
    ) -> Result<(crate::CalendarEvent, Vec<crate::Warning>)> {
        let event = self.references.event(&input.event_ref)?;
        Ok((calendar_event(input.event_ref, &event.fields, &event), Vec::new()))
    }

    async fn fresh_events(
        &self,
        account_ids: Option<&[String]>,
        folder_ids: Option<Vec<String>>,
    ) -> Result<(Vec<crate::CalendarEvent>, Vec<crate::Warning>)> {
        let backends = self.selected(account_ids)?;
        let results = join_all(backends.into_iter().map(|backend| {
            let folders = folder_ids.clone();
            async move {
                let id = backend.account().account_id;
                (id, backend.list_calendar(folders.as_deref()).await)
            }
        }))
        .await;
        let (groups, warnings) = self.collect_partial(results)?;
        let events = groups
            .into_iter()
            .flatten()
            .map(|event| self.calendar_event(event))
            .collect::<Result<Vec<_>>>()?;
        Ok((events, warnings))
    }
}
