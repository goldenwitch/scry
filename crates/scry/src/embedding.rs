//! `embedding`: a vector frozen to the model that produced it.

use crate::model::Model;
use crate::vector::Vector;

/// A [`Vector`] and the [`Model`] that produced it.
///
/// Vectors from different models share no space, so the model is not a fact
/// about an embedding but part of what one is: there is no way to hold a
/// vector here without the model it came from, and no way to pair one with a
/// model of a different dimension.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Embedding {
    model: Model,
    vector: Vector,
}

impl Embedding {
    /// Binds `vector` to `model`, or `None` if the vector is not `dimension`
    /// floats long.
    #[must_use]
    pub(crate) fn new(model: Model, vector: Vector) -> Option<Self> {
        (vector.len() == model.dimension().get()).then_some(Self { model, vector })
    }

    /// The model that produced it.
    #[must_use]
    pub(crate) const fn model(&self) -> &Model {
        &self.model
    }

    /// The normalised floats.
    #[must_use]
    pub(crate) const fn vector(&self) -> &Vector {
        &self.vector
    }
}

#[cfg(test)]
mod tests {
    use super::Embedding;
    use crate::limit::Limit;
    use crate::model::Model;
    use crate::vector::Vector;

    fn model(dimension: usize) -> Model {
        let Some(limit) = Limit::new(512) else {
            unreachable!()
        };
        let Some(model) = Model::new("test", dimension, limit) else {
            unreachable!()
        };
        model
    }

    fn vector() -> Vector {
        let Some(vector) = Vector::normalise(vec![1.0, 0.0]) else {
            unreachable!()
        };
        vector
    }

    #[test]
    fn a_vector_of_the_wrong_dimension_does_not_embed() {
        assert_eq!(Embedding::new(model(3), vector()), None);
    }

    #[test]
    fn a_vector_of_the_model_s_dimension_does() {
        assert!(Embedding::new(model(2), vector()).is_some());
    }
}
