use std::io::{self, Write as _};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Report {
    pub version: &'static str,
    pub accounts: Vec<AccountReport>,
    pub self_write: bool,
}

#[derive(Debug, Serialize)]
pub struct AccountReport {
    pub account_id: String,
    pub folders: usize,
    pub mail: usize,
    pub calendar: usize,
    pub search: usize,
    pub attachment_checked: bool,
    pub writes_checked: bool,
    pub cold_mail_list_ms: u128,
    pub warm_mail_list_ms: u128,
}

pub fn confirm() -> anyhow::Result<()> {
    writeln!(
        io::stderr(),
        "This will send, reply, forward, and temporarily toggle read state only on your own mailboxes."
    )?;
    write!(io::stderr(), "Type SELF-WRITE to continue: ")?;
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    anyhow::ensure!(input.trim() == "SELF-WRITE", "self-write confirmation was not provided");
    Ok(())
}
