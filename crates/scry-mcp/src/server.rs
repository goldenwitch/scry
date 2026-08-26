use std::io::{self, BufRead, Write};
use std::time::Duration;

use scry::{AddRefusal, Count, Embed, HandleRefusal, Query, Slice, Store, Text};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::protocol::{self, Failure, Request};
use crate::wire::HandleWire;

const SERVER_NAME: &str = "scry-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Server {
    store: Store,
    embed: Embed,
    slice: Slice,
    initialized: bool,
}

impl Server {
    pub(crate) fn new(store: Store, embed: Embed, slice: Slice) -> Self {
        Self {
            store,
            embed,
            slice,
            initialized: false,
        }
    }

    pub(crate) fn run<R: BufRead, W: Write>(&mut self, input: R, mut output: W) -> io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let request = match serde_json::from_str::<Request>(&line) {
                Ok(request) => request,
                Err(error) => {
                    protocol::write(
                        &mut output,
                        &protocol::error(
                            Value::Null,
                            Failure::new(protocol::PARSE_ERROR, error.to_string()),
                        ),
                    )?;
                    continue;
                }
            };
            if let Some(response) = self.handle(request) {
                protocol::write(&mut output, &response)?;
            }
        }
        Ok(())
    }

    fn handle(&mut self, request: Request) -> Option<protocol::Response> {
        let id = request.id.clone();
        let result = self.dispatch(request);
        id.map(|id| match result {
            Ok(result) => protocol::success(id, result),
            Err(failure) => protocol::error(id, failure),
        })
    }

    fn dispatch(&mut self, request: Request) -> Result<Value, Failure> {
        if request.jsonrpc != "2.0" {
            return Err(Failure::invalid_request("jsonrpc must be \"2.0\""));
        }
        match request.method.as_str() {
            "initialize" => self.initialize(&request.params),
            "notifications/initialized" | "ping" => Ok(json!({})),
            "tools/list" => {
                self.require_initialized()?;
                Ok(tools())
            }
            "tools/call" => self.call(request.params),
            method => Err(Failure::method_not_found(method)),
        }
    }

    fn initialize(&mut self, params: &Value) -> Result<Value, Failure> {
        if !params.is_object() {
            return Err(Failure::invalid_params(
                "initialize params must be an object",
            ));
        }
        self.initialized = true;
        Ok(json!({
            "protocolVersion": protocol::PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION,
            },
        }))
    }

    fn call(&mut self, params: Value) -> Result<Value, Failure> {
        self.require_initialized()?;
        let params = serde_json::from_value::<CallParams>(params)
            .map_err(|error| Failure::invalid_params(error.to_string()))?;
        match self.call_tool(&params.name, params.arguments) {
            Ok(result) => tool_success(&result),
            Err(error) => Ok(tool_failure(&error)),
        }
    }

    fn require_initialized(&self) -> Result<(), Failure> {
        if self.initialized {
            Ok(())
        } else {
            Err(Failure::new(
                -32_002,
                "initialize must complete before using tools",
            ))
        }
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, ToolError> {
        match name {
            "add" => self.add(arguments),
            "delete" => self.delete(arguments),
            "search" => self.search(arguments),
            "neighbours" => self.neighbours(arguments),
            "provenance" => self.provenance(arguments),
            _ => Err(ToolError::new(
                "unknown_tool",
                format!("unknown tool: {name}"),
            )),
        }
    }

    fn add(&mut self, arguments: Value) -> Result<Value, ToolError> {
        let arguments = parse_arguments::<AddArguments>(arguments)?;
        let origin = scry::Origin::parse(&arguments.origin)
            .ok_or_else(|| ToolError::new("invalid_input", "origin is not valid"))?;
        match self.store.add(
            &mut self.embed,
            &self.slice,
            &origin,
            Duration::from_secs(arguments.ttl_seconds),
        ) {
            Ok(Ok(())) => Ok(json!({})),
            Ok(Err(refusal)) => Err(add_refusal(&refusal)),
            Err(error) => Err(ToolError::new("io", error.to_string())),
        }
    }

    fn delete(&self, arguments: Value) -> Result<Value, ToolError> {
        let arguments = parse_arguments::<OriginArguments>(arguments)?;
        let origin = scry::Origin::parse(&arguments.origin)
            .ok_or_else(|| ToolError::new("invalid_input", "origin is not valid"))?;
        self.store
            .delete(&origin)
            .map(|()| json!({}))
            .map_err(|error| ToolError::new("io", error.to_string()))
    }

    fn search(&mut self, arguments: Value) -> Result<Value, ToolError> {
        let arguments = parse_arguments::<SearchArguments>(arguments)?;
        let count = count(arguments.count)?;
        let query = Query::new(Text::from(arguments.query));
        let hits = self
            .store
            .search(&mut self.embed, &query, count)
            .map_err(|error| ToolError::new("io", error.to_string()))?;
        crate::wire::hits(&hits).map_err(|error| ToolError::new("serialization", error))
    }

    fn neighbours(&self, arguments: Value) -> Result<Value, ToolError> {
        let arguments = parse_arguments::<HandleArguments>(arguments)?;
        let handle = arguments
            .handle
            .into_handle()
            .map_err(|error| ToolError::new("invalid_input", error))?;
        let count = count(arguments.count)?;
        match self.store.neighbours(&handle, count) {
            Ok(Ok(passages)) => crate::wire::passages(&passages)
                .map_err(|error| ToolError::new("serialization", error)),
            Ok(Err(refusal)) => Err(handle_refusal(&refusal)),
            Err(error) => Err(ToolError::new("io", error.to_string())),
        }
    }

    fn provenance(&self, arguments: Value) -> Result<Value, ToolError> {
        let arguments = parse_arguments::<ProvenanceArguments>(arguments)?;
        let handle = arguments
            .handle
            .into_handle()
            .map_err(|error| ToolError::new("invalid_input", error))?;
        match self.store.provenance(&handle) {
            Ok(Ok(provenance)) => crate::wire::provenance(&provenance)
                .map_err(|error| ToolError::new("serialization", error)),
            Ok(Err(refusal)) => Err(handle_refusal(&refusal)),
            Err(error) => Err(ToolError::new("io", error.to_string())),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
    #[serde(rename = "_meta", default)]
    _meta: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddArguments {
    origin: String,
    ttl_seconds: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginArguments {
    origin: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    query: String,
    count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandleArguments {
    handle: HandleWire,
    count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceArguments {
    handle: HandleWire,
}

struct ToolError {
    kind: String,
    message: String,
}

impl ToolError {
    fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

fn parse_arguments<T: DeserializeOwned>(arguments: Value) -> Result<T, ToolError> {
    serde_json::from_value(arguments)
        .map_err(|error| ToolError::new("invalid_input", error.to_string()))
}

fn count(wanted: u64) -> Result<Count, ToolError> {
    let wanted = usize::try_from(wanted)
        .map_err(|_| ToolError::new("invalid_input", "count does not fit this platform"))?;
    Count::new(wanted)
        .ok_or_else(|| ToolError::new("invalid_input", "count must be greater than zero"))
}

fn add_refusal(refusal: &AddRefusal) -> ToolError {
    let kind = match refusal {
        AddRefusal::NotFound(_) => "not_found",
        AddRefusal::NotText(_) => "not_text",
    };
    ToolError::new(kind, refusal.to_string())
}

fn handle_refusal(refusal: &HandleRefusal) -> ToolError {
    let kind = match refusal {
        HandleRefusal::Stale(_) => "stale",
        HandleRefusal::Gone(_) => "gone",
    };
    ToolError::new(kind, refusal.to_string())
}

fn tool_success(result: &Value) -> Result<Value, Failure> {
    let text = serde_json::to_string(result).map_err(|error| Failure::server(error.to_string()))?;
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": result,
        "isError": false,
    }))
}

fn tool_failure(error: &ToolError) -> Value {
    let message = error.message.clone();
    json!({
        "content": [{ "type": "text", "text": message }],
        "structuredContent": {
            "error": {
                "kind": error.kind,
                "message": error.message,
            },
        },
        "isError": true,
    })
}

fn tools() -> Value {
    json!({
        "tools": [
            tool(
                "add",
                "Fetch an origin, index its text, and replace its document in the corpus.",
                &json!({
                    "origin": { "type": "string" },
                    "ttl_seconds": { "type": "integer", "minimum": 0 },
                }),
                &["origin", "ttl_seconds"],
            ),
            tool(
                "delete",
                "Delete the document at an origin.",
                &json!({ "origin": { "type": "string" } }),
                &["origin"],
            ),
            tool(
                "search",
                "Search the corpus and return passages with their handles and scores.",
                &json!({
                    "query": { "type": "string" },
                    "count": { "type": "integer", "minimum": 1 },
                }),
                &["query", "count"],
            ),
            tool(
                "neighbours",
                "Return passages adjacent to a handle's chunk.",
                &json!({
                    "handle": handle_schema(),
                    "count": { "type": "integer", "minimum": 1 },
                }),
                &["handle", "count"],
            ),
            tool(
                "provenance",
                "Verify a handle and return the origin, span, fetch time, and ttl.",
                &json!({ "handle": handle_schema() }),
                &["handle"],
            ),
        ],
    })
}

fn tool(name: &str, description: &str, properties: &Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

fn handle_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "origin": { "type": "string" },
            "start": { "type": "integer", "minimum": 0 },
            "end": { "type": "integer", "minimum": 0 },
            "digest": {
                "type": "string",
                "pattern": "^[0-9a-fA-F]{64}$",
            },
        },
        "required": ["origin", "start", "end", "digest"],
        "additionalProperties": false,
    })
}
