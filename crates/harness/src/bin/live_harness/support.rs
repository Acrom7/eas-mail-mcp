use std::io::{self, Write as _};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Report {
    pub version: &'static str,
    pub accounts: Vec<AccountReport>,
    pub self_write: bool,
    pub meeting_profiles: usize,
    pub meeting_directions: usize,
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
    pub calendar_writes_checked: bool,
    pub cold_mail_list_ms: u128,
    pub warm_mail_list_ms: u128,
}

pub fn confirm() -> anyhow::Result<()> {
    writeln!(
        io::stderr(),
        "This will exercise mail writes, temporary Calendar events, and meetings only between accounts that share one endpoint profile."
    )?;
    write!(io::stderr(), "Type SELF-WRITE to continue: ")?;
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    anyhow::ensure!(input.trim() == "SELF-WRITE", "self-write confirmation was not provided");
    Ok(())
}
