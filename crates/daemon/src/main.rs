//! `fungid`: owns the intent queue, serves JSON-RPC on a Unix socket and runs one session for
//! as long as it lives. Runs in the foreground; leave supervision to systemd or launchd.

mod session;

use std::fs::{self, Permissions};
use std::io;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::Instant;

use bitcoin::address::NetworkUnchecked;
use bitcoin::{Address, Amount, Network};
use clap::Parser;
use cost_function::decision_space::DecisionSpace;
use cost_function::intent::{FixedPaymentInstructions, Intent, Outcome, PayoffCurve};
use cost_function::queue::{InMemoryQueue, Queue};
use fungi_core::persist::InMemoryPersister;
use fungi_rpc::{AddIntent, Call, Error, ListedIntent, Request, Response, TwoPointZero};
use futures::channel::mpsc::{self, UnboundedSender};
use futures::stream;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;
use tracing::{error, info, warn};

use session::{Input, StubSession};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Socket to listen on [default: $XDG_RUNTIME_DIR/fungid.sock]
    #[arg(long)]
    socket: Option<PathBuf>,

    /// Network that intent addresses must belong to
    #[arg(long, default_value_t = Network::Bitcoin)]
    network: Network,
}

/// What RPC handlers share with the session.
struct Daemon {
    queue: Mutex<InMemoryQueue>,
    plans: UnboundedSender<Input>,
    network: Network,
    shutdown: Notify,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().with_writer(io::stderr).init();
    let args = Args::parse();

    let Some(socket) = args.socket.or_else(fungi_rpc::default_socket_path) else {
        error!("XDG_RUNTIME_DIR is not set; pass --socket");
        return ExitCode::FAILURE;
    };
    let listener = match bind(&socket) {
        Ok(listener) => listener,
        Err(err) => {
            error!(socket = %socket.display(), %err, "cannot listen");
            return ExitCode::FAILURE;
        }
    };
    info!(socket = %socket.display(), network = %args.network, "listening");

    // shortcut: the event log only grows; bound it once sessions end or durable storage lands.
    let persister = InMemoryPersister::default();
    let (plans, plan_stream) = mpsc::unbounded();
    let daemon = Daemon {
        queue: Mutex::new(InMemoryQueue::new([])),
        plans,
        network: args.network,
        shutdown: Notify::new(),
    };
    let session = fungi_driver::run(
        StubSession,
        &persister,
        stream::pending(),
        plan_stream,
        session::run_effect,
    );

    let code = tokio::select! {
        _ = session => {
            error!("session ended, which it never should");
            ExitCode::FAILURE
        }
        Err(err) = serve(&listener, &daemon) => {
            error!(%err, "cannot accept connections");
            ExitCode::FAILURE
        }
        result = shutdown_requested(&daemon.shutdown) => match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                error!(%err, "cannot listen for signals");
                ExitCode::FAILURE
            }
        },
    };

    if let Err(err) = fs::remove_file(&socket) {
        warn!(socket = %socket.display(), %err, "cannot remove socket");
    }
    info!("stopped");
    code
}

/// Listen on `path`, readable and writable by this user only. Replaces a stale socket left by
/// a daemon that did not stop cleanly, but nothing else.
fn bind(path: &Path) -> io::Result<UnixListener> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_socket() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "path exists and is not a socket",
            ));
        }
        Ok(_) if std::os::unix::net::UnixStream::connect(path).is_ok() => {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another fungid is listening",
            ));
        }
        Ok(_) => fs::remove_file(path)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Serve connections one at a time, forever. Only failing to accept ends it.
// shortcut: a client that holds its connection open blocks every other client; serve each
// connection in its own task once more than one client talks to fungid at a time.
async fn serve(listener: &UnixListener, daemon: &Daemon) -> io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        if let Err(err) = serve_connection(stream, daemon).await {
            warn!(%err, "connection failed");
        }
    }
}

async fn serve_connection(stream: UnixStream, daemon: &Daemon) -> io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let (response, shutdown) = daemon.handle(&line);
        let mut line = serde_json::to_string(&response).expect("responses always serialize");
        line.push('\n');
        write.write_all(line.as_bytes()).await?;
        if shutdown {
            daemon.shutdown.notify_one();
            return Ok(());
        }
    }
    Ok(())
}

async fn shutdown_requested(notify: &Notify) -> io::Result<()> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    tokio::select! {
        () = notify.notified() => info!("shutdown requested"),
        _ = terminate.recv() => info!("SIGTERM"),
        _ = interrupt.recv() => info!("SIGINT"),
    }
    Ok(())
}

impl Daemon {
    /// Answer one request line. The flag says whether the daemon should stop once the response
    /// is written.
    fn handle(&self, line: &str) -> (Response, bool) {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(err) => return (failure(None, Error::PARSE_ERROR, err), false),
        };
        let id = value.get("id").and_then(Value::as_u64);
        // shortcut: unknown methods and bad params report -32600 with serde's message; split
        // them into -32601/-32602 once a client needs to tell them apart.
        let request: Request = match serde_json::from_value(value) {
            Ok(request) => request,
            Err(err) => return (failure(id, Error::INVALID_REQUEST, err), false),
        };

        let id = Some(request.id);
        let (result, shutdown) = match request.call {
            Call::AddIntent(params) => (self.add_intent(params).map(Value::from), false),
            Call::ListIntents => (Ok(serde_json::json!(self.list_intents())), false),
            Call::Shutdown => (Ok(Value::Null), true),
        };
        let response = match result {
            Ok(value) => Response {
                jsonrpc: TwoPointZero,
                id,
                outcome: fungi_rpc::Outcome::Result(value),
            },
            Err(err) => failure(id, Error::INVALID_PARAMS, err),
        };
        (response, shutdown)
    }

    /// Queue an intent and hand the session a fresh plan covering the whole queue.
    fn add_intent(&self, params: AddIntent) -> Result<u64, String> {
        let address = params
            .address
            .parse::<Address<NetworkUnchecked>>()
            .map_err(|err| err.to_string())?
            .require_network(self.network)
            .map_err(|_| format!("address is not for {}", self.network))?;
        let amount = Amount::from_sat(params.amount_sat);
        if amount == Amount::ZERO || amount > Amount::MAX_MONEY {
            return Err(format!("amount must be between 1 sat and {}", Amount::MAX_MONEY));
        }
        let intent = Intent::new(
            Outcome::OutputCreation(FixedPaymentInstructions {
                script_pubkey: address.script_pubkey(),
                amount,
            }),
            // TODO: replace with a real payoff curve.
            PayoffCurve::new(Instant::now(), []),
        );

        // Planning and sending under the lock keeps plans in the order intents arrive.
        let mut queue = self.queue.lock().expect("lock poisoned");
        let id = queue.push(intent);
        let plan = DecisionSpace::new(&*queue)
            .iter()
            .next()
            .expect("a queue holding an intent has a plan");
        self.plans
            .unbounded_send(Input::Plan(plan))
            .expect("the session outlives every request");
        Ok(id)
    }

    fn list_intents(&self) -> Vec<ListedIntent> {
        let queue = self.queue.lock().expect("lock poisoned");
        let mut intents: Vec<ListedIntent> = queue
            .iter()
            .map(|(id, intent)| {
                let Outcome::OutputCreation(payment) = intent.outcome();
                ListedIntent {
                    id,
                    script_pubkey: payment.script_pubkey.to_hex_string(),
                    amount_sat: payment.amount.to_sat(),
                }
            })
            .collect();
        intents.sort_by_key(|intent| intent.id);
        intents
    }
}

fn failure(id: Option<u64>, code: i64, err: impl std::fmt::Display) -> Response {
    Response {
        jsonrpc: TwoPointZero,
        id,
        outcome: fungi_rpc::Outcome::Error(Error {
            code,
            message: err.to_string(),
        }),
    }
}
