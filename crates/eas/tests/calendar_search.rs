use eas_mail_protocol::Patch;
use eas_mail_protocol::protocol::{
    build_calendar_search, parse_calendar_item_fetch, parse_calendar_search,
};
use eas_mail_protocol::wbxml::{Element, decode, encode};

#[test]
fn calendar_search_request_is_class_scoped_and_metadata_only() -> anyhow::Result<()> {
    let data = build_calendar_search("Quarterly review", 0, 20)?;
    let tree = decode(&data)?.ok_or_else(|| anyhow::anyhow!("request is empty"))?;
    assert_eq!(
        tree.descendant("AirSync", "Class").map(Element::text_content),
        Some("Calendar".into())
    );
    assert_eq!(
        tree.descendant("AirSyncBase", "TruncationSize").map(Element::text_content),
        Some("0".into())
    );
    Ok(())
}

#[test]
fn calendar_search_and_item_fetch_parse_calendar_fields() -> anyhow::Result<()> {
    let search = parse_calendar_search(&encode(&search_response())?)?;
    assert_eq!(search.total, 3);
    assert_eq!(search.items.len(), 1);
    let first = search.items.first().ok_or_else(|| anyhow::anyhow!("search result is empty"))?;
    assert_eq!(first.long_id, "long-1");
    assert_eq!(first.fields.subject, Patch::Value("Planning".into()));

    let item = parse_calendar_item_fetch(&encode(&item_response())?)?;
    assert_eq!(item.fields.body, Patch::Value("Private agenda".into()));
    assert_eq!(item.fields.attendees, Patch::Value(vec!["a@example.com".into()]));
    Ok(())
}

fn search_response() -> Element {
    let mut root = Element::new("Search", "Search");
    let mut store = Element::new("Search", "Store");
    store.push(Element::text("Search", "Status", "1"));
    store.push(Element::text("Search", "Total", "3"));
    let mut result = Element::new("Search", "Result");
    result.push(Element::text("Search", "LongId", "long-1"));
    let mut properties = Element::new("Search", "Properties");
    properties.push(Element::text("Calendar", "Subject", "Planning"));
    properties.push(Element::text("Calendar", "StartTime", "20260824T090000Z"));
    properties.push(Element::text("Calendar", "EndTime", "20260824T100000Z"));
    result.push(properties);
    store.push(result);
    root.push(store);
    root
}

fn item_response() -> Element {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut response = Element::new("ItemOperations", "Response");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    let mut properties = Element::new("ItemOperations", "Properties");
    properties.push(Element::text("Calendar", "Subject", "Planning"));
    let mut body = Element::new("AirSyncBase", "Body");
    body.push(Element::text("AirSyncBase", "Data", "Private agenda"));
    properties.push(body);
    let mut attendees = Element::new("Calendar", "Attendees");
    let mut attendee = Element::new("Calendar", "Attendee");
    attendee.push(Element::text("Calendar", "Email", "a@example.com"));
    attendees.push(attendee);
    properties.push(attendees);
    fetch.push(properties);
    response.push(fetch);
    root.push(response);
    root
}
