use std::time::{SystemTime, UNIX_EPOCH};

use scry::{Handle, Hit, Passage, Provenance};
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
