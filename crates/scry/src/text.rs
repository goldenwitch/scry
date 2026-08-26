//! `text`: the UTF-8 a document is, and the only thing a span indexes.

use crate::span::Span;

/// Text: UTF-8, strictly.
///
/// scry indexes what it is given rather than converting it, so bytes that are
/// not UTF-8 never become `Text` — they become a refusal at the verb that read
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Text(String);

impl Text {
    /// Reads bytes as text, or `None` if they are not UTF-8.
    ///
    /// This is the whole of the check behind `NotText`: there is no lossy
    /// path, so substitution characters cannot enter the corpus.
    #[must_use]
    pub fn from_utf8(bytes: Vec<u8>) -> Option<Self> {
        String::from_utf8(bytes).ok().map(Self)
    }

    /// The text as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The length of the text in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the text holds no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A span of this text, or `None` if those offsets are not within it and
    /// on character boundaries.
    ///
    /// A span is born from the text it indexes, which is why no span can
    /// exceed its text.
    #[must_use]
    pub(crate) fn span(&self, start: usize, end: usize) -> Option<Span> {
        if start > end || end > self.0.len() {
            return None;
        }
        if !self.0.is_char_boundary(start) || !self.0.is_char_boundary(end) {
            return None;
        }
        Some(Span::new(start, end))
    }

    /// The text at `span`, or `None` if the span was not cut from this text.
    #[must_use]
    pub(crate) fn at(&self, span: Span) -> Option<&str> {
        self.0.get(span.start()..span.end())
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Self(text)
    }
}

#[cfg(test)]
mod tests {
    use super::Text;

    fn text() -> Text {
        Text::from("héllo".to_owned())
    }

    #[test]
    fn bytes_that_are_not_utf8_are_not_text() {
        assert_eq!(Text::from_utf8(vec![0xff, 0xfe]), None);
    }

    #[test]
    fn a_span_cannot_exceed_its_text() {
        let text = text();
        assert_eq!(text.span(0, text.len() + 1), None);
    }

    #[test]
    fn a_span_cannot_split_a_character() {
        // `é` occupies bytes 1 and 2.
        assert_eq!(text().span(0, 2), None);
    }

    #[test]
    fn a_span_reads_back_the_text_it_was_cut_from() {
        let text = text();
        let Some(span) = text.span(0, 3) else {
            unreachable!()
        };
        assert_eq!(text.at(span), Some("hé"));
    }

    #[test]
    fn a_span_from_another_text_does_not_read() {
        let long = text();
        let Some(span) = long.span(0, 3) else {
            unreachable!()
        };
        let short = Text::from("h".to_owned());
        assert_eq!(short.at(span), None);
    }
}
