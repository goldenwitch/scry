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
    // Large enough to cross the production passage microbatch boundary while
    // remaining a deterministic local fixture.
    let source_text = (0..900)
        .map(|line| format!("Paragraph {line}: the tide pool holds anemones and limpets."))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&source, source_text)?;
    let other = directory.join("other.md");
    let other_text = (0..40)
        .map(|line| format!("Paragraph {line}: the locomotive raises boiler pressure."))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&other, other_text)?;
    let preflight = directory.join("preflight.md");
    fs::write(&preflight, "this must not be indexed")?;
    let missing = directory.join("missing.md");
    let not_text = directory.join("picture.bin");
    fs::write(&not_text, [0xff, 0xfe, 0x00, 0x80])?;
    let source_path = source
        .to_str()
        .ok_or_else(|| io::Error::other("source path was not UTF-8"))?
        .to_owned();
    let other_path = other
        .to_str()
        .ok_or_else(|| io::Error::other("other path was not UTF-8"))?
        .to_owned();
    let duplicate_path = directory
        .join(".")
        .join("source.md")
        .to_str()
        .ok_or_else(|| io::Error::other("duplicate path was not UTF-8"))?
        .to_owned();
    let preflight_path = preflight
        .to_str()
        .ok_or_else(|| io::Error::other("preflight path was not UTF-8"))?
        .to_owned();
    let missing_path = missing
        .to_str()
        .ok_or_else(|| io::Error::other("missing path was not UTF-8"))?
        .to_owned();
    let not_text_path = not_text
        .to_str()
        .ok_or_else(|| io::Error::other("non-text path was not UTF-8"))?
        .to_owned();
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
                        "origin": &source_path,
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

    let add_tool = tools
        .iter()
        .find(|tool| tool.get("name") == Some(&json!("add")))
        .ok_or_else(|| io::Error::other("tools/list returned no add tool"))?;
    assert_eq!(
        add_tool.get("inputSchema"),
        Some(&json!({
            "type": "object",
            "properties": {
                "origins": {
                    "type": "array",
                    "items": { "type": "string" },
                },
                "ttl_seconds": { "type": "integer", "minimum": 0 },
            },
            "required": ["origins", "ttl_seconds"],
            "additionalProperties": false,
        }))
    );

    let empty = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": { "origins": [], "ttl_seconds": 0 },
            },
        }),
    )?;
    assert_eq!(
        empty.get("result").and_then(|result| result.get("isError")),
        Some(&json!(false))
    );
    assert_eq!(
        structured(&empty)?,
        &json!({ "status": "complete", "items": [] })
    );

    let invalid = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": { "origins": [&preflight_path, ""], "ttl_seconds": 3600 },
            },
        }),
    )?;
    assert_eq!(
        invalid
            .get("result")
            .and_then(|result| result.get("isError")),
        Some(&json!(true))
    );
    assert_eq!(
        structured(&invalid)?
            .get("error")
            .and_then(|error| error.get("kind"))
            .and_then(Value::as_str),
        Some("invalid_input")
    );

    let preflight_search = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "search",
                "arguments": { "query": "must not be indexed", "count": 3 },
            },
        }),
    )?;
    assert!(
        structured(&preflight_search)?
            .get("hits")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    );

    let added = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": {
                    "origins": [&other_path, &duplicate_path, &source_path],
                    "ttl_seconds": 3600,
                },
            },
        }),
    )?;
    assert_eq!(
        added.get("result").and_then(|result| result.get("isError")),
        Some(&json!(false))
    );
    let added_items = structured(&added)?
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("batch add returned no items"))?;
    assert_eq!(added_items.len(), 2);
    let reported_origins = added_items
        .iter()
        .map(|item| {
            item.get("origin")
                .and_then(Value::as_str)
                .ok_or_else(|| io::Error::other("add item returned no origin"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut expected_origins = vec![source_path.as_str(), other_path.as_str()];
    expected_origins.sort_unstable();
    assert_eq!(reported_origins, expected_origins);
    assert!(
        added_items
            .iter()
            .all(|item| item.get("status") == Some(&json!("upserted")))
    );

    let mixed = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": {
                    "origins": [&source_path, &not_text_path, &missing_path],
                    "ttl_seconds": 3600,
                },
            },
        }),
    )?;
    assert_eq!(
        mixed.get("result").and_then(|result| result.get("isError")),
        Some(&json!(false))
    );
    assert_eq!(structured(&mixed)?.get("status"), Some(&json!("complete")));
    let mixed_items = structured(&mixed)?
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("mixed add returned no items"))?;
    assert_eq!(mixed_items.len(), 3);
    let status_for = |origin: &str| {
        mixed_items
            .iter()
            .find(|item| item.get("origin").and_then(Value::as_str) == Some(origin))
            .and_then(|item| item.get("status"))
            .and_then(Value::as_str)
    };
    assert_eq!(status_for(&source_path), Some("upserted"));
    assert_eq!(status_for(&not_text_path), Some("refused"));
    assert_eq!(status_for(&missing_path), Some("refused"));
    assert!(mixed_items.iter().any(|item| {
        item.get("origin").and_then(Value::as_str) == Some(&not_text_path)
            && item.get("error").and_then(|error| error.get("kind")) == Some(&json!("not_text"))
    }));

    let searched = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 9,
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
    assert!(hits.iter().any(|hit| {
        hit.get("passage")
            .and_then(|passage| passage.get("handle"))
            .and_then(|handle| handle.get("origin"))
            .and_then(Value::as_str)
            == Some(&source_path)
    }));
    let other_search = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "search",
                "arguments": { "query": "locomotive boiler pressure", "count": 3 },
            },
        }),
    )?;
    assert!(
        structured(&other_search)?
            .get("hits")
            .and_then(Value::as_array)
            .is_some_and(|hits| {
                hits.iter().any(|hit| {
                    hit.get("passage")
                        .and_then(|passage| passage.get("handle"))
                        .and_then(|handle| handle.get("origin"))
                        .and_then(Value::as_str)
                        == Some(&other_path)
                })
            })
    );

    let traced = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": { "name": "provenance", "arguments": { "handle": handle } },
        }),
    )?;
    assert_eq!(
        structured(&traced)?
            .get("chunk")
            .and_then(|chunk| chunk.get("origin"))
            .and_then(Value::as_str),
        Some(source_path.as_str())
    );

    let widened = request(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0",
            "id": 12,
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
            "id": 13,
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
            "id": 14,
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
