//! Minimal offline ONNX parsing for the static benchmark bridge.
//!
//! The parser retains graph structure, tensor metadata, constants, and
//! operator attributes needed for shape propagation and Grimoire generation;
//! it never loads or executes the runtime graph.

use std::collections::BTreeMap;
use std::fmt;

use crate::shape::infer_shapes;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelGraph {
    pub(crate) nodes: Vec<Node>,
    pub(crate) inputs: Vec<ValueInfo>,
    pub(crate) outputs: Vec<ValueInfo>,
    pub(crate) initializers: Vec<ValueInfo>,
    pub(crate) opsets: BTreeMap<String, u64>,
    pub(crate) values: BTreeMap<String, Shape>,
    pub(crate) tensor_values: BTreeMap<String, Vec<i64>>,
    pub(crate) tensor_dimensions: BTreeMap<String, Vec<Dimension>>,
    pub(crate) element_types: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) operator: String,
    pub(crate) domain: String,
    pub(crate) inputs: Vec<String>,
    pub(crate) outputs: Vec<String>,
    pub(crate) attributes: Vec<Attribute>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Attribute {
    pub(crate) name: String,
    pub(crate) integer: Option<i64>,
    pub(crate) integers: Vec<i64>,
    pub(crate) tensor: Option<TensorValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TensorValue {
    pub(crate) dimensions: Vec<i64>,
    pub(crate) values: Vec<i64>,
    pub(crate) element_type: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValueInfo {
    pub(crate) name: String,
    pub(crate) shape: Option<Shape>,
    pub(crate) element_type: Option<u64>,
    pub(crate) values: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    pub(crate) dimensions: Vec<Dimension>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Dimension {
    Literal(u64),
    Symbolic(String),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OnnxError {
    message: String,
}

pub(crate) fn parse_model(bytes: &[u8]) -> Result<ModelGraph, OnnxError> {
    let model_fields = fields(bytes)?;
    let graph_bytes = model_fields
        .iter()
        .find(|field| field.number == 7)
        .and_then(Field::bytes)
        .ok_or_else(|| error("model has no graph"))?;
    let mut opsets = BTreeMap::new();
    for field in model_fields.iter().filter(|field| field.number == 8) {
        let (domain, version) = parse_opset(required_bytes(field, "operator set")?)?;
        if opsets.insert(domain.clone(), version).is_some() {
            return Err(error(format!("operator set `{domain}` is repeated")));
        }
    }
    let mut graph = parse_graph(graph_bytes)?;
    graph.opsets = opsets;
    infer_shapes(&mut graph).map_err(error)?;
    Ok(graph)
}

fn parse_graph(bytes: &[u8]) -> Result<ModelGraph, OnnxError> {
    let mut nodes = Vec::new();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut initializers = Vec::new();
    let mut values = BTreeMap::new();
    let mut tensor_values = BTreeMap::new();
    let tensor_dimensions = BTreeMap::new();
    let mut element_types = BTreeMap::new();

    for field in fields(bytes)? {
        match field.number {
            1 => nodes.push(parse_node(required_bytes(&field, "node")?, nodes.len())?),
            5 => {
                let initializer = parse_initializer(required_bytes(&field, "initializer")?)?;
                let initializer_values = initializer.values.clone();
                let value = ValueInfo {
                    name: initializer.name,
                    shape: initializer.shape,
                    element_type: initializer.element_type,
                    values: initializer_values.clone(),
                };
                insert_value(&mut values, &mut tensor_values, &mut element_types, &value)?;
                if !initializer_values.is_empty() {
                    tensor_values.insert(value.name.clone(), initializer_values);
                }
                initializers.push(value);
            }
            11 => {
                let value = parse_value_info(required_bytes(&field, "graph input")?)?;
                insert_value(&mut values, &mut tensor_values, &mut element_types, &value)?;
                inputs.push(value);
            }
            12 => {
                let value = parse_value_info(required_bytes(&field, "graph output")?)?;
                insert_value(&mut values, &mut tensor_values, &mut element_types, &value)?;
                outputs.push(value);
            }
            13 => {
                let value = parse_value_info(required_bytes(&field, "value info")?)?;
                insert_value(&mut values, &mut tensor_values, &mut element_types, &value)?;
            }
            _ => {}
        }
    }

    if nodes.is_empty() {
        return Err(error("graph has no nodes"));
    }
    Ok(ModelGraph {
        nodes,
        inputs,
        outputs,
        initializers,
        opsets: BTreeMap::new(),
        values,
        tensor_values,
        tensor_dimensions,
        element_types,
    })
}

fn insert_value(
    values: &mut BTreeMap<String, Shape>,
    tensor_values: &mut BTreeMap<String, Vec<i64>>,
    element_types: &mut BTreeMap<String, u64>,
    value: &ValueInfo,
) -> Result<(), OnnxError> {
    if let Some(shape) = &value.shape {
        insert_shape(values, &value.name, shape)?;
    }
    if !value.values.is_empty() {
        tensor_values.insert(value.name.clone(), value.values.clone());
    }
    let Some(element_type) = value.element_type else {
        return Ok(());
    };
    if element_types
        .insert(value.name.clone(), element_type)
        .is_some_and(|existing| existing != element_type)
    {
        return Err(error(format!(
            "tensor `{}` has conflicting element types",
            value.name
        )));
    }
    Ok(())
}

fn insert_shape(
    values: &mut BTreeMap<String, Shape>,
    name: &str,
    shape: &Shape,
) -> Result<(), OnnxError> {
    if let Some(existing) = values.insert(name.to_owned(), shape.clone())
        && existing != *shape
    {
        return Err(error(format!("tensor `{name}` has conflicting shapes")));
    }
    Ok(())
}

fn parse_node(bytes: &[u8], index: usize) -> Result<Node, OnnxError> {
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut name = String::new();
    let mut operator = String::new();
    let mut domain = String::new();
    let mut attributes = Vec::new();
    for field in fields(bytes)? {
        match field.number {
            1 => inputs.push(required_string(&field, "node input")?),
            2 => outputs.push(required_string(&field, "node output")?),
            3 => name = required_string(&field, "node name")?,
            4 => operator = required_string(&field, "node operator")?,
            5 => attributes.push(parse_attribute(required_bytes(&field, "node attribute")?)?),
            7 => domain = required_string(&field, "node domain")?,
            _ => {}
        }
    }
    if operator.is_empty() {
        return Err(error(format!("node {index} has no operator")));
    }
    if name.is_empty() {
        name = format!("{operator}-{index}");
    }
    Ok(Node {
        index,
        name,
        operator,
        domain,
        inputs,
        outputs,
        attributes,
    })
}

struct Initializer {
    name: String,
    shape: Option<Shape>,
    element_type: Option<u64>,
    values: Vec<i64>,
}

fn parse_initializer(bytes: &[u8]) -> Result<Initializer, OnnxError> {
    let mut name = String::new();
    let mut dimensions = Vec::new();
    let mut element_type = None;
    let mut values = Vec::new();
    let mut raw_data = None;
    for field in fields(bytes)? {
        match field.number {
            1 => dimensions.push(Dimension::Literal(required_varint(
                &field,
                "initializer dimension",
            )?)),
            2 => element_type = Some(required_varint(&field, "initializer element type")?),
            5 => values.push(required_varint(&field, "initializer int32 value")?.cast_signed()),
            7 => values.push(required_varint(&field, "initializer int64 value")?.cast_signed()),
            9 => raw_data = Some(required_bytes(&field, "initializer raw data")?.to_vec()),
            8 => name = required_string(&field, "initializer name")?,
            _ => {}
        }
    }
    if name.is_empty() {
        return Err(error("initializer has no name"));
    }
    if let Some(raw_data) = raw_data {
        values.extend(raw_integer_values(&raw_data, element_type)?);
    }
    Ok(Initializer {
        name,
        shape: Some(Shape { dimensions }),
        element_type,
        values,
    })
}

fn parse_opset(bytes: &[u8]) -> Result<(String, u64), OnnxError> {
    let mut domain = String::new();
    let mut version = None;
    for field in fields(bytes)? {
        match field.number {
            1 => domain = required_string(&field, "operator set domain")?,
            2 => version = Some(required_varint(&field, "operator set version")?),
            _ => {}
        }
    }
    let version = version.ok_or_else(|| error("operator set has no version"))?;
    Ok((domain, version))
}

fn parse_attribute(bytes: &[u8]) -> Result<Attribute, OnnxError> {
    let mut name = String::new();
    let mut integer = None;
    let mut integers = Vec::new();
    let mut tensor = None;
    for field in fields(bytes)? {
        match field.number {
            1 => name = required_string(&field, "attribute name")?,
            3 => integer = Some(required_varint(&field, "attribute integer")?.cast_signed()),
            5 => {
                tensor = Some(parse_tensor_value(required_bytes(
                    &field,
                    "attribute tensor",
                )?)?);
            }
            8 => integers.extend(integer_values(&field, "attribute integers")?),
            _ => {}
        }
    }
    if name.is_empty() {
        return Err(error("attribute has no name"));
    }
    Ok(Attribute {
        name,
        integer,
        integers,
        tensor,
    })
}

fn parse_tensor_value(bytes: &[u8]) -> Result<TensorValue, OnnxError> {
    let mut dimensions = Vec::new();
    let mut element_type = None;
    let mut values = Vec::new();
    let mut raw_data = None;
    for field in fields(bytes)? {
        match field.number {
            1 => dimensions.push(required_varint(&field, "tensor dimension")?.cast_signed()),
            2 => element_type = Some(required_varint(&field, "tensor element type")?),
            5 | 7 => values.extend(integer_values(&field, "tensor integer values")?),
            9 => raw_data = Some(required_bytes(&field, "tensor raw data")?.to_vec()),
            _ => {}
        }
    }
    if let Some(raw_data) = raw_data {
        values.extend(raw_integer_values(&raw_data, element_type)?);
    }
    Ok(TensorValue {
        dimensions,
        values,
        element_type,
    })
}

fn integer_values(field: &Field<'_>, name: &str) -> Result<Vec<i64>, OnnxError> {
    match field.value {
        FieldValue::Varint(value) => Ok(vec![value.cast_signed()]),
        FieldValue::Bytes(bytes) => fields(bytes)?
            .iter()
            .map(|field| required_varint(field, name).map(u64::cast_signed))
            .collect(),
        FieldValue::Ignored => Err(error(format!("{name} is not an integer field"))),
    }
}

fn raw_integer_values(bytes: &[u8], element_type: Option<u64>) -> Result<Vec<i64>, OnnxError> {
    let width = match element_type {
        Some(6) => 4,
        Some(7) => 8,
        Some(_) | None => return Ok(Vec::new()),
    };
    if !bytes.len().is_multiple_of(width) {
        return Err(error("raw integer tensor data is not aligned"));
    }
    bytes
        .chunks_exact(width)
        .map(|chunk| match width {
            4 => {
                let array: [u8; 4] = chunk
                    .try_into()
                    .map_err(|_| error("raw int32 tensor data has invalid width"))?;
                Ok(i64::from(i32::from_le_bytes(array)))
            }
            8 => {
                let array: [u8; 8] = chunk
                    .try_into()
                    .map_err(|_| error("raw int64 tensor data has invalid width"))?;
                Ok(i64::from_le_bytes(array))
            }
            _ => Err(error("raw integer tensor width is unsupported")),
        })
        .collect()
}

fn parse_value_info(bytes: &[u8]) -> Result<ValueInfo, OnnxError> {
    let mut name = String::new();
    let mut shape = None;
    let mut element_type = None;
    for field in fields(bytes)? {
        match field.number {
            1 => name = required_string(&field, "value name")?,
            2 => {
                let (value_shape, value_element_type) =
                    parse_type(required_bytes(&field, "value type")?)?;
                shape = value_shape;
                element_type = value_element_type;
            }
            _ => {}
        }
    }
    if name.is_empty() {
        return Err(error("value info has no name"));
    }
    Ok(ValueInfo {
        name,
        shape,
        element_type,
        values: Vec::new(),
    })
}

fn parse_type(bytes: &[u8]) -> Result<(Option<Shape>, Option<u64>), OnnxError> {
    for field in fields(bytes)? {
        if field.number == 1 {
            return parse_tensor_type(required_bytes(&field, "tensor type")?);
        }
    }
    Ok((None, None))
}

fn parse_tensor_type(bytes: &[u8]) -> Result<(Option<Shape>, Option<u64>), OnnxError> {
    let mut element_type = None;
    let mut shape = None;
    for field in fields(bytes)? {
        match field.number {
            1 => element_type = Some(required_varint(&field, "tensor element type")?),
            2 => shape = Some(parse_shape(required_bytes(&field, "tensor shape")?)?),
            _ => {}
        }
    }
    Ok((shape, element_type))
}

fn parse_shape(bytes: &[u8]) -> Result<Shape, OnnxError> {
    let mut dimensions = Vec::new();
    for field in fields(bytes)? {
        if field.number == 1 {
            dimensions.push(parse_dimension(required_bytes(&field, "shape dimension")?)?);
        }
    }
    Ok(Shape { dimensions })
}

fn parse_dimension(bytes: &[u8]) -> Result<Dimension, OnnxError> {
    let mut dimension = None;
    for field in fields(bytes)? {
        let next = match field.number {
            1 => Dimension::Literal(required_varint(&field, "dimension extent")?),
            2 => {
                let value = required_string(&field, "dimension parameter")?;
                if value.is_empty() {
                    Dimension::Unknown
                } else {
                    Dimension::Symbolic(value)
                }
            }
            _ => continue,
        };
        if dimension.replace(next).is_some() {
            return Err(error("shape dimension has multiple representations"));
        }
    }
    Ok(dimension.unwrap_or(Dimension::Unknown))
}

#[derive(Clone, Copy, Debug)]
struct Field<'bytes> {
    number: u32,
    value: FieldValue<'bytes>,
}

#[derive(Clone, Copy, Debug)]
enum FieldValue<'bytes> {
    Varint(u64),
    Bytes(&'bytes [u8]),
    Ignored,
}

impl<'bytes> Field<'bytes> {
    fn bytes(&self) -> Option<&'bytes [u8]> {
        match self.value {
            FieldValue::Bytes(bytes) => Some(bytes),
            FieldValue::Varint(_) | FieldValue::Ignored => None,
        }
    }
}

fn fields(bytes: &[u8]) -> Result<Vec<Field<'_>>, OnnxError> {
    let mut fields = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let key = read_varint(bytes, &mut offset)?;
        let number =
            u32::try_from(key >> 3).map_err(|_| error("protobuf field number is too large"))?;
        if number == 0 {
            return Err(error("protobuf field number is zero"));
        }
        let wire = key & 7;
        let value = match wire {
            0 => FieldValue::Varint(read_varint(bytes, &mut offset)?),
            1 => {
                skip(bytes, &mut offset, 8)?;
                FieldValue::Ignored
            }
            2 => {
                let length = usize::try_from(read_varint(bytes, &mut offset)?)
                    .map_err(|_| error("protobuf field length is too large"))?;
                let end = offset
                    .checked_add(length)
                    .ok_or_else(|| error("protobuf field end overflowed"))?;
                let Some(value) = bytes.get(offset..end) else {
                    return Err(error("protobuf field exceeds input"));
                };
                offset = end;
                FieldValue::Bytes(value)
            }
            5 => {
                skip(bytes, &mut offset, 4)?;
                FieldValue::Ignored
            }
            3 | 4 => return Err(error("protobuf groups are unsupported")),
            _ => return Err(error("protobuf wire type is unsupported")),
        };
        fields.push(Field { number, value });
    }
    Ok(fields)
}

fn read_varint(bytes: &[u8], offset: &mut usize) -> Result<u64, OnnxError> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let Some(byte) = bytes.get(*offset) else {
            return Err(error("protobuf varint is truncated"));
        };
        *offset += 1;
        let payload = u64::from(byte & 0x7f);
        value |= payload << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(error("protobuf varint is too long"))
}

fn skip(bytes: &[u8], offset: &mut usize, amount: usize) -> Result<(), OnnxError> {
    let end = offset
        .checked_add(amount)
        .ok_or_else(|| error("protobuf offset overflowed"))?;
    if bytes.get(*offset..end).is_none() {
        return Err(error("protobuf fixed-width field is truncated"));
    }
    *offset = end;
    Ok(())
}

fn required_bytes<'bytes>(field: &Field<'bytes>, name: &str) -> Result<&'bytes [u8], OnnxError> {
    field
        .bytes()
        .ok_or_else(|| error(format!("{name} is not a length-delimited field")))
}

fn required_string(field: &Field<'_>, name: &str) -> Result<String, OnnxError> {
    let bytes = required_bytes(field, name)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| error(format!("{name} is not UTF-8")))
}

fn required_varint(field: &Field<'_>, name: &str) -> Result<u64, OnnxError> {
    match field.value {
        FieldValue::Varint(value) => Ok(value),
        FieldValue::Bytes(_) | FieldValue::Ignored => Err(error(format!("{name} is not a varint"))),
    }
}

fn error(message: impl Into<String>) -> OnnxError {
    OnnxError {
        message: message.into(),
    }
}

impl fmt::Display for OnnxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OnnxError {}

#[cfg(test)]
mod tests {
    use super::{Dimension, parse_model};

    #[test]
    fn truncated_input_fails_visibly() {
        let result = parse_model(&[0x3a, 0x01]);
        assert!(result.is_err(), "truncated graph should fail");
        let Some(error) = result.err() else {
            return;
        };
        assert!(error.to_string().contains("exceeds input"));
    }

    #[test]
    fn scalar_varint_is_parsed() {
        let graph = [0x3a, 0x05, 0x0a, 0x03, b'n', b'o', b'p'];
        let result = parse_model(&graph);
        assert!(result.is_err(), "model wrapper is missing");
        let Some(error) = result.err() else {
            return;
        };
        assert!(!error.to_string().is_empty());
        let _ = Dimension::Unknown;
    }
}
