//! `document`: an origin, its text, when it was read, and how it is cut.

use std::time::{Duration, SystemTime};

use crate::origin::Origin;
use crate::span::Span;
use crate::text::Text;

/// An [`Origin`], its [`Text`], the moment it was read, its `ttl`, and the
/// spans it is cut into.
///
/// The spans partition the text — contiguous, non-overlapping, covering — and
/// a document that would not be so cut cannot be made. The partition is
/// therefore a fact about the document rather than something each slicer has
/// to reproduce.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Document {
    origin: Origin,
    text: Text,
    fetched_at: SystemTime,
    ttl: Duration,
    spans: Vec<Span>,
}

impl Document {
    /// The document read from `origin` at `fetched_at`, cut into `spans`, or
    /// `None` if those spans do not partition the text.
    #[must_use]
    pub(crate) fn new(
        origin: Origin,
        text: Text,
        fetched_at: SystemTime,
        ttl: Duration,
        spans: Vec<Span>,
    ) -> Option<Self> {
        partitions(&text, &spans).then_some(Self {
            origin,
            text,
            fetched_at,
            ttl,
            spans,
        })
    }

    /// Its identity.
    #[must_use]
    pub(crate) const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// What was read.
    #[must_use]
    pub(crate) const fn text(&self) -> &Text {
        &self.text
    }

    /// When it was read.
    #[must_use]
    pub(crate) const fn fetched_at(&self) -> SystemTime {
        self.fetched_at
    }

    /// How long the material was said to stay good for.
    #[must_use]
    pub(crate) const fn ttl(&self) -> Duration {
        self.ttl
    }

    /// How the text is cut, in order, covering all of it.
    #[must_use]
    pub(crate) fn spans(&self) -> &[Span] {
        &self.spans
    }
}

/// Whether `spans` are contiguous, non-overlapping and covering over `text`,
/// so every offset in it falls in exactly one of them.
fn partitions(text: &Text, spans: &[Span]) -> bool {
    let text = text.as_str();
    let mut next = 0;
    for span in spans {
        if span.start() != next || span.is_empty() {
            return false;
        }
        if !text.is_char_boundary(span.start()) || !text.is_char_boundary(span.end()) {
            return false;
        }
        next = span.end();
    }
    next == text.len()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::Document;
    use crate::scaffold::origin;
    use crate::span::Span;
    use crate::text::Text;

    fn text() -> Text {
        Text::from("one two three".to_owned())
    }

    fn cut(text: &Text, cuts: &[(usize, usize)]) -> Vec<Span> {
        cuts.iter()
            .filter_map(|(start, end)| text.span(*start, *end))
            .collect()
    }

    fn document(spans: Vec<Span>) -> Option<Document> {
        Document::new(
            origin("a.md"),
            text(),
            SystemTime::UNIX_EPOCH,
            Duration::from_secs(60),
            spans,
        )
    }

    #[test]
    fn a_covering_partition_makes_a_document() {
        let text = text();
        let spans = cut(&text, &[(0, 4), (4, 8), (8, 13)]);
        assert!(document(spans).is_some());
    }

    #[test]
    fn a_gap_is_not_a_partition() {
        let text = text();
        let spans = cut(&text, &[(0, 4), (5, 13)]);
        assert!(document(spans).is_none());
    }

    #[test]
    fn an_overlap_is_not_a_partition() {
        let text = text();
        let spans = cut(&text, &[(0, 5), (4, 13)]);
        assert!(document(spans).is_none());
    }

    #[test]
    fn stopping_short_of_the_end_is_not_a_partition() {
        let text = text();
        let spans = cut(&text, &[(0, 4)]);
        assert!(document(spans).is_none());
    }

    #[test]
    fn an_empty_span_is_not_part_of_a_partition() {
        let text = text();
        let spans = cut(&text, &[(0, 4), (4, 4), (4, 13)]);
        assert!(document(spans).is_none());
    }

    #[test]
    fn an_empty_document_is_cut_into_nothing() {
        let empty = Document::new(
            origin("a.md"),
            Text::from(String::new()),
            SystemTime::UNIX_EPOCH,
            Duration::from_secs(60),
            Vec::new(),
        );
        assert!(empty.is_some());
    }
}
