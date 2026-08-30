//! Translate the pinned ONNX graph into a validated Grimoire description.
//!
//! This is static analysis only: it owns graph identity, addressed elements,
//! shapes, groups, and cost expressions, while runtime observations stay in
//! the workload and scry instrumentation modules.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;

use grimoire::{
    Address, Block, Connection, CoreGraph, CostExpression, CostModel, Description,
    ExtensionParameter, ExtensionValue, Group, Layer, LayerInput, Namespace, Port, Projection,
    SchemaUse, SelectItem, Value, Version, prototype_schemas, serialize_description,
    validate_description,
};
use hmac_sha256::Hash;

use crate::onnx::{Dimension, ModelGraph, Node, Shape, parse_model};

pub(crate) const GRIMOIRE_SOURCE: &str = "https://github.com/goldenwitch/grimoire.git";
pub(crate) const GRIMOIRE_REVISION: &str = "bd9920bc1ae79d40383fedcacea3adb87fd98109";
pub(crate) const MODEL_REPOSITORY: &str = "Xenova/bge-small-en-v1.5";
pub(crate) const MODEL_REVISION: &str = "ea104dacec62c0de699686887e3f920caeb4f3e3";
pub(crate) const MODEL_NAME: &str =
    "Xenova/bge-small-en-v1.5@ea104dacec62c0de699686887e3f920caeb4f3e3";
pub(crate) const MODEL_DIMENSION: u64 = 384;
pub(crate) const MODEL_LIMIT: u64 = 512;
pub(crate) const WINDOW: u64 = 256;
pub(crate) const MODEL_FILES: [(&str, u64); 5] = [
    ("model.onnx", 133_093_490),
    ("tokenizer.json", 711_396),
    ("config.json", 683),
    ("special_tokens_map.json", 125),
    ("tokenizer_config.json", 366),
];

const PROTOTYPE_ROOT: &str = "https://github.com/goldenwitch/grimoire/extension";
const ONNX_NAMESPACE: &str = "https://github.com/goldenwitch/scry/benchmark/onnx";
const VERSION: Version = Version::new(1, 0, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BridgeConfig {
    pub(crate) batch_size: u64,
    pub(crate) sequence_length: u64,
}

impl BridgeConfig {
    pub(crate) fn new(batch_size: u64, sequence_length: u64) -> Result<Self, BridgeError> {
        if batch_size == 0 || sequence_length == 0 {
            return Err(error("batch size and sequence length must be positive"));
        }
        Ok(Self {
            batch_size,
            sequence_length,
        })
    }
}

pub(crate) struct StaticModel {
    pub(crate) description: Description,
    pub(crate) canonical: String,
    pub(crate) raw_graph_hash: String,
    pub(crate) canonical_hash: String,
    pub(crate) axes: BTreeMap<Address, u64>,
    pub(crate) macs: CostModel,
    pub(crate) fma_flops: CostModel,
    pub(crate) matmul_group: Address,
    pub(crate) operator_counts: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BridgeError {
    message: String,
}

type BuildParts = (
    Description,
    BTreeMap<Address, u64>,
    CostModel,
    CostModel,
    Address,
    BTreeMap<String, u64>,
);

pub(crate) fn build_static_model(
    model_bytes: &[u8],
    config: BridgeConfig,
) -> Result<StaticModel, BridgeError> {
    let graph = parse_model(model_bytes).map_err(|err| error(err.to_string()))?;
    let raw_graph_hash = hex(&Hash::hash(model_bytes));
    let builder = Builder::new(graph, config)?;
    let (description, axes, macs, fma_flops, matmul_group, operator_counts) = builder.build()?;
    let schemas = prototype_schemas().map_err(|err| error(err.to_string()))?;
    validate_description(&description, &schemas)
        .map_err(|errors| error(format_validation_errors(&errors)))?;
    let canonical = serialize_description(&description).map_err(|err| error(err.to_string()))?;
    let reparsed = grimoire::parse_description(&canonical).map_err(|err| error(err.to_string()))?;
    validate_description(&reparsed, &schemas)
        .map_err(|errors| error(format_validation_errors(&errors)))?;
    let canonical_hash = hex(&Hash::hash(canonical.as_bytes()));
    Ok(StaticModel {
        description,
        canonical,
        raw_graph_hash,
        canonical_hash,
        axes,
        macs,
        fma_flops,
        matmul_group,
        operator_counts,
    })
}

struct Builder {
    graph: ModelGraph,
    description: Description,
    blocks: BTreeMap<Address, Block>,
    connections: BTreeMap<Address, Connection>,
    groups: BTreeMap<Address, Group>,
    axes_by_symbol: BTreeMap<String, Address>,
    axis_values: BTreeMap<Address, u64>,
    tensor_sources: BTreeMap<String, Address>,
    node_input_ports: BTreeMap<(usize, usize), Address>,
    node_addresses: BTreeMap<usize, Address>,
    operator_members: BTreeMap<String, Vec<Address>>,
    matmul_assignments: Vec<(Address, CostExpression)>,
    fma_assignments: Vec<(Address, CostExpression)>,
}

impl Builder {
    fn new(graph: ModelGraph, config: BridgeConfig) -> Result<Self, BridgeError> {
        let description = Description::new(
            address("@scry/bge-small-en-v1-5")?,
            Some("BGE small ONNX graph".to_owned()),
            VERSION,
        );
        let mut axes_by_symbol = BTreeMap::new();
        let mut axis_values = BTreeMap::new();
        let mut symbols = BTreeSet::new();
        for shape in graph.values.values() {
            for dimension in &shape.dimensions {
                if let Dimension::Symbolic(symbol) = dimension {
                    symbols.insert(symbol.clone());
                }
            }
        }
        for (index, symbol) in symbols.into_iter().enumerate() {
            let axis = address(&format!("@scry/bge/axis-{index:03}"))?;
            let extent = match symbol.as_str() {
                "batch_size" => config.batch_size,
                "sequence_length" => config.sequence_length,
                _ => {
                    return Err(error(format!(
                        "symbolic ONNX dimension `{symbol}` has no v1 binding"
                    )));
                }
            };
            axes_by_symbol.insert(symbol, axis.clone());
            axis_values.insert(axis, extent);
        }
        Ok(Self {
            graph,
            description,
            blocks: BTreeMap::new(),
            connections: BTreeMap::new(),
            groups: BTreeMap::new(),
            axes_by_symbol,
            axis_values,
            tensor_sources: BTreeMap::new(),
            node_input_ports: BTreeMap::new(),
            node_addresses: BTreeMap::new(),
            operator_members: BTreeMap::new(),
            matmul_assignments: Vec::new(),
            fma_assignments: Vec::new(),
        })
    }

    fn build(mut self) -> Result<BuildParts, BridgeError> {
        self.add_axes()?;
        self.add_inputs()?;
        self.add_initializers()?;
        self.add_node_blocks()?;
        self.add_node_connections()?;
        self.add_outputs()?;
        let matmul_group = self.add_groups()?;
        let layers = vec![Self::layer("architecture")?, Self::layer("cost")?];
        self.description.core = CoreGraph {
            blocks: self.blocks,
            connections: self.connections,
            groups: self.groups,
        };
        self.description.layers = layers;
        let macs = CostModel::new(self.matmul_assignments).map_err(|err| error(err.to_string()))?;
        let fma_flops =
            CostModel::new(self.fma_assignments).map_err(|err| error(err.to_string()))?;
        let operator_counts = self
            .operator_members
            .iter()
            .map(|(operator, members)| (operator.clone(), members.len() as u64))
            .collect();
        Ok((
            self.description,
            self.axis_values,
            macs,
            fma_flops,
            matmul_group,
            operator_counts,
        ))
    }

    fn add_axes(&mut self) -> Result<(), BridgeError> {
        if self.axes_by_symbol.is_empty() {
            return Ok(());
        }
        let block_address = address("@scry/bge/axes")?;
        let mut block = Block {
            address: block_address,
            name: "Symbolic axes".to_owned(),
            ports: BTreeMap::new(),
            extensions: Vec::new(),
        };
        for (symbol, axis) in &self.axes_by_symbol {
            let port = Port {
                address: axis.clone(),
                label: Some(symbol.clone()),
                extensions: vec![axis_extension(symbol)?],
            };
            block.ports.insert(axis.clone(), port);
        }
        self.add_block(block)
    }

    fn add_inputs(&mut self) -> Result<(), BridgeError> {
        for (index, input) in self.graph.inputs.clone().into_iter().enumerate() {
            let block_address = address(&format!("@scry/bge/input-{index:03}"))?;
            let port_address = address(&format!("@scry/bge/input-{index:03}/output"))?;
            let port = Port {
                address: port_address.clone(),
                label: Some(input.name.clone()),
                extensions: shape_extensions(input.shape.as_ref(), self)?,
            };
            let mut ports = BTreeMap::new();
            ports.insert(port_address.clone(), port);
            self.add_block(Block {
                address: block_address,
                name: format!("Graph input {index}: {}", input.name),
                ports,
                extensions: Vec::new(),
            })?;
            self.add_tensor_source(&input.name, port_address)?;
        }
        Ok(())
    }

    fn add_initializers(&mut self) -> Result<(), BridgeError> {
        for (index, initializer) in self.graph.initializers.clone().into_iter().enumerate() {
            let block_address = address(&format!("@scry/bge/initializer-{index:03}"))?;
            let port_address = address(&format!("@scry/bge/initializer-{index:03}/output"))?;
            let port = Port {
                address: port_address.clone(),
                label: Some(initializer.name.clone()),
                extensions: shape_extensions(initializer.shape.as_ref(), self)?,
            };
            let mut ports = BTreeMap::new();
            ports.insert(port_address.clone(), port);
            self.add_block(Block {
                address: block_address,
                name: format!("Initializer {index}: {}", initializer.name),
                ports,
                extensions: Vec::new(),
            })?;
            self.add_tensor_source(&initializer.name, port_address)?;
        }
        Ok(())
    }

    fn add_node_blocks(&mut self) -> Result<(), BridgeError> {
        for node in self.graph.nodes.clone() {
            let block_address = address(&format!("@scry/bge/node-{0:04}", node.index))?;
            let mut ports = BTreeMap::new();
            for (index, name) in node.inputs.iter().enumerate() {
                let port_address =
                    address(&format!("@scry/bge/node-{}/input-{index:03}", node.index))?;
                ports.insert(
                    port_address.clone(),
                    Port {
                        address: port_address.clone(),
                        label: (!name.is_empty()).then(|| name.clone()),
                        extensions: shape_extensions(self.graph.values.get(name), self)?,
                    },
                );
                self.node_input_ports
                    .insert((node.index, index), port_address);
            }
            for (index, name) in node.outputs.iter().enumerate() {
                let port_address =
                    address(&format!("@scry/bge/node-{}/output-{index:03}", node.index))?;
                ports.insert(
                    port_address.clone(),
                    Port {
                        address: port_address.clone(),
                        label: (!name.is_empty()).then(|| name.clone()),
                        extensions: shape_extensions(self.graph.values.get(name), self)?,
                    },
                );
                if !name.is_empty() {
                    self.add_tensor_source(name, port_address)?;
                }
            }
            let block = Block {
                address: block_address.clone(),
                name: node.name.clone(),
                ports,
                extensions: vec![
                    architecture_extension(&node)?,
                    onnx_extension(&node, &self.graph)?,
                ],
            };
            self.add_block(block)?;
            self.node_addresses
                .insert(node.index, block_address.clone());
            self.operator_members
                .entry(node.operator.clone())
                .or_default()
                .push(block_address.clone());
            if node.operator == "MatMul" {
                let expression = matmul_expression(&node, &self.graph, &self.axes_by_symbol)
                    .map_err(|err| error(format!("node {}: {err}", node.index)))?;
                self.matmul_assignments
                    .push((block_address.clone(), expression.clone()));
                self.fma_assignments.push((
                    block_address,
                    CostExpression::product(vec![CostExpression::constant(2), expression]),
                ));
            }
        }
        Ok(())
    }

    fn add_node_connections(&mut self) -> Result<(), BridgeError> {
        for node in self.graph.nodes.clone() {
            for (index, tensor) in node.inputs.iter().enumerate() {
                if tensor.is_empty() {
                    continue;
                }
                let source = self.tensor_sources.get(tensor).cloned().ok_or_else(|| {
                    error(format!(
                        "node {} input `{tensor}` has no source",
                        node.index
                    ))
                })?;
                let destination = self
                    .node_input_ports
                    .get(&(node.index, index))
                    .cloned()
                    .ok_or_else(|| error("node input port was not generated"))?;
                let relation = address(&format!("@scry/bge/flow/n{}/i{index}", node.index))?;
                self.add_connection(Connection {
                    address: relation,
                    label: None,
                    source,
                    destination,
                    extensions: Vec::new(),
                })?;
            }
        }
        Ok(())
    }

    fn add_outputs(&mut self) -> Result<(), BridgeError> {
        for (index, output) in self.graph.outputs.clone().into_iter().enumerate() {
            let block_address = address(&format!("@scry/bge/output-{index:03}"))?;
            let port_address = address(&format!("@scry/bge/output-{index:03}/input"))?;
            let source = self
                .tensor_sources
                .get(&output.name)
                .cloned()
                .ok_or_else(|| error(format!("graph output `{}` has no source", output.name)))?;
            let port = Port {
                address: port_address.clone(),
                label: Some(output.name.clone()),
                extensions: shape_extensions(output.shape.as_ref(), self)?,
            };
            let mut ports = BTreeMap::new();
            ports.insert(port_address.clone(), port);
            self.add_block(Block {
                address: block_address,
                name: format!("Graph output {index}: {}", output.name),
                ports,
                extensions: Vec::new(),
            })?;
            self.add_connection(Connection {
                address: address(&format!("@scry/bge/output-flow-{index:03}"))?,
                label: None,
                source,
                destination: port_address,
                extensions: Vec::new(),
            })?;
        }
        Ok(())
    }

    fn add_groups(&mut self) -> Result<Address, BridgeError> {
        let mut operator_groups = Vec::new();
        for (index, (operator, members)) in self.operator_members.iter().enumerate() {
            let group_address = address(&format!("@scry/bge/operator-{index:03}"))?;
            self.groups.insert(
                group_address.clone(),
                Group {
                    address: group_address.clone(),
                    label: Some(operator.clone()),
                    members: members.clone(),
                    extensions: Vec::new(),
                },
            );
            operator_groups.push(group_address);
        }
        let matmul_group = address("@scry/bge/cost-matmul")?;
        let matmul_members = self
            .operator_members
            .get("MatMul")
            .cloned()
            .unwrap_or_default();
        self.groups.insert(
            matmul_group.clone(),
            Group {
                address: matmul_group.clone(),
                label: Some("MatMul cost projection".to_owned()),
                members: matmul_members,
                extensions: Vec::new(),
            },
        );
        let operators_group = address("@scry/bge/operators")?;
        self.groups.insert(
            operators_group.clone(),
            Group {
                address: operators_group.clone(),
                label: Some("ONNX operator families".to_owned()),
                members: operator_groups,
                extensions: Vec::new(),
            },
        );
        let mut graph_members = self.blocks.keys().cloned().collect::<Vec<_>>();
        graph_members.extend(self.connections.keys().cloned());
        graph_members.push(operators_group);
        graph_members.push(matmul_group.clone());
        let graph_group = address("@scry/bge/graph")?;
        self.groups.insert(
            graph_group.clone(),
            Group {
                address: graph_group,
                label: Some("BGE static graph".to_owned()),
                members: graph_members,
                extensions: Vec::new(),
            },
        );
        Ok(matmul_group)
    }

    fn layer(name: &str) -> Result<Layer, BridgeError> {
        Ok(Layer {
            name: name.to_owned(),
            inputs: vec![LayerInput::Core],
            projection_language: VERSION,
            schemas: vec![
                schema_use("axes")?,
                schema_use("architecture")?,
                schema_use("shapes")?,
            ],
            projection: Projection {
                select: vec![SelectItem::Use(vec![address("@scry/bge/graph")?])],
                ..Projection::default()
            },
        })
    }

    fn add_block(&mut self, block: Block) -> Result<(), BridgeError> {
        if self.blocks.insert(block.address.clone(), block).is_some() {
            return Err(error("generated block address is duplicated"));
        }
        Ok(())
    }

    fn add_connection(&mut self, connection: Connection) -> Result<(), BridgeError> {
        if self
            .connections
            .insert(connection.address.clone(), connection)
            .is_some()
        {
            return Err(error("generated connection address is duplicated"));
        }
        Ok(())
    }

    fn add_tensor_source(&mut self, name: &str, port: Address) -> Result<(), BridgeError> {
        if self.tensor_sources.insert(name.to_owned(), port).is_some() {
            return Err(error(format!("tensor `{name}` has multiple sources")));
        }
        Ok(())
    }
}

fn shape_extensions(
    shape: Option<&Shape>,
    builder: &Builder,
) -> Result<Vec<ExtensionParameter>, BridgeError> {
    let Some(shape) = shape else {
        return Ok(Vec::new());
    };
    let dimensions = shape
        .dimensions
        .iter()
        .map(|dimension| match dimension {
            Dimension::Literal(value) if *value > 0 => Ok(Value::Tagged {
                tag: "literal".to_owned(),
                value: Box::new(Value::PositiveInteger(*value)),
            }),
            Dimension::Symbolic(symbol) => builder
                .axes_by_symbol
                .get(symbol)
                .cloned()
                .map(Value::AddressReference)
                .map(|value| Value::Tagged {
                    tag: "symbolic".to_owned(),
                    value: Box::new(value),
                })
                .ok_or_else(|| error(format!("shape references unknown axis `{symbol}`"))),
            Dimension::Literal(_) => Err(error("shape contains a zero dimension")),
            Dimension::Unknown => Err(error("shape contains an unknown dimension")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let layout = match shape.dimensions.len() {
        0 => "scalar",
        1 => "vector",
        2 => "sequence",
        3 => "grid",
        _ => "volume",
    };
    let mut fields = BTreeMap::new();
    fields.insert("layout".to_owned(), Value::Enum(layout.to_owned()));
    fields.insert("dimensions".to_owned(), Value::Sequence(dimensions));
    Ok(vec![ExtensionParameter {
        namespace: namespace("shapes")?,
        name: "shape".to_owned(),
        schema: "shapes".to_owned(),
        version: VERSION,
        value: ExtensionValue::Known(Value::Product(fields)),
    }])
}

fn axis_extension(symbol: &str) -> Result<ExtensionParameter, BridgeError> {
    let mut fields = BTreeMap::new();
    fields.insert("name".to_owned(), Value::Text(symbol.to_owned()));
    fields.insert("description".to_owned(), Value::Absent);
    Ok(ExtensionParameter {
        namespace: namespace("axes")?,
        name: "axis".to_owned(),
        schema: "axes".to_owned(),
        version: VERSION,
        value: ExtensionValue::Known(Value::Product(fields)),
    })
}

fn architecture_extension(node: &Node) -> Result<ExtensionParameter, BridgeError> {
    let mut fields = BTreeMap::new();
    fields.insert("family".to_owned(), Value::Text("onnx".to_owned()));
    for name in [
        "parameter_count",
        "width",
        "depth",
        "head_count",
        "mlp_width",
        "activation",
        "position_encoding",
        "attention_regime",
        "interface",
    ] {
        fields.insert(name.to_owned(), Value::Absent);
    }
    fields.insert(
        "operator".to_owned(),
        Value::Present(Box::new(Value::Text(node.operator.clone()))),
    );
    Ok(ExtensionParameter {
        namespace: namespace("architecture")?,
        name: "architecture".to_owned(),
        schema: "architecture".to_owned(),
        version: VERSION,
        value: ExtensionValue::Known(Value::Product(fields)),
    })
}

fn onnx_extension(node: &Node, graph: &ModelGraph) -> Result<ExtensionParameter, BridgeError> {
    let element_type = node
        .outputs
        .iter()
        .chain(node.inputs.iter())
        .find_map(|name| graph.element_types.get(name))
        .map_or_else(|| "unknown".to_owned(), u64::to_string);
    let opset = graph
        .opsets
        .get(&node.domain)
        .or_else(|| graph.opsets.get(""))
        .copied()
        .unwrap_or_default();
    let value = format!(
        "extension \"{ONNX_NAMESPACE}\" node schema node @1.0.0 = {{ index: {}, name: {}, operator: {}, domain: {}, opset: {}, element_type: {} }};",
        node.index,
        quote(&node.name),
        quote(&node.operator),
        quote(&node.domain),
        opset,
        quote(&element_type),
    );
    Ok(ExtensionParameter {
        namespace: Namespace::parse(ONNX_NAMESPACE).map_err(|err| error(err.to_string()))?,
        name: "node".to_owned(),
        schema: "node".to_owned(),
        version: VERSION,
        value: ExtensionValue::Opaque(value.into_bytes()),
    })
}

fn matmul_expression(
    node: &Node,
    graph: &ModelGraph,
    axes: &BTreeMap<String, Address>,
) -> Result<CostExpression, BridgeError> {
    let Some(first) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Err(error(format!(
            "MatMul node {} has no first input",
            node.index
        )));
    };
    let Some(second) = node.inputs.get(1).filter(|name| !name.is_empty()) else {
        return Err(error(format!(
            "MatMul node {} has no second input",
            node.index
        )));
    };
    let left = graph.values.get(first).ok_or_else(|| {
        error(format!(
            "MatMul node {} input `{first}` has no shape",
            node.index
        ))
    })?;
    let right = graph.values.get(second).ok_or_else(|| {
        error(format!(
            "MatMul node {} input `{second}` has no shape",
            node.index
        ))
    })?;
    let (left_prefix, left_m, left_k) = matmul_parts(left, "left")?;
    let (right_prefix, right_k, right_n) = right_parts(right, "right")?;
    if !dimensions_match(&left_k, &right_k) {
        return Err(error(format!(
            "MatMul node {} has incompatible reduction dimensions",
            node.index
        )));
    }
    let mut factors = Vec::new();
    let prefix_count = left_prefix.len().max(right_prefix.len());
    for offset in 0..prefix_count {
        let left_dimension = left_prefix.get(left_prefix.len().wrapping_sub(offset + 1));
        let right_dimension = right_prefix.get(right_prefix.len().wrapping_sub(offset + 1));
        factors.push(broadcast_expression(left_dimension, right_dimension, axes)?);
    }
    factors.reverse();
    factors.push(dimension_expression(&left_m, axes)?);
    factors.push(dimension_expression(&left_k, axes)?);
    factors.push(dimension_expression(&right_n, axes)?);
    Ok(CostExpression::product(factors))
}

fn matmul_parts(
    shape: &Shape,
    side: &str,
) -> Result<(Vec<Dimension>, Dimension, Dimension), BridgeError> {
    if shape.dimensions.is_empty() {
        return Err(error(format!("MatMul {side} input is scalar")));
    }
    if shape.dimensions.len() == 1 {
        let dimension = shape
            .dimensions
            .first()
            .ok_or_else(|| error("one-dimensional shape has no dimension"))?;
        return Ok((Vec::new(), Dimension::Literal(1), dimension.clone()));
    }
    let split = shape.dimensions.len() - 2;
    let (prefix, matrix) = shape.dimensions.split_at(split);
    let m = matrix
        .first()
        .ok_or_else(|| error(format!("MatMul {side} input has no rows")))?;
    let k = matrix
        .get(1)
        .ok_or_else(|| error(format!("MatMul {side} input has no reduction dimension")))?;
    Ok((prefix.to_vec(), m.clone(), k.clone()))
}

fn right_parts(
    shape: &Shape,
    side: &str,
) -> Result<(Vec<Dimension>, Dimension, Dimension), BridgeError> {
    if shape.dimensions.is_empty() {
        return Err(error(format!("MatMul {side} input is scalar")));
    }
    if shape.dimensions.len() == 1 {
        let dimension = shape
            .dimensions
            .first()
            .ok_or_else(|| error("one-dimensional shape has no dimension"))?;
        return Ok((Vec::new(), dimension.clone(), Dimension::Literal(1)));
    }
    let split = shape.dimensions.len() - 2;
    let (prefix, matrix) = shape.dimensions.split_at(split);
    let k = matrix
        .first()
        .ok_or_else(|| error(format!("MatMul {side} input has no reduction dimension")))?;
    let n = matrix
        .get(1)
        .ok_or_else(|| error(format!("MatMul {side} input has no columns")))?;
    Ok((prefix.to_vec(), k.clone(), n.clone()))
}

fn dimensions_match(left: &Dimension, right: &Dimension) -> bool {
    match (left, right) {
        (Dimension::Literal(left), Dimension::Literal(right)) => left == right,
        (Dimension::Symbolic(left), Dimension::Symbolic(right)) => left == right,
        _ => false,
    }
}

fn broadcast_expression(
    left: Option<&Dimension>,
    right: Option<&Dimension>,
    axes: &BTreeMap<String, Address>,
) -> Result<CostExpression, BridgeError> {
    match (left, right) {
        (None, None) => Ok(CostExpression::constant(1)),
        (Some(dimension), None) | (None, Some(dimension)) => dimension_expression(dimension, axes),
        (Some(Dimension::Literal(1)), other) | (other, Some(Dimension::Literal(1))) => {
            dimension_expression(other.unwrap_or(&Dimension::Literal(1)), axes)
        }
        (Some(left), Some(right)) if dimensions_match(left, right) => {
            dimension_expression(left, axes)
        }
        _ => Err(error(
            "MatMul batch dimensions are not broadcast-compatible",
        )),
    }
}

fn dimension_expression(
    dimension: &Dimension,
    axes: &BTreeMap<String, Address>,
) -> Result<CostExpression, BridgeError> {
    match dimension {
        Dimension::Literal(value) if *value > 0 => Ok(CostExpression::constant(*value)),
        Dimension::Symbolic(symbol) => axes
            .get(symbol)
            .cloned()
            .map(CostExpression::axis)
            .ok_or_else(|| error(format!("shape references unknown axis `{symbol}`"))),
        Dimension::Literal(_) => Err(error("cost dimension is zero")),
        Dimension::Unknown => Err(error("cost dimension is unknown")),
    }
}

fn schema_use(name: &str) -> Result<SchemaUse, BridgeError> {
    Ok(SchemaUse {
        namespace: namespace(name)?,
        name: name.to_owned(),
        version: VERSION,
    })
}

fn namespace(name: &str) -> Result<Namespace, BridgeError> {
    Namespace::parse(&format!("{PROTOTYPE_ROOT}/{name}")).map_err(|err| error(err.to_string()))
}

fn address(value: &str) -> Result<Address, BridgeError> {
    Address::parse(value).map_err(|err| error(err.to_string()))
}

fn format_validation_errors(errors: &[grimoire::ValidationError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(quoted, "\\u{:04x}", character as u32);
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn hex(bytes: &[u8; 32]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

fn error(message: impl Into<String>) -> BridgeError {
    BridgeError {
        message: message.into(),
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BridgeError {}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn opaque_metadata_quotes_control_text() {
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }
}
