#!/usr/bin/env python3
"""Equivalent direct-stdio Python MCP baseline for the performance gate."""

from __future__ import annotations

from datetime import UTC, datetime
from html.parser import HTMLParser
from itertools import count

from mcp.server.mcpserver import MCPServer
from pydantic import BaseModel, Field


MESSAGE_COUNT = 100
server = MCPServer(name="python-stdio-baseline", version="0.1.0", log_level="ERROR")
reference_ids = count()
references: dict[str, dict[str, str]] = {}


class PlainText(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []

    def handle_data(self, data: str) -> None:
        self.parts.append(data)


class MailSummary(BaseModel):
    mail_ref: str
    account_id: str
    folder_id: str
    subject: str
    sender: str
    recipients: str
    received_at: datetime | None
    preview: str
    is_read: bool
    has_attachments: bool
    untrusted_external_content: bool


class MailPage(BaseModel):
    items: list[MailSummary]
    next_cursor: str | None


class MailResponse(BaseModel):
    data: MailPage
    error: None = None
    warnings: list[dict[str, str]] = Field(default_factory=list)


def sanitize(value: str) -> str:
    parser = PlainText()
    parser.feed(value)
    return " ".join("".join(parser.parts).split())


def fake_messages() -> list[dict[str, object]]:
    return [
        {
            "account_id": "example",
            "folder_id": "inbox",
            "server_id": f"message-{index}",
            "subject": "Quarterly update",
            "sender": "Sender <sender@example.invalid>",
            "recipients": "example@example.invalid",
            "received_at": datetime.fromtimestamp(1_700_000_000, UTC),
            "body": "<p>Safe <strong>plain</strong> body</p>",
            "is_read": False,
            "has_attachments": True,
        }
        for index in range(MESSAGE_COUNT)
    ]


def summarize(message: dict[str, object]) -> MailSummary:
    mail_ref = f"mail_{next(reference_ids):016x}"
    references[mail_ref] = {
        "account_id": str(message["account_id"]),
        "folder_id": str(message["folder_id"]),
        "server_id": str(message["server_id"]),
    }
    return MailSummary(
        mail_ref=mail_ref,
        account_id=str(message["account_id"]),
        folder_id=str(message["folder_id"]),
        subject=str(message["subject"]),
        sender=str(message["sender"]),
        recipients=str(message["recipients"]),
        received_at=message["received_at"],
        preview=sanitize(str(message["body"]))[:500],
        is_read=bool(message["is_read"]),
        has_attachments=bool(message["has_attachments"]),
        untrusted_external_content=True,
    )


@server.tool(structured_output=True)
async def mail_list(limit: int = 100) -> MailResponse:
    """List deterministic fake-EAS messages without marking them as read."""
    if limit < 1 or limit > 100:
        raise ValueError("limit must be between 1 and 100")
    items = [summarize(message) for message in fake_messages()]
    items.sort(key=lambda item: item.received_at or datetime.min.replace(tzinfo=UTC), reverse=True)
    return MailResponse(data=MailPage(items=items[:limit], next_cursor=None))


if __name__ == "__main__":
    server.run("stdio")
