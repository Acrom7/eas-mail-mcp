mod account_secrets;
mod accounts;
mod clients;
mod doctor;

use std::io::Write as _;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand, ValueEnum};
use eas_mail_protocol::{ProfileKey, ProfileRegistry};

use crate::{AppError, ErrorCode, Paths, Result, Runtime, load_config};

/// Direct stdio MCP and local administration CLI.
#[derive(Debug, Parser)]
#[command(name = "eas-mail-mcp", about, disable_version_flag = true)]
struct Cli {
    /// Print application version information.
    #[arg(long)]
    version: bool,
    /// Include compile-time profile bundle metadata with --version.
    #[arg(long, requires = "version")]
    verbose: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the MCP server over stdin/stdout.
    Serve,
    /// Interactively add one managed Exchange account.
    Setup(SetupArgs),
    /// Manage account configuration and Keychain credentials.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Run redacted configuration and live EAS diagnostics.
    Doctor,
    /// Register or remove the MCP from an AI client.
    Client {
        #[command(subcommand)]
        command: ClientCommand,
    },
}

#[derive(Debug, Args)]
struct SetupArgs {
    /// Stable local account identifier.
    #[arg(long)]
    account_id: Option<String>,
    /// Managed Exchange profile.
    #[arg(long)]
    profile: Option<ProfileKey>,
    /// Mailbox address.
    #[arg(long)]
    email: Option<String>,
    /// Exchange or AD username.
    #[arg(long)]
    username: Option<String>,
    /// Read the password from stdin instead of a terminal prompt.
    #[arg(long)]
    password_stdin: bool,
    /// Enable write tools for this account; client confirmation remains mandatory.
    #[arg(long)]
    enable_writes: bool,
}

#[derive(Debug, Subcommand)]
enum AccountCommand {
    /// List configured accounts without credentials.
    List,
    /// Add and live-verify an account.
    Add(AddAccountArgs),
    /// Replace and live-verify an account password.
    UpdatePassword(PasswordArgs),
    /// Enable or disable write tools for one account.
    SetWrites(ToggleArgs),
    /// Remove account configuration and credentials.
    Remove(AccountIdArgs),
}

#[derive(Debug, Args)]
struct AddAccountArgs {
    /// Stable local account identifier.
    account_id: String,
    /// Managed Exchange profile.
    #[arg(long)]
    profile: ProfileKey,
    /// Mailbox address.
    #[arg(long)]
    email: String,
    /// Exchange or AD username.
    #[arg(long)]
    username: String,
    /// Read the password from stdin instead of a terminal prompt.
    #[arg(long)]
    password_stdin: bool,
    /// Enable write tools for this account.
    #[arg(long)]
    enable_writes: bool,
}

#[derive(Debug, Args)]
struct PasswordArgs {
    /// Stable local account identifier.
    account_id: String,
    /// Read the password from stdin instead of a terminal prompt.
    #[arg(long)]
    password_stdin: bool,
}

#[derive(Debug, Args)]
struct ToggleArgs {
    /// Stable local account identifier.
    account_id: String,
    /// New state.
    #[arg(value_enum)]
    value: Toggle,
}

#[derive(Debug, Args)]
struct AccountIdArgs {
    /// Stable local account identifier.
    account_id: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Toggle {
    On,
    Off,
}

#[derive(Debug, Subcommand)]
enum ClientCommand {
    /// Add the MCP and write-tool confirmation rules.
    Configure(ClientArgs),
    /// Remove only entries managed by this application.
    Unconfigure(ClientArgs),
}

#[derive(Debug, Args)]
struct ClientArgs {
    /// Supported AI client.
    #[arg(value_enum)]
    client: clients::ClientKind,
    /// Override the client executable used for version detection and setup.
    #[arg(long)]
    executable: Option<String>,
}

/// Parses and runs one CLI command.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.version {
        if cli.command.is_some() {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "--version cannot be combined with a command",
            ));
        }
        return emit_version(cli.verbose);
    }
    let command = cli.command.ok_or_else(|| {
        AppError::new(ErrorCode::ValidationFailed, "a command or --version is required")
    })?;
    let paths = Paths::standard()?;
    paths.ensure()?;
    match command {
        Command::Serve => {
            let config = load_config(&paths.config)?;
            let runtime = Arc::new(Runtime::production(config, &paths)?);
            crate::mcp::serve_stdio(runtime).await.map_err(|_| {
                AppError::new(ErrorCode::ProtocolError, "MCP stdio transport stopped unexpectedly")
            })
        }
        Command::Setup(arguments) => {
            let request = accounts::interactive_request(arguments)?;
            let result = accounts::add(&paths, request).await?;
            emit(&result)
        }
        Command::Account { command } => emit(&accounts::run(&paths, command).await?),
        Command::Doctor => emit(&doctor::run(&paths).await?),
        Command::Client { command } => emit(&clients::run(&paths, command)?),
    }
}

fn emit_version(verbose: bool) -> Result<()> {
    let mut output = std::io::stdout().lock();
    if !verbose {
        return writeln!(output, "eas-mail-mcp {}", env!("CARGO_PKG_VERSION"))
            .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"));
    }
    let registry = ProfileRegistry::compiled();
    let document = serde_json::json!({
        "name": "EAS Mail MCP",
        "binary": "eas-mail-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "profile_bundle": {
            "version": registry.bundle_version(),
            "sha256": registry.bundle_hash(),
            "development_only": registry.development_only(),
        },
    });
    let document = serde_json::to_string_pretty(&document)
        .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot serialize CLI output"))?;
    writeln!(output, "{document}")
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"))
}

fn emit(value: &serde_json::Value) -> Result<()> {
    let document = serde_json::to_string_pretty(value)
        .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot serialize CLI output"))?;
    writeln!(std::io::stdout().lock(), "{document}")
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"))
}

fn prompt(label: &str) -> Result<String> {
    let mut stderr = std::io::stderr().lock();
    write!(stderr, "{label}: ")
        .and_then(|()| stderr.flush())
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write terminal prompt"))?;
    let mut value = String::new();
    std::io::stdin()
        .read_line(&mut value)
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot read terminal input"))?;
    Ok(value.trim().to_owned())
}
