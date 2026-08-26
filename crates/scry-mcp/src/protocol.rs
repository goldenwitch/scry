use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const PROTOCOL_VERSION: &str = "2025-06-18";
pub(crate) const PARSE_ERROR: i64 = -32_700;
pub(crate) const INVALID_REQUEST: i64 = -32_600;
pub(crate) const METHOD_NOT_FOUND: i64 = -32_601;
pub(crate) const INVALID_PARAMS: i64 = -32_602;
pub(crate) const SERVER_ERROR: i64 = -32_603;

#[derive(Debug, Deserialize)]
pub(crate) struct Request {
    pub(crate) jsonrpc: String,
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) params: Value,
    #[serde(default)]
    pub(crate) id: Option<Value>,
}

#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) code: i64,
    pub(crate) message: String,
}

impl Failure {
    #[must_use]
    pub(crate) fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message)
    }

    #[must_use]
    pub(crate) fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    #[must_use]
    pub(crate) fn method_not_found(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("unknown method: {method}"))
    }

    #[must_use]
    pub(crate) fn server(message: impl Into<String>) -> Self {
        Self::new(SERVER_ERROR, message)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

pub(crate) fn success(id: Value, result: Value) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

pub(crate) fn error(id: Value, failure: Failure) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code: failure.code,
            message: failure.message,
        }),
    }
}

pub(crate) fn write<W: Write>(writer: &mut W, response: &Response) -> io::Result<()> {
    let bytes = serde_json::to_vec(response).map_err(io::Error::other)?;
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
