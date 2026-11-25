//! JSON-RPC 2.0 between `fungid` and its clients, over a Unix socket with one JSON object per
//! line. Batches and notifications are not supported.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::{self, DeserializeOwned};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Where `fungid` listens unless told otherwise: `$XDG_RUNTIME_DIR/fungid.sock`.
pub fn default_socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("fungid.sock"))
}

/// The `"jsonrpc": "2.0"` member. Deserializing any other version fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TwoPointZero;

impl Serialize for TwoPointZero {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for TwoPointZero {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `String`, not `&str`: under `#[serde(flatten)]` the value arrives buffered, not borrowed.
        match String::deserialize(deserializer)?.as_str() {
            "2.0" => Ok(TwoPointZero),
            other => Err(de::Error::invalid_value(
                de::Unexpected::Str(other),
                &"\"2.0\"",
            )),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: TwoPointZero,
    pub id: u64,
    #[serde(flatten)]
    pub call: Call,
}

/// A method and its params.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Call {
    /// Queue an intent. Returns its id as a `u64`.
    AddIntent(AddIntent),
    /// Returns every queued intent as `Vec<ListedIntent>`.
    ListIntents,
    /// Stop the daemon once the response is written. Returns `null`.
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddIntent {
    /// Checked by the daemon against the network it runs on.
    pub address: String,
    pub amount_sat: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListedIntent {
    pub id: u64,
    /// Hex encoded.
    pub script_pubkey: String,
    pub amount_sat: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: TwoPointZero,
    /// `None` only when the request's id could not be read.
    pub id: Option<u64>,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Result(serde_json::Value),
    Error(Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message} ({code})")]
pub struct Error {
    pub code: i64,
    pub message: String,
}

impl Error {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("daemon closed the connection")]
    Closed,
    #[error("response id {got:?} does not match request id {expected}")]
    UnexpectedId { expected: u64, got: Option<u64> },
    #[error(transparent)]
    Rpc(#[from] Error),
}

/// A blocking connection to `fungid`.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

impl Client {
    /// Calls fail with a timeout error if the daemon takes longer than ten seconds to answer.
    pub fn connect(socket: &Path) -> io::Result<Self> {
        let writer = UnixStream::connect(socket)?;
        // The daemon serves one connection at a time, so a busy or stuck one would block forever.
        writer.set_read_timeout(Some(Duration::from_secs(10)))?;
        let reader = BufReader::new(writer.try_clone()?);
        Ok(Self {
            reader,
            writer,
            next_id: 0,
        })
    }

    pub fn add_intent(&mut self, params: AddIntent) -> Result<u64, ClientError> {
        self.call(Call::AddIntent(params))
    }

    pub fn list_intents(&mut self) -> Result<Vec<ListedIntent>, ClientError> {
        self.call(Call::ListIntents)
    }

    pub fn shutdown(&mut self) -> Result<(), ClientError> {
        self.call(Call::Shutdown)
    }

    fn call<T: DeserializeOwned>(&mut self, call: Call) -> Result<T, ClientError> {
        let id = self.next_id;
        self.next_id += 1;

        let request = Request {
            jsonrpc: TwoPointZero,
            id,
            call,
        };
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;

        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(ClientError::Closed);
        }
        let response: Response = serde_json::from_str(&line)?;
        match response.outcome {
            // An error may carry a null id when the daemon could not read the request.
            Outcome::Error(error) => Err(error.into()),
            Outcome::Result(_) if response.id != Some(id) => Err(ClientError::UnexpectedId {
                expected: id,
                got: response.id,
            }),
            Outcome::Result(value) => Ok(serde_json::from_value(value)?),
        }
    }
}
