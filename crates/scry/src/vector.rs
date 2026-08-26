//! `vector`: floats, normalised, so cosine is a dot product.

/// `dimension` floats, normalised to unit length.
///
/// Normalisation happens where the vector is made, so nothing downstream has
/// to remember it and an unnormalised vector is not a thing that exists.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Vector(Vec<f32>);

impl Vector {
    /// Normalises `values` to unit length, or `None` if they are empty, hold
    /// anything that is not a finite number, or have no direction to keep.
    #[must_use]
    pub(crate) fn normalise(mut values: Vec<f32>) -> Option<Self> {
        if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
        if !norm.is_normal() {
            return None;
        }
        for value in &mut values {
            *value /= norm;
        }
        Some(Self(values))
    }

    /// The floats.
    #[must_use]
    pub(crate) fn as_slice(&self) -> &[f32] {
        &self.0
    }

    /// How many floats there are.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::Vector;

    #[test]
    fn a_vector_comes_back_normalised() {
        let Some(vector) = Vector::normalise(vec![3.0, 4.0]) else {
            unreachable!()
        };
        let norm: f32 = vector.as_slice().iter().map(|value| value * value).sum();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_vector_with_no_direction_is_not_a_vector() {
        assert_eq!(Vector::normalise(vec![0.0, 0.0]), None);
    }

    #[test]
    fn a_vector_holding_a_non_number_is_not_a_vector() {
        assert_eq!(Vector::normalise(vec![1.0, f32::NAN]), None);
        assert_eq!(Vector::normalise(vec![1.0, f32::INFINITY]), None);
    }
}
