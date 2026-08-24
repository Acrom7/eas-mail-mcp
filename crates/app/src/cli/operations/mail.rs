use serde::Serialize;

use super::mail_args::MailCommand;
use super::mail_input::{self, PagedInput};
use super::output::{self, OutputKind, OutputMode};
use super::writes;
use crate::cli::CliExit;
use crate::model::{ApiResponse, MailListInput, MailPage, MailSearchInput, MailSummary, Warning};
use crate::{AppError, Runtime};

#[derive(Serialize)]
struct CliMailPage {
    items: Vec<MailSummary>,
    results_truncated: bool,
}

pub(super) async fn run(
    runtime: &Runtime,
    command: MailCommand,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match command {
        MailCommand::List(arguments) => {
            let response = drain_list(runtime, mail_input::list(arguments)?).await;
            output::emit(response, mode, OutputKind::MailList, true)
        }
        MailCommand::Search(arguments) => {
            let response = drain_search(runtime, mail_input::search(arguments)?).await;
            output::emit(response, mode, OutputKind::MailList, true)
        }
        MailCommand::Get(arguments) => {
            let response = runtime.mail_get(mail_input::get(arguments)?).await;
            output::emit(response, mode, OutputKind::MailDetail, true)
        }
        MailCommand::Attachments(arguments) => {
            let response = runtime.mail_list_attachments(mail_input::attachments(arguments)?).await;
            output::emit(response, mode, OutputKind::Attachments, true)
        }
        MailCommand::Download(arguments) => {
            let response = runtime.mail_download_attachment(mail_input::download(arguments)?).await;
            output::emit(response, mode, OutputKind::Download, true)
        }
        MailCommand::MarkRead(arguments) => {
            let (input, yes) = mail_input::mark_read(arguments)?;
            writes::mail_mark_read(runtime, input, yes, mode).await
        }
        MailCommand::Send(arguments) => {
            let (input, yes) = mail_input::send(arguments)?;
            writes::mail_send(runtime, input, yes, mode).await
        }
        MailCommand::Reply(arguments) => {
            let (input, yes) = mail_input::reply(arguments)?;
            writes::mail_reply(runtime, input, yes, mode).await
        }
        MailCommand::Forward(arguments) => {
            let (input, yes) = mail_input::forward(arguments)?;
            writes::mail_forward(runtime, input, yes, mode).await
        }
    }
}

async fn drain_list(
    runtime: &Runtime,
    request: PagedInput<MailListInput>,
) -> ApiResponse<CliMailPage> {
    let mut input = request.input;
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    loop {
        input.limit = page_size(request.maximum, items.len());
        let response = runtime.mail_list(input.clone()).await;
        let page = match page(response, &mut warnings) {
            Ok(value) => value,
            Err(error) => return ApiResponse::failure(error.envelope),
        };
        let next = collect(&mut items, page, request.maximum);
        match next {
            PageState::Continue(cursor) => input.cursor = Some(cursor),
            PageState::Complete(truncated) => {
                return ApiResponse::success(
                    CliMailPage { items, results_truncated: truncated },
                    warnings,
                );
            }
        }
    }
}

async fn drain_search(
    runtime: &Runtime,
    request: PagedInput<MailSearchInput>,
) -> ApiResponse<CliMailPage> {
    let mut input = request.input;
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    loop {
        input.limit = page_size(request.maximum, items.len());
        let response = runtime.mail_search(input.clone()).await;
        let page = match page(response, &mut warnings) {
            Ok(value) => value,
            Err(error) => return ApiResponse::failure(error.envelope),
        };
        let next = collect(&mut items, page, request.maximum);
        match next {
            PageState::Continue(cursor) => input.cursor = Some(cursor),
            PageState::Complete(truncated) => {
                return ApiResponse::success(
                    CliMailPage { items, results_truncated: truncated },
                    warnings,
                );
            }
        }
    }
}

enum PageState {
    Continue(String),
    Complete(bool),
}

fn page(
    response: ApiResponse<MailPage>,
    warnings: &mut Vec<Warning>,
) -> Result<MailPage, AppError> {
    warnings.extend(response.warnings);
    if let Some(error) = response.error {
        return Err(AppError { envelope: error });
    }
    response.data.ok_or_else(|| {
        AppError::new(crate::ErrorCode::ProtocolError, "mail page response has no data")
    })
}

fn collect(items: &mut Vec<MailSummary>, mut page: MailPage, maximum: Option<usize>) -> PageState {
    let remaining = maximum.map_or(usize::MAX, |value| value.saturating_sub(items.len()));
    let page_overflow = page.items.len() > remaining;
    page.items.truncate(remaining);
    items.append(&mut page.items);
    if maximum.is_some_and(|value| items.len() >= value) {
        PageState::Complete(page_overflow || page.next_cursor.is_some())
    } else if let Some(cursor) = page.next_cursor {
        PageState::Continue(cursor)
    } else {
        PageState::Complete(false)
    }
}

fn page_size(maximum: Option<usize>, current: usize) -> Option<u8> {
    Some(maximum.map_or(100, |value| value.saturating_sub(current).min(100)) as u8)
}
