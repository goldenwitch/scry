//! Shape propagation for the offline ONNX benchmark analysis.
//!
//! Shape facts are resolved before the bridge writes Grimoire elements, so a
//! cost expression cannot silently substitute an unknown extent.

#![allow(clippy::type_complexity)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::too_many_lines)]

use crate::onnx::{Dimension, ModelGraph, Node, Shape};

pub(crate) fn infer_shapes(graph: &mut ModelGraph) -> Result<(), String> {
    let passes = graph.nodes.len().max(1);
    for _ in 0..passes {
        let mut changed = false;
        for node in graph.nodes.clone() {
            let inferred = infer_node(&node, graph)?;
            for (output, shape) in node.outputs.iter().zip(inferred.shapes.iter()) {
                if !output.is_empty()
                    && let Some(shape) = shape
                {
                    changed |= set_shape(graph, output, shape.clone())?;
                }
            }
            if let Some(values) = inferred.values.as_ref()
                && let Some(output) = node.outputs.first().filter(|output| !output.is_empty())
            {
                changed |= set_values(graph, output, values.clone())?;
            }
            if let Some(element_type) = inferred.element_type {
                for output in &node.outputs {
                    if !output.is_empty() {
                        changed |= set_element_type(graph, output, element_type)?;
                    }
                }
            }
            if let Some(dimensions) = tensor_dimensions(&node, graph)?
                && let Some(output) = node.outputs.first().filter(|output| !output.is_empty())
            {
                changed |= set_tensor_dimensions(graph, output, dimensions)?;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

struct Inferred {
    shapes: Vec<Option<Shape>>,
    values: Option<Vec<i64>>,
    element_type: Option<u64>,
}

fn infer_node(node: &Node, graph: &ModelGraph) -> Result<Inferred, String> {
    let shape = match node.operator.as_str() {
        "Add" | "Div" | "Mul" | "Pow" | "Sub" | "Where" => broadcast_inputs(node, graph),
        "Cast" | "Erf" | "Identity" | "Relu" | "Sigmoid" | "Softmax" | "Sqrt" | "Tanh" => {
            first_input_shape(node, graph)
        }
        "MatMul" => matmul_shape(node, graph),
        "Transpose" => transpose_shape(node, graph),
        "Reshape" => reshape_shape(node, graph),
        "Unsqueeze" => unsqueeze_shape(node, graph),
        "Squeeze" => squeeze_shape(node, graph),
        "Concat" => concat_shape(node, graph),
        "Gather" => gather_shape(node, graph),
        "Slice" => slice_shape(node, graph),
        "ReduceMean" => reduce_shape(node, graph),
        "Shape" => shape_shape(node, graph),
        "Constant" => constant_shape(node),
        "Flatten" => flatten_shape(node, graph),
        "Split" => split_shapes(node, graph),
        _ => Ok((vec![None; node.outputs.len()], None, None)),
    };
    match shape {
        Ok((shapes, values, element_type)) => Ok(Inferred {
            shapes,
            values,
            element_type,
        }),
        Err(message) => Err(format!(
            "node {} ({}): {message}",
            node.index, node.operator
        )),
    }
}

fn first_input_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let shape = node
        .inputs
        .iter()
        .find_map(|input| graph.values.get(input).cloned());
    let element_type = node
        .inputs
        .iter()
        .find_map(|input| graph.element_types.get(input).copied());
    Ok((vec![shape], None, element_type))
}

fn broadcast_inputs(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let mut result = None;
    let mut element_type = None;
    for input in &node.inputs {
        if input.is_empty() {
            continue;
        }
        let Some(shape) = graph.values.get(input) else {
            return Ok((vec![None; node.outputs.len()], None, None));
        };
        result = Some(match result {
            Some(existing) => broadcast_shapes(&existing, shape)?,
            None => shape.clone(),
        });
        element_type = element_type.or_else(|| graph.element_types.get(input).copied());
    }
    Ok((vec![result; node.outputs.len()], None, element_type))
}

fn matmul_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(left_name) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(right_name) = node.inputs.get(1).filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let (Some(left), Some(right)) = (graph.values.get(left_name), graph.values.get(right_name))
    else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let left_rank = left.dimensions.len();
    let right_rank = right.dimensions.len();
    if left_rank == 0 || right_rank == 0 {
        return Ok((vec![None; node.outputs.len()], None, None));
    }
    let left_prefix = left
        .dimensions
        .get(..left_rank.saturating_sub(2))
        .ok_or("left MatMul prefix is out of range")?
        .to_vec();
    let right_prefix = right
        .dimensions
        .get(..right_rank.saturating_sub(2))
        .ok_or("right MatMul prefix is out of range")?
        .to_vec();
    let left_vector = left_rank == 1;
    let right_vector = right_rank == 1;
    let left_k = left
        .dimensions
        .last()
        .ok_or("left MatMul shape has no reduction dimension")?;
    let right_k = if right_vector {
        right
            .dimensions
            .last()
            .ok_or("right MatMul shape has no reduction dimension")?
    } else {
        right
            .dimensions
            .get(right_rank - 2)
            .ok_or("right MatMul shape has no reduction dimension")?
    };
    if !dimensions_compatible(left_k, right_k) {
        return Err("reduction dimensions are incompatible".to_owned());
    }
    let left_m = if left_vector {
        Dimension::Literal(1)
    } else {
        left.dimensions
            .get(left_rank - 2)
            .cloned()
            .ok_or("left MatMul shape has no rows")?
    };
    let right_n = if right_vector {
        Dimension::Literal(1)
    } else {
        right
            .dimensions
            .last()
            .cloned()
            .ok_or("right MatMul shape has no columns")?
    };
    let prefix = broadcast_dimension_lists(&left_prefix, &right_prefix)?;
    let mut dimensions = prefix;
    if !left_vector {
        dimensions.push(left_m);
    }
    if !right_vector {
        dimensions.push(right_n);
    }
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(left_name).copied(),
    ))
}

fn transpose_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(shape) = graph.values.get(input) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let permutation = attribute_ints(node, "perm").unwrap_or_else(|| {
        (0..i64::try_from(shape.dimensions.len()).unwrap_or(i64::MAX))
            .rev()
            .collect()
    });
    let mut dimensions = Vec::with_capacity(permutation.len());
    for index in permutation {
        let index = normalize_index(index, shape.dimensions.len())?;
        let Some(dimension) = shape.dimensions.get(index) else {
            return Err("transpose permutation is out of range".to_owned());
        };
        dimensions.push(dimension.clone());
    }
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(input).copied(),
    ))
}

fn reshape_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(input_shape) = graph.values.get(input) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let Some(shape_input) = node.inputs.get(1).filter(|name| !name.is_empty()) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let Some(target) = graph.tensor_values.get(shape_input) else {
        if let Some(target) = graph.tensor_dimensions.get(shape_input) {
            return Ok((
                vec![Some(Shape {
                    dimensions: target.clone(),
                })],
                None,
                graph.element_types.get(input).copied(),
            ));
        }
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let mut dimensions = Vec::new();
    let mut inferred = None;
    let mut known_product = 1u64;
    for (index, value) in target.iter().copied().enumerate() {
        match value {
            -1 => {
                if inferred.replace(index).is_some() {
                    return Err("reshape has more than one inferred dimension".to_owned());
                }
                dimensions.push(Dimension::Unknown);
            }
            0 => {
                let dimension = input_shape
                    .dimensions
                    .get(index)
                    .cloned()
                    .ok_or("reshape zero dimension is out of range")?;
                if let Dimension::Literal(value) = dimension {
                    known_product = known_product
                        .checked_mul(value)
                        .ok_or("reshape size overflowed")?;
                }
                dimensions.push(dimension);
            }
            value if value > 0 => {
                let value = u64::try_from(value).map_err(|_| "reshape dimension is too large")?;
                known_product = known_product
                    .checked_mul(value)
                    .ok_or("reshape size overflowed")?;
                dimensions.push(Dimension::Literal(value));
            }
            _ => return Err("reshape dimension is negative".to_owned()),
        }
    }
    if let Some(index) = inferred {
        let input_size = shape_product(input_shape);
        if let Some(input_size) = input_size {
            if known_product == 0 || !input_size.is_multiple_of(known_product) {
                return Err("reshape inferred dimension is not integral".to_owned());
            }
            let value = input_size / known_product;
            if value == 0 {
                return Err("reshape inferred dimension is zero".to_owned());
            }
            if let Some(dimension) = dimensions.get_mut(index) {
                *dimension = Dimension::Literal(value);
            }
        }
    }
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(input).copied(),
    ))
}

fn unsqueeze_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(shape) = graph.values.get(input) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let axes = node
        .inputs
        .get(1)
        .and_then(|name| graph.tensor_values.get(name))
        .cloned()
        .or_else(|| attribute_ints(node, "axes"))
        .unwrap_or_default();
    let rank = shape.dimensions.len() + axes.len();
    let mut positions = axes
        .into_iter()
        .map(|axis| normalize_insert_index(axis, rank))
        .collect::<Result<Vec<_>, _>>()?;
    positions.sort_unstable();
    let mut dimensions = shape.dimensions.clone();
    for position in positions {
        dimensions.insert(position, Dimension::Literal(1));
    }
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(input).copied(),
    ))
}

fn squeeze_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(shape) = graph.values.get(input) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let axes = node
        .inputs
        .get(1)
        .and_then(|name| graph.tensor_values.get(name))
        .cloned()
        .or_else(|| attribute_ints(node, "axes"));
    let mut dimensions = Vec::new();
    for (index, dimension) in shape.dimensions.iter().enumerate() {
        let selected = axes.as_ref().is_none_or(|axes| {
            axes.iter()
                .any(|axis| normalize_index(*axis, shape.dimensions.len()).ok() == Some(index))
        });
        if selected {
            if !matches!(dimension, Dimension::Literal(1)) {
                return Err("squeeze axis does not have extent one".to_owned());
            }
        } else {
            dimensions.push(dimension.clone());
        }
    }
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(input).copied(),
    ))
}

fn concat_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let shapes = node
        .inputs
        .iter()
        .filter(|input| !input.is_empty())
        .map(|input| graph.values.get(input).cloned())
        .collect::<Option<Vec<_>>>();
    let Some(shapes) = shapes else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(first) = shapes.first() else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let axis = attribute_int(node, "axis").unwrap_or(0);
    let axis = normalize_index(axis, first.dimensions.len())?;
    let mut dimensions = first.dimensions.clone();
    for shape in shapes.iter().skip(1) {
        if shape.dimensions.len() != dimensions.len() {
            return Err("concat ranks differ".to_owned());
        }
        for (index, dimension) in dimensions.iter_mut().enumerate() {
            let Some(other) = shape.dimensions.get(index) else {
                return Err("concat input dimension is out of range".to_owned());
            };
            if index == axis {
                *dimension = sum_dimension(dimension, other);
            } else if !dimensions_compatible(dimension, other) {
                return Err("concat dimensions differ".to_owned());
            }
        }
    }
    Ok((vec![Some(Shape { dimensions })], None, None))
}

fn gather_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let (Some(data_name), Some(indices_name)) = (
        node.inputs.first().filter(|name| !name.is_empty()),
        node.inputs.get(1).filter(|name| !name.is_empty()),
    ) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let (Some(data), Some(indices)) = (graph.values.get(data_name), graph.values.get(indices_name))
    else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(data_name).copied(),
        ));
    };
    let axis = normalize_index(
        attribute_int(node, "axis").unwrap_or(0),
        data.dimensions.len(),
    )?;
    let mut dimensions = data
        .dimensions
        .get(..axis)
        .ok_or("gather prefix is out of range")?
        .to_vec();
    dimensions.extend(indices.dimensions.iter().cloned());
    dimensions.extend(
        data.dimensions
            .get(axis + 1..)
            .ok_or("gather suffix is out of range")?
            .iter()
            .cloned(),
    );
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(data_name).copied(),
    ))
}

fn slice_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(shape) = graph.values.get(input) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let Some(starts_name) = node.inputs.get(1).filter(|name| !name.is_empty()) else {
        return Ok((
            vec![Some(shape.clone()); node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let Some(ends_name) = node.inputs.get(2).filter(|name| !name.is_empty()) else {
        return Ok((
            vec![Some(shape.clone()); node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let Some(starts) = graph.tensor_values.get(starts_name) else {
        return Ok((
            vec![Some(shape.clone()); node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let ends = graph.tensor_values.get(ends_name);
    let axes = node
        .inputs
        .get(3)
        .and_then(|name| graph.tensor_values.get(name))
        .cloned()
        .unwrap_or_else(|| (0..i64::try_from(starts.len()).unwrap_or(i64::MAX)).collect());
    let steps = node
        .inputs
        .get(4)
        .and_then(|name| graph.tensor_values.get(name))
        .cloned()
        .unwrap_or_else(|| vec![1; starts.len()]);
    let mut dimensions = shape.dimensions.clone();
    for (index, axis) in axes.iter().copied().enumerate() {
        let axis_position = normalize_index(axis, dimensions.len())?;
        let Some(Dimension::Literal(extent)) = dimensions.get(axis_position) else {
            continue;
        };
        let start = *starts.get(index).ok_or("slice start is missing")?;
        let step = *steps.get(index).ok_or("slice step is missing")?;
        if step <= 0 {
            return Err("slice step must be positive in v1".to_owned());
        }
        let dynamic_end = ends.is_none();
        let end_dimension = graph
            .tensor_dimensions
            .get(ends_name)
            .and_then(|ends| ends.get(index));
        if dynamic_end && let Some(Dimension::Symbolic(symbol)) = end_dimension {
            let replacement = if *extent == 1 {
                Dimension::Literal(1)
            } else if starts.get(index).copied() == Some(0) && step == 1 {
                Dimension::Symbolic(symbol.clone())
            } else {
                Dimension::Unknown
            };
            if let Some(dimension) = dimensions.get_mut(axis_position) {
                *dimension = replacement;
            }
            continue;
        }
        let end = ends
            .and_then(|ends| ends.get(index).copied())
            .or_else(|| {
                end_dimension.and_then(|end| match end {
                    Dimension::Literal(value) => i64::try_from(*value).ok(),
                    Dimension::Symbolic(_) | Dimension::Unknown => None,
                })
            })
            .unwrap_or(i64::MAX);
        let extent = i64::try_from(*extent).map_err(|_| "slice extent is too large")?;
        let start = start.clamp(0, extent);
        let end = end.clamp(0, extent);
        let length = if end > start {
            (end - start + step - 1) / step
        } else {
            0
        };
        if let Some(dimension) = dimensions.get_mut(axis_position) {
            *dimension =
                Dimension::Literal(u64::try_from(length).map_err(|_| "slice length is negative")?);
        }
    }
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(input).copied(),
    ))
}

fn reduce_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(shape) = graph.values.get(input) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let axes = node
        .inputs
        .get(1)
        .and_then(|name| graph.tensor_values.get(name))
        .cloned()
        .or_else(|| attribute_ints(node, "axes"));
    let Some(axes) = axes else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let keepdims = attribute_int(node, "keepdims").unwrap_or(1) != 0;
    let mut dimensions = Vec::new();
    for (index, dimension) in shape.dimensions.iter().enumerate() {
        if axes
            .iter()
            .any(|axis| normalize_index(*axis, shape.dimensions.len()).ok() == Some(index))
        {
            if keepdims {
                dimensions.push(Dimension::Literal(1));
            }
        } else {
            dimensions.push(dimension.clone());
        }
    }
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(input).copied(),
    ))
}
fn shape_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(shape) = graph.values.get(input) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let values = shape
        .dimensions
        .iter()
        .map(|dimension| match dimension {
            Dimension::Literal(value) => i64::try_from(*value).ok(),
            Dimension::Symbolic(_) | Dimension::Unknown => None,
        })
        .collect::<Option<Vec<_>>>();
    Ok((
        vec![Some(Shape {
            dimensions: vec![Dimension::Literal(shape.dimensions.len() as u64)],
        })],
        values,
        Some(7),
    ))
}

fn constant_shape(
    node: &Node,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(attribute) = node
        .attributes
        .iter()
        .find(|attribute| attribute.name == "value")
    else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(tensor) = &attribute.tensor else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let dimensions = tensor
        .dimensions
        .iter()
        .map(|dimension| {
            if *dimension > 0 {
                Ok(Dimension::Literal(
                    u64::try_from(*dimension).map_err(|_| "constant shape is too large")?,
                ))
            } else {
                Err("constant shape has a non-positive dimension".to_owned())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        vec![Some(Shape { dimensions })],
        (!tensor.values.is_empty()).then(|| tensor.values.clone()),
        tensor.element_type,
    ))
}

fn flatten_shape(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(shape) = graph.values.get(input) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    let axis = normalize_insert_index(
        attribute_int(node, "axis").unwrap_or(1),
        shape.dimensions.len(),
    )?;
    let first = shape_product_of(
        shape
            .dimensions
            .get(..axis)
            .ok_or("flatten prefix is out of range")?,
    );
    let second = shape_product_of(
        shape
            .dimensions
            .get(axis..)
            .ok_or("flatten suffix is out of range")?,
    );
    let dimensions = vec![
        first.map_or(Dimension::Unknown, Dimension::Literal),
        second.map_or(Dimension::Unknown, Dimension::Literal),
    ];
    Ok((
        vec![Some(Shape { dimensions })],
        None,
        graph.element_types.get(input).copied(),
    ))
}

fn split_shapes(
    node: &Node,
    graph: &ModelGraph,
) -> Result<(Vec<Option<Shape>>, Option<Vec<i64>>, Option<u64>), String> {
    let Some(input) = node.inputs.first().filter(|name| !name.is_empty()) else {
        return Ok((vec![None; node.outputs.len()], None, None));
    };
    let Some(shape) = graph.values.get(input) else {
        return Ok((
            vec![None; node.outputs.len()],
            None,
            graph.element_types.get(input).copied(),
        ));
    };
    Ok((
        vec![Some(shape.clone()); node.outputs.len()],
        None,
        graph.element_types.get(input).copied(),
    ))
}

fn broadcast_shapes(left: &Shape, right: &Shape) -> Result<Shape, String> {
    Ok(Shape {
        dimensions: broadcast_dimension_lists(&left.dimensions, &right.dimensions)?,
    })
}

fn broadcast_dimension_lists(
    left: &[Dimension],
    right: &[Dimension],
) -> Result<Vec<Dimension>, String> {
    let count = left.len().max(right.len());
    let mut dimensions = Vec::with_capacity(count);
    for offset in 0..count {
        let left_dimension = left.get(left.len().checked_sub(offset + 1).unwrap_or(usize::MAX));
        let right_dimension = right.get(right.len().checked_sub(offset + 1).unwrap_or(usize::MAX));
        dimensions.push(broadcast_dimension(left_dimension, right_dimension)?);
    }
    dimensions.reverse();
    Ok(dimensions)
}

fn broadcast_dimension(
    left: Option<&Dimension>,
    right: Option<&Dimension>,
) -> Result<Dimension, String> {
    if left.is_none_or(is_one) {
        return Ok(right.cloned().unwrap_or(Dimension::Literal(1)));
    }
    if right.is_none_or(is_one) {
        return Ok(left.cloned().unwrap_or(Dimension::Literal(1)));
    }
    match (left, right) {
        (Some(left), Some(right)) if dimensions_compatible(left, right) => Ok(left.clone()),
        (Some(Dimension::Unknown), _) | (_, Some(Dimension::Unknown)) => Ok(Dimension::Unknown),
        _ => Err("broadcast dimensions are incompatible".to_owned()),
    }
}

fn is_one(dimension: &Dimension) -> bool {
    matches!(dimension, Dimension::Literal(1))
}

fn dimensions_compatible(left: &Dimension, right: &Dimension) -> bool {
    matches!(
        (left, right),
        (Dimension::Unknown, _) | (_, Dimension::Unknown)
    ) || match (left, right) {
        (Dimension::Literal(left), Dimension::Literal(right)) => left == right,
        (Dimension::Symbolic(left), Dimension::Symbolic(right)) => left == right,
        _ => false,
    }
}

fn sum_dimension(left: &Dimension, right: &Dimension) -> Dimension {
    match (left, right) {
        (Dimension::Literal(left), Dimension::Literal(right)) => left
            .checked_add(*right)
            .map_or(Dimension::Unknown, Dimension::Literal),
        _ => Dimension::Unknown,
    }
}

fn shape_product(shape: &Shape) -> Option<u64> {
    shape_product_of(&shape.dimensions)
}

fn shape_product_of(dimensions: &[Dimension]) -> Option<u64> {
    dimensions
        .iter()
        .try_fold(1u64, |total, dimension| match dimension {
            Dimension::Literal(value) => total.checked_mul(*value),
            Dimension::Symbolic(_) | Dimension::Unknown => None,
        })
}

fn set_shape(graph: &mut ModelGraph, name: &str, shape: Shape) -> Result<bool, String> {
    if let Some(existing) = graph.values.get(name) {
        if existing != &shape {
            return Err(format!("tensor `{name}` has conflicting inferred shapes"));
        }
        return Ok(false);
    }
    graph.values.insert(name.to_owned(), shape);
    Ok(true)
}

fn set_values(graph: &mut ModelGraph, name: &str, values: Vec<i64>) -> Result<bool, String> {
    if let Some(existing) = graph.tensor_values.get(name) {
        if existing != &values {
            return Err(format!("tensor `{name}` has conflicting inferred values"));
        }
        return Ok(false);
    }
    graph.tensor_values.insert(name.to_owned(), values);
    Ok(true)
}

fn set_element_type(graph: &mut ModelGraph, name: &str, element_type: u64) -> Result<bool, String> {
    if let Some(existing) = graph.element_types.get(name) {
        if *existing != element_type {
            return Err(format!(
                "tensor `{name}` has conflicting inferred element types"
            ));
        }
        return Ok(false);
    }
    graph.element_types.insert(name.to_owned(), element_type);
    Ok(true)
}

fn tensor_dimensions(node: &Node, graph: &ModelGraph) -> Result<Option<Vec<Dimension>>, String> {
    match node.operator.as_str() {
        "Cast" | "Identity" => Ok(node
            .inputs
            .first()
            .and_then(|input| graph.tensor_dimensions.get(input))
            .cloned()),
        "Shape" => Ok(node
            .inputs
            .first()
            .and_then(|input| graph.values.get(input))
            .map(|shape| shape.dimensions.clone())),
        "Gather" => gather_tensor_dimensions(node, graph),
        "Concat" => concat_tensor_dimensions(node, graph),
        "Unsqueeze" => Ok(node
            .inputs
            .first()
            .and_then(|input| graph.tensor_dimensions.get(input))
            .cloned()),
        _ => Ok(None),
    }
}

fn concat_tensor_dimensions(
    node: &Node,
    graph: &ModelGraph,
) -> Result<Option<Vec<Dimension>>, String> {
    if attribute_int(node, "axis").unwrap_or(0) != 0 {
        return Ok(None);
    }
    let mut dimensions = Vec::new();
    for input in node.inputs.iter().filter(|input| !input.is_empty()) {
        if let Some(values) = graph.tensor_dimensions.get(input) {
            dimensions.extend(values.iter().cloned());
            continue;
        }
        let Some(values) = graph.tensor_values.get(input) else {
            return Ok(None);
        };
        let Some(values) = values
            .iter()
            .copied()
            .map(|value| u64::try_from(value).ok().map(Dimension::Literal))
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(None);
        };
        dimensions.extend(values);
    }
    Ok((!dimensions.is_empty()).then_some(dimensions))
}

fn gather_tensor_dimensions(
    node: &Node,
    graph: &ModelGraph,
) -> Result<Option<Vec<Dimension>>, String> {
    let (Some(data), Some(indices)) = (
        node.inputs.first().filter(|name| !name.is_empty()),
        node.inputs.get(1).filter(|name| !name.is_empty()),
    ) else {
        return Ok(None);
    };
    let (Some(data_dimensions), Some(index_values)) = (
        graph.tensor_dimensions.get(data),
        graph.tensor_values.get(indices),
    ) else {
        return Ok(None);
    };
    if !graph
        .values
        .get(indices)
        .is_some_and(|shape| shape.dimensions.is_empty())
    {
        return Ok(None);
    }
    let Some(index) = index_values.first().copied() else {
        return Ok(None);
    };
    let index = normalize_index(index, data_dimensions.len())?;
    Ok(data_dimensions
        .get(index)
        .cloned()
        .map(|dimension| vec![dimension]))
}

fn set_tensor_dimensions(
    graph: &mut ModelGraph,
    name: &str,
    dimensions: Vec<Dimension>,
) -> Result<bool, String> {
    if let Some(existing) = graph.tensor_dimensions.get(name) {
        if existing != &dimensions {
            return Err(format!("tensor `{name}` has conflicting inferred values"));
        }
        return Ok(false);
    }
    graph.tensor_dimensions.insert(name.to_owned(), dimensions);
    Ok(true)
}

fn attribute_int(node: &Node, name: &str) -> Option<i64> {
    node.attributes
        .iter()
        .find(|attribute| attribute.name == name)
        .and_then(|attribute| attribute.integer)
}

fn attribute_ints(node: &Node, name: &str) -> Option<Vec<i64>> {
    node.attributes
        .iter()
        .find(|attribute| attribute.name == name)
        .map(|attribute| attribute.integers.clone())
        .filter(|values| !values.is_empty())
}

fn normalize_index(index: i64, rank: usize) -> Result<usize, String> {
    let rank = i64::try_from(rank).map_err(|_| "rank is too large")?;
    let normalized = if index < 0 { rank + index } else { index };
    if normalized < 0 || normalized >= rank {
        return Err("axis is out of range".to_owned());
    }
    usize::try_from(normalized).map_err(|_| "axis is too large".to_owned())
}

fn normalize_insert_index(index: i64, rank: usize) -> Result<usize, String> {
    let rank = i64::try_from(rank).map_err(|_| "rank is too large")?;
    let normalized = if index < 0 { rank + index } else { index };
    if normalized < 0 || normalized > rank {
        return Err("insertion axis is out of range".to_owned());
    }
    usize::try_from(normalized).map_err(|_| "insertion axis is too large".to_owned())
}
