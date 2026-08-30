use std::time::{SystemTime, UNIX_EPOCH};

use scry::{AddOutcome, AddRefusal, AddReport, AddResult, Handle, Hit, Passage, Provenance};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct HandleWire {
    origin: String,
    start: usize,
    end: usize,
    digest: String,
}

impl HandleWire {
    pub(crate) fn into_handle(self) -> Result<Handle, String> {
        let origin = scry::Origin::parse(&self.origin)
            .ok_or_else(|| "handle origin is not a valid origin".to_owned())?;
        let digest = decode_hex(&self.digest)
            .ok_or_else(|| "handle digest must be 64 hexadecimal characters".to_owned())?;
        Handle::from_parts(origin, self.start, self.end, digest)
            .ok_or_else(|| "handle span has a start after its end".to_owned())
    }

    fn from_handle(handle: &Handle) -> Self {
        let span = handle.chunk().span();
        Self {
            origin: handle.chunk().origin().to_string(),
            start: span.start(),
            end: span.end(),
            digest: encode_hex(handle.digest().as_bytes()),
        }
    }
}

#[derive(Debug, Serialize)]
struct PassageWire {
    handle: HandleWire,
    text: String,
}

#[derive(Debug, Serialize)]
struct HitWire {
    passage: PassageWire,
    score: f32,
}

#[derive(Debug, Serialize)]
struct HitsWire {
    hits: Vec<HitWire>,
}

#[derive(Debug, Serialize)]
struct PassagesWire {
    passages: Vec<PassageWire>,
}

#[derive(Debug, Serialize)]
struct ChunkWire {
    origin: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Serialize)]
struct TimestampWire {
    unix_seconds: u64,
    nanoseconds: u32,
}

#[derive(Debug, Serialize)]
struct ProvenanceWire {
    chunk: ChunkWire,
    fetched_at: TimestampWire,
    ttl_seconds: u64,
}

#[derive(Debug, Serialize)]
struct AddItemWire {
    origin: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AddErrorWire>,
}

#[derive(Debug, Serialize)]
struct AddErrorWire {
    kind: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct AddReportWire {
    status: &'static str,
    items: Vec<AddItemWire>,
}

pub(crate) fn add_report(value: &AddReport) -> Result<(Value, bool), String> {
    let items = value.items().iter().map(add_item).collect();
    add_report_items(items)
}

pub(crate) fn hits(value: &[Hit]) -> Result<Value, String> {
    let hits = value
        .iter()
        .map(|hit| HitWire {
            passage: passage(hit.passage()),
            score: hit.score().get(),
        })
        .collect();
    serde_json::to_value(HitsWire { hits }).map_err(|error| error.to_string())
}

pub(crate) fn passages(value: &[Passage]) -> Result<Value, String> {
    let passages = value.iter().map(passage).collect();
    serde_json::to_value(PassagesWire { passages }).map_err(|error| error.to_string())
}

pub(crate) fn provenance(value: &Provenance) -> Result<Value, String> {
    let chunk = value.chunk();
    let fetched_at = timestamp(value.fetched_at())?;
    serde_json::to_value(ProvenanceWire {
        chunk: ChunkWire {
            origin: chunk.origin().to_string(),
            start: chunk.span().start(),
            end: chunk.span().end(),
        },
        fetched_at,
        ttl_seconds: value.ttl().as_secs(),
    })
    .map_err(|error| error.to_string())
}

fn add_item(value: &AddResult) -> AddItemWire {
    let (status, error) = match value.outcome() {
        AddOutcome::Upserted => ("upserted", None),
        AddOutcome::Refused(refusal) => ("refused", Some(add_error(refusal))),
        AddOutcome::Failed(error) => ("failed", Some(io_error(error))),
        AddOutcome::Uncertain(error) => ("uncertain", Some(io_error(error))),
        AddOutcome::NotAttempted => ("not_attempted", None),
    };
    AddItemWire {
        origin: value.origin().to_string(),
        status,
        error,
    }
}

fn add_report_items(items: Vec<AddItemWire>) -> Result<(Value, bool), String> {
    let halted = items
        .iter()
        .any(|item| matches!(item.status, "failed" | "uncertain" | "not_attempted"));
    let report = AddReportWire {
        status: if halted { "halted" } else { "complete" },
        items,
    };
    serde_json::to_value(report)
        .map(|value| (value, halted))
        .map_err(|error| error.to_string())
}

fn add_error(refusal: &AddRefusal) -> AddErrorWire {
    let kind = match refusal {
        AddRefusal::NotFound(_) => "not_found",
        AddRefusal::NotText(_) => "not_text",
    };
    AddErrorWire {
        kind,
        message: refusal.to_string(),
    }
}

fn io_error(error: &std::io::Error) -> AddErrorWire {
    AddErrorWire {
        kind: "io",
        message: error.to_string(),
    }
}

fn passage(value: &Passage) -> PassageWire {
    PassageWire {
        handle: HandleWire::from_handle(value.handle()),
        text: value.text().as_str().to_owned(),
    }
}

fn timestamp(value: SystemTime) -> Result<TimestampWire, String> {
    let elapsed = value
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("fetched_at is before the Unix epoch: {error}"))?;
    Ok(TimestampWire {
        unix_seconds: elapsed.as_secs(),
        nanoseconds: elapsed.subsec_nanos(),
    })
}

fn encode_hex(bytes: &[u8; 32]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(hex_digit(byte >> 4));
        encoded.push(hex_digit(byte & 0x0f));
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'a',
        11 => 'b',
        12 => 'c',
        13 => 'd',
        14 => 'e',
        15 => 'f',
        _ => '\0',
    }
}

fn decode_hex(input: &str) -> Option<[u8; 32]> {
    if input.len() != 64 {
        return None;
    }
    let mut bytes = [0; 32];
    for (index, pair) in input.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_value(pair.first().copied()?)?;
        let low = hex_value(pair.get(1).copied()?)?;
        *bytes.get_mut(index)? = (high << 4) | low;
    }
    Some(bytes)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{AddErrorWire, AddItemWire, add_report_items};

    fn item(origin: &str, status: &'static str, error: Option<AddErrorWire>) -> AddItemWire {
        AddItemWire {
            origin: origin.to_owned(),
            status,
            error,
        }
    }

    #[test]
    fn a_complete_report_has_no_tool_error_state() {
        let Ok((report, halted)) = add_report_items(vec![
            item("a.md", "upserted", None),
            item(
                "b.md",
                "refused",
                Some(AddErrorWire {
                    kind: "not_found",
                    message: "no bytes at b.md".to_owned(),
                }),
            ),
        ]) else {
            unreachable!()
        };
        assert!(!halted);
        assert_eq!(
            report,
            json!({
                "status": "complete",
                "items": [
                    { "origin": "a.md", "status": "upserted" },
                    {
                        "origin": "b.md",
                        "status": "refused",
                        "error": { "kind": "not_found", "message": "no bytes at b.md" },
                    },
                ],
            })
        );
    }

    #[test]
    fn a_halted_report_retains_failed_and_unattempted_items() {
        let Ok((report, halted)) = add_report_items(vec![
            item("a.md", "upserted", None),
            item(
                "b.md",
                "failed",
                Some(AddErrorWire {
                    kind: "io",
                    message: "processing failed".to_owned(),
                }),
            ),
            item("c.md", "not_attempted", None),
        ]) else {
            unreachable!()
        };
        assert!(halted);
        assert_eq!(
            report,
            json!({
                "status": "halted",
                "items": [
                    { "origin": "a.md", "status": "upserted" },
                    {
                        "origin": "b.md",
                        "status": "failed",
                        "error": { "kind": "io", "message": "processing failed" },
                    },
                    { "origin": "c.md", "status": "not_attempted" },
                ],
            })
        );
    }
}
