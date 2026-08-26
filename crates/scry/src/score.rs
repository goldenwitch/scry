//! `score`: the cosine of a query against a passage.

use crate::embedding::Embedding;

/// The cosine of one embedding against another.
///
/// A score is only made by taking that cosine, so a number that ranks a hit
/// cannot come from anywhere else.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Score(f32);

impl Score {
    /// The cosine of `query` against `passage`, or `None` if they were
    /// produced by different models and so share no space.
    ///
    /// Both vectors are normalised, so this is their dot product.
    #[must_use]
    pub(crate) fn cosine(query: &Embedding, passage: &Embedding) -> Option<Self> {
        if query.model() != passage.model() {
            return None;
        }
        let dot: f32 = query
            .vector()
            .as_slice()
            .iter()
            .zip(passage.vector().as_slice())
            .map(|(query, passage)| query * passage)
            .sum();
        Some(Self(dot.clamp(-1.0, 1.0)))
    }

    /// The cosine, between -1 and 1.
    #[must_use]
    pub const fn get(&self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Score;
    use crate::embedding::Embedding;
    use crate::limit::Limit;
    use crate::model::Model;
    use crate::vector::Vector;

    fn embedding(name: &str, values: Vec<f32>) -> Embedding {
        let Some(limit) = Limit::new(512) else {
            unreachable!()
        };
        let Some(model) = Model::new(name, values.len(), limit) else {
            unreachable!()
        };
        let Some(vector) = Vector::normalise(values) else {
            unreachable!()
        };
        let Some(embedding) = Embedding::new(model, vector) else {
            unreachable!()
        };
        embedding
    }

    #[test]
    fn a_thing_is_itself() {
        let one = embedding("test", vec![1.0, 2.0, 3.0]);
        let Some(score) = Score::cosine(&one, &one) else {
            unreachable!()
        };
        assert!((score.get() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn different_models_do_not_compare() {
        let one = embedding("one", vec![1.0, 0.0]);
        let other = embedding("other", vec![1.0, 0.0]);
        assert_eq!(Score::cosine(&one, &other), None);
    }
}
