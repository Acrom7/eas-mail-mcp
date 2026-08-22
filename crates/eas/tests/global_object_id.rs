use eas_mail_protocol::protocol::global_object_id_uid;

#[test]
fn converts_outlook_global_object_id() -> anyhow::Result<()> {
    let value = "BAAAAIIA4AB0xbcQGoLgCAfUCRDgQMnBJoXEAQAAAAAAAAAAEAAAAAvw7UtuTulOnjnjhns3jvM=";
    let uid = global_object_id_uid(value)?;
    assert_eq!(
        uid,
        "040000008200E00074C5B7101A82E00800000000E040C9C12685C4010000000000000000100000000BF0ED4B6E4EE94E9E39E3867B378EF3"
    );
    Ok(())
}

#[test]
fn converts_vcal_global_object_id_with_whitespace() -> anyhow::Result<()> {
    let value = concat!(
        "BAAAAIIA4AB0xbcQGoLgCAAAAAAAAAAAAAAAAAAAAAAAAAAAMwAAAHZDYWwtVWlk",
        "\nAQAAAHs4MTQxMkQzQy0yQTI0LTRFOUQtQjIwRS0xMUY3QkJFOTI3OTl9AA=="
    );
    assert_eq!(global_object_id_uid(value)?, "{81412D3C-2A24-4E9D-B20E-11F7BBE92799}");
    Ok(())
}

#[test]
fn rejects_truncated_global_object_id() {
    assert!(global_object_id_uid("BAAAAIIA4AA=").is_err());
}
