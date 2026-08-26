//! End-to-end validation through the MCP stdio boundary.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

#[test]
#[allow(clippy::too_many_lines)]
fn mcp_drives_the_library_from_add_through_gone() -> Result<(), Box<dyn std::error::Error>> {
    let directory = scratch_directory()?;
    let source = directory.join("source.md");
    let source_text = (0..40)
        .map(|line| format!("Paragraph {line}: the tide pool holds anemones and limpets."))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&source, source_text)?;
    let source_path = source
        .to_str()
        .ok_or_else(|| io::Error::other("source path was not UTF-8"))?;
    let store_path = directory.join("corpus.redb");
    let cache_path = std::env::temp_dir().join("scry-model-cache");

    let store_path = store_path
        .to_str()
        .ok_or_else(|| io::Error::other("store path was not UTF-8"))?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_scry-mcp"))
        .args(["--store", store_path, "--cache"])
        .arg(&cache_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("MCP stdin was not available"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("MCP stdout was not available"))?;
    let mut output = BufReader::new(stdout);

    let initialized = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "scry-mcp-e2e", "version": "0.1.0" },
            },
        }),
    )?;
    assert_eq!(
        initialized
            .get("result")
            .and_then(|result| result.get("protocolVersion"))
            .and_then(Value::as_str),
        Some("2025-06-18")
    );
    notification(
        &mut input,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        }),
    )?;

    let malformed_handle = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "provenance",
                "arguments": {
                    "handle": {
                        "origin": source_path,
                        "start": 2,
                        "end": 1,
                        "digest": "0000000000000000000000000000000000000000000000000000000000000000",
                    },
                },
            },
        }),
    )?;
    assert_eq!(
        malformed_handle
            .get("result")
            .and_then(|result| result.get("isError")),
        Some(&json!(true))
    );
    assert_eq!(
        structured(&malformed_handle)?
            .get("error")
            .and_then(|error| error.get("kind"))
            .and_then(Value::as_str),
        Some("invalid_input")
    );

    let listed = request(
        &mut input,
        &mut output,
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
    )?;
    let tools = listed
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("tools/list returned no tools"))?;
    let names = tools
        .iter()
        .filter_map(|tool| tool.get("name"))
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["add", "delete", "search", "neighbours", "provenance"]
    );

    let added = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": { "origin": source_path, "ttl_seconds": 3600 },
            },
        }),
    )?;
    assert_eq!(structured(&added)?, &json!({}));

    let searched = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "search",
                "arguments": { "query": "what lives in the tide pool", "count": 3 },
            },
        }),
    )?;
    let hits = structured(&searched)?
        .get("hits")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("search returned no hit list"))?;
    let handle = hits
        .first()
        .and_then(|hit| hit.get("passage"))
        .and_then(|passage| passage.get("handle"))
        .cloned()
        .ok_or_else(|| io::Error::other("search returned no handle"))?;

    let traced = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": { "name": "provenance", "arguments": { "handle": handle } },
        }),
    )?;
    assert_eq!(
        structured(&traced)?
            .get("chunk")
            .and_then(|chunk| chunk.get("origin"))
            .and_then(Value::as_str),
        Some(source_path)
    );

    let widened = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "neighbours",
                "arguments": { "handle": handle, "count": 2 },
            },
        }),
    )?;
    let passages = structured(&widened)?
        .get("passages")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("neighbours returned no passage list"))?;
    assert!(!passages.is_empty());

    let deleted = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": { "name": "delete", "arguments": { "origin": source_path } },
        }),
    )?;
    assert_eq!(structured(&deleted)?, &json!({}));

    let gone = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": { "name": "provenance", "arguments": { "handle": handle } },
        }),
    )?;
    assert_eq!(
        gone.get("result").and_then(|result| result.get("isError")),
        Some(&json!(true))
    );
    assert_eq!(
        structured(&gone)?
            .get("error")
            .and_then(|error| error.get("kind"))
            .and_then(Value::as_str),
        Some("gone")
    );

    drop(input);
    let status = child.wait()?;
    assert!(status.success());
    fs::remove_dir_all(directory)?;
    Ok(())
}

fn scratch_directory() -> io::Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("scry-mcp-e2e-{}-{timestamp}", std::process::id()));
    fs::create_dir_all(&directory)?;
    Ok(directory)
}

#[allow(clippy::needless_pass_by_value)]
fn request<W: Write, R: BufRead>(
    input: &mut W,
    output: &mut R,
    message: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let encoded = serde_json::to_string(&message)?;
    writeln!(input, "{encoded}")?;
    input.flush()?;
    let mut line = String::new();
    output.read_line(&mut line)?;
    if line.is_empty() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "MCP closed stdout").into());
    }
    Ok(serde_json::from_str(&line)?)
}

#[allow(clippy::needless_pass_by_value)]
fn notification<W: Write>(input: &mut W, message: Value) -> io::Result<()> {
    let encoded = serde_json::to_string(&message).map_err(io::Error::other)?;
    writeln!(input, "{encoded}")?;
    input.flush()
}

fn structured(response: &Value) -> Result<&Value, Box<dyn std::error::Error>> {
    response
        .get("result")
        .and_then(|result| result.get("structuredContent"))
        .ok_or_else(|| io::Error::other("MCP response had no structured content").into())
}
