//! `fungi-cli`: queue and inspect intents on a running `fungid`.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use fungi_rpc::{AddIntent, Client, ClientError};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Socket fungid listens on [default: $XDG_RUNTIME_DIR/fungid.sock]
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Queue a payment and print its id
    AddIntent {
        /// Address to pay, on fungid's network
        address: String,
        /// Whole sats
        amount_sat: u64,
    },
    /// Print queued intents, one per line: id, script_pubkey (hex), amount_sat, tab separated
    ListIntents,
    /// Stop fungid
    Shutdown,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("fungi-cli: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let socket = cli
        .socket
        .or_else(fungi_rpc::default_socket_path)
        .ok_or("XDG_RUNTIME_DIR is not set; pass --socket")?;
    let mut client = Client::connect(&socket).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
            format!("{err}; is fungid running? (socket: {})", socket.display())
        }
        _ => err.to_string(),
    })?;

    let mut out = io::stdout().lock();
    let written = match cli.command {
        Command::AddIntent {
            address,
            amount_sat,
        } => {
            let id = client
                .add_intent(AddIntent {
                    address,
                    amount_sat,
                })
                .map_err(rpc_error)?;
            writeln!(out, "{id}")
        }
        Command::ListIntents => {
            let intents = client.list_intents().map_err(rpc_error)?;
            intents.iter().try_for_each(|intent| {
                writeln!(
                    out,
                    "{}\t{}\t{}",
                    intent.id, intent.script_pubkey, intent.amount_sat
                )
            })
        }
        Command::Shutdown => {
            client.shutdown().map_err(rpc_error)?;
            Ok(())
        }
    };
    match written {
        // A closed pipe, as in `fungi-cli list-intents | head`, is not an error.
        Err(err) if err.kind() != io::ErrorKind::BrokenPipe => Err(err.to_string()),
        _ => Ok(()),
    }
}

fn rpc_error(err: ClientError) -> String {
    match err {
        // The client's read timeout surfaces as `WouldBlock` on Unix.
        ClientError::Io(err) if err.kind() == io::ErrorKind::WouldBlock => {
            "fungid did not answer in time".to_owned()
        }
        err => err.to_string(),
    }
}
