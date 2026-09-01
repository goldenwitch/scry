//! `slice`: where the boundaries fall.
//!
//! Counting happens in tokens and cutting happens at offsets, and only the
//! model's tokenizer relates the two — so this is the one place that holds a
//! tokenizer for counting rather than for embedding.

use core::num::NonZeroUsize;
use std::io;

use tokenizers::Tokenizer;

use crate::embed::Embed;
use crate::span::Span;
use crate::text::Text;
use crate::window::Window;

/// The window slice targets, in tokens: half the model's limit, so a chunk's
/// embedding stays on one subject.
const WINDOW: usize = 256;

/// The slice seam: text in, the spans it is cut into out.
///
/// The spans partition the text — contiguous, non-overlapping, covering — so
/// every offset falls in exactly one, and they are what `Document::new` is
/// given.
///
/// Cuts fall every window of tokens and by no other rule. There is no sentence
/// or paragraph preference here: the window was the boundary decision. A window
/// is measured from the cut before it, so no chunk holds more than one.
pub struct Slice {
    tokenizer: Tokenizer,
    budget: NonZeroUsize,
}

/// Text and the bounded spans cut from that exact text.
///
/// Only [`Slice::cut`] can create this value, so the embedding seam can accept
/// it without reopening the model-limit check or pairing spans with another
/// text by convention.
pub(crate) struct SlicedText {
    text: Text,
    spans: Vec<Span>,
}

impl SlicedText {
    pub(crate) fn passages(&self) -> io::Result<Vec<&str>> {
        self.spans
            .iter()
            .map(|span| self.text.at(*span))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| io::Error::other("a span of the sliced text does not read"))
    }

    pub(crate) fn into_parts(self) -> (Text, Vec<Span>) {
        (self.text, self.spans)
    }
}

impl Slice {
    /// Builds the seam from the model `embed` is bound to.
    ///
    /// The tokenizer comes from the embed seam, and so does the limit the
    /// window is measured against, so a slicer cut for one model and a model
    /// that embeds it cannot disagree.
    ///
    /// The tokenizer built here has truncation and padding switched off. The
    /// one inside fastembed is configured to truncate at the model's limit, so
    /// it reports a long text as 512 tokens rather than counting it, and this
    /// seam needs to count.
    ///
    /// # Errors
    ///
    /// The tokenizer refusing its own bytes, or the window leaving no room for
    /// text once the tokenizer's own tokens are in it — both weather rather
    /// than any of the refusals scry names.
    pub fn new(embed: &Embed) -> io::Result<Self> {
        let mut tokenizer =
            Tokenizer::from_bytes(embed.tokenizer_file()).map_err(io::Error::other)?;
        tokenizer.with_truncation(None).map_err(io::Error::other)?;
        tokenizer.with_padding(None);
        let window = Window::new(WINDOW, embed.model().limit())
            .ok_or_else(|| io::Error::other("the window does not fit the model's limit"))?;
        let added = tokenizer.encode("", true).map_err(io::Error::other)?.len();
        let budget = window
            .tokens()
            .get()
            .checked_sub(added)
            .and_then(NonZeroUsize::new)
            .ok_or_else(|| io::Error::other("the tokenizer's own tokens fill the window"))?;
        Ok(Self { tokenizer, budget })
    }

    /// Takes ownership of text and returns its bounded, paired spans.
    ///
    /// The owner keeps the text beside the spans until embedding is complete,
    /// so a caller cannot hand the embedding seam an unrelated string slice.
    pub(crate) fn cut(&self, text: Text) -> io::Result<SlicedText> {
        let spans = self.spans(&text)?;
        Ok(SlicedText { text, spans })
    }

    /// Counts the tokens and spans of `text` for benchmark workload identity.
    ///
    /// This is available only to benchmark builds. It reuses the slicer's
    /// tokenizer and therefore follows the same pinned input interpretation as
    /// the cut operation.
    ///
    /// # Errors
    ///
    /// The pinned tokenizer refusing the text.
    #[cfg(feature = "benchmark-instrumentation")]
    pub fn benchmark_counts(&self, text: &Text) -> io::Result<(usize, usize)> {
        let tokens = self
            .tokenizer
            .encode(text.as_str(), false)
            .map_err(io::Error::other)?
            .len();
        let spans = self.spans(text)?.len();
        Ok((tokens, spans))
    }

    /// Returns copied benchmark passages from the exact sliced text.
    ///
    /// This is available only to benchmark builds that need to inspect the
    /// model tokenizer's padded batch shape without embedding the passages.
    ///
    /// # Errors
    ///
    /// The pinned tokenizer refusing the text or a sliced passage not reading
    /// from the text it was cut from.
    #[cfg(feature = "benchmark-instrumentation")]
    pub fn benchmark_passages(&self, text: &Text) -> io::Result<Vec<String>> {
        let sliced = self.cut(text.clone())?;
        Ok(sliced.passages()?.into_iter().map(str::to_owned).collect())
    }

    /// Cuts `text` into the spans that partition it.
    ///
    /// No span holds more than a window of tokens, so a chunk the model would
    /// read only the beginning of is not produced.
    ///
    /// An empty text is cut into nothing, which is the partition of no bytes.
    ///
    /// # Errors
    ///
    /// The tokenizer failing to read the text, which is weather; and a whole
    /// window of tokens spent inside one character, which leaves nowhere to
    /// cut.
    pub(crate) fn spans(&self, text: &Text) -> io::Result<Vec<Span>> {
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let encoding = self
            .tokenizer
            .encode(text.as_str(), false)
            .map_err(io::Error::other)?;
        crate::benchmark::record_sliced_tokens(encoding.len());
        let mut start = 0;
        let mut spans = Vec::new();
        for end in self.cuts(text, encoding.get_offsets())? {
            spans.push(span(text, start, end)?);
            start = end;
        }
        spans.push(span(text, start, text.len())?);
        crate::benchmark::record_sliced_spans(spans.len());
        Ok(spans)
    }

    /// The offsets to cut at: the offset the token a window ends on begins at.
    ///
    /// Spans are derived from these cut points rather than from the tokens'
    /// own offsets, because a word-piece tokenizer's offsets skip inter-token
    /// whitespace and normalised-away characters — so spans built from them
    /// would have gaps, and covering is what a document is checked for.
    ///
    /// A window is measured from the cut that was taken, not from a grid laid
    /// over the whole text, and the loop runs while more than a window of
    /// tokens is left — so a chunk holding more than a window is not something
    /// this can leave behind. Under a grid, a cut that could not be taken
    /// handed its tokens to the next chunk, and two in a row handed on three
    /// windows at once, which is more than the model reads.
    ///
    /// # Errors
    ///
    /// A whole window of tokens spent inside one character, which leaves
    /// nowhere to cut: every candidate is the same offset, and cutting
    /// anywhere else would split a character, which is not a span at all.
    fn cuts(&self, text: &Text, offsets: &[(usize, usize)]) -> io::Result<Vec<usize>> {
        let budget = self.budget.get();
        let mut cuts = Vec::new();
        let mut first = 0;
        while first + budget < offsets.len() {
            let held = offsets.get(first).map_or(0, |(start, _)| *start);
            let mut at = first + budget;
            let cut = match offsets.get(at) {
                Some((start, _))
                    if *start > held
                        && *start < text.len()
                        && text.as_str().is_char_boundary(*start) =>
                {
                    *start
                }
                _ => {
                    return Err(io::Error::other(
                        "a window of tokens falls inside one character, so there is nowhere to cut",
                    ));
                }
            };
            while at > 0 && offsets.get(at - 1).is_some_and(|(start, _)| *start == cut) {
                at -= 1;
            }
            cuts.push(cut);
            first = at;
        }
        Ok(cuts)
    }
}

/// A span of `text`, or an error if the tokenizer's offsets did not land in
/// the text they were counted from.
fn span(text: &Text, start: usize, end: usize) -> io::Result<Span> {
    text.span(start, end)
        .ok_or_else(|| io::Error::other("a cut fell outside the text it was counted from"))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{Slice, WINDOW};
    use crate::document::Document;
    use crate::origin::Origin;
    use crate::scaffold::seams;
    use crate::text::Text;

    fn slice() -> Slice {
        seams().1
    }

    /// A text of `words` tokens' worth of ordinary English, with a multibyte
    /// character in it so the spans are checked against something other than
    /// ASCII.
    fn long(words: usize) -> Text {
        let mut text = String::from("Le café was quiet.");
        for word in 0..words {
            text.push_str(if word % 7 == 0 { "\n\n" } else { " " });
            text.push_str("the kettle boiled and the room filled with steam");
        }
        Text::from(text)
    }

    fn spans_of(slice: &Slice, text: &Text) -> Vec<crate::span::Span> {
        match slice.spans(text) {
            Ok(spans) => spans,
            Err(error) => unreachable!("{error}"),
        }
    }

    fn count(slice: &Slice, text: &str, special: bool) -> usize {
        match slice.tokenizer.encode(text, special) {
            Ok(encoding) => encoding.len(),
            Err(error) => unreachable!("{error}"),
        }
    }

    #[test]
    fn the_window_leaves_room_for_text() {
        // 256 less the [CLS] and [SEP] this tokenizer adds to a chunk.
        assert_eq!(slice().budget.get(), WINDOW - 2);
    }

    #[test]
    fn the_tokenizer_counts_past_the_model_limit() {
        // The whole reason this seam builds its own: fastembed's truncates
        // here, and would answer 512.
        let slice = slice();
        let text = long(400);
        assert!(count(&slice, text.as_str(), false) > 512);
    }

    #[test]
    fn an_empty_text_is_cut_into_nothing() {
        assert!(spans_of(&slice(), &Text::from(String::new())).is_empty());
    }

    #[test]
    fn a_text_under_the_window_is_one_chunk() {
        let slice = slice();
        let text = Text::from("Le café was quiet.".to_owned());
        let spans = spans_of(&slice, &text);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            spans.first().and_then(|span| text.at(*span)),
            Some(text.as_str())
        );
    }

    #[test]
    fn the_spans_partition_the_text() {
        let slice = slice();
        let text = long(400);
        let spans = spans_of(&slice, &text);
        assert!(spans.len() > 1);
        let document = Document::new(
            match Origin::parse("a.md") {
                Some(origin) => origin,
                None => unreachable!(),
            },
            text.clone(),
            SystemTime::UNIX_EPOCH,
            Duration::from_secs(60),
            spans.clone(),
        );
        assert!(document.is_some());
        let rejoined: String = spans.iter().filter_map(|span| text.at(*span)).collect();
        assert_eq!(rejoined, text.as_str());
    }

    #[test]
    fn a_cut_falls_every_window_of_tokens() {
        let slice = slice();
        let text = long(400);
        let tokens = count(&slice, text.as_str(), false);
        let spans = spans_of(&slice, &text);
        assert_eq!(spans.len(), tokens.div_ceil(slice.budget.get()));
    }

    #[test]
    fn no_chunk_exceeds_the_limit() {
        let slice = slice();
        let text = long(400);
        let largest = spans_of(&slice, &text)
            .iter()
            .filter_map(|span| text.at(*span))
            .map(|chunk| count(&slice, chunk, true))
            .max();
        // Measured: the largest chunk re-counted on its own comes back at 256,
        // the window exactly, against a limit of 512. The assertion is the
        // stronger of the two, so the limit is covered by it.
        assert_eq!(largest, Some(WINDOW));
    }

    /// Offsets of `tokens` tokens over a text of one byte per token, with the
    /// tokens from `run` numbering `held` all beginning at the same offset —
    /// which is what a character the tokenizer spends several tokens on looks
    /// like from here.
    fn offsets(tokens: usize, run: usize, held: usize) -> (Text, Vec<(usize, usize)>) {
        let offsets = (0..tokens)
            .map(|token| {
                let start = if (run..run + held).contains(&token) {
                    run
                } else if token < run {
                    token
                } else {
                    token - held + 1
                };
                (start, start + 1)
            })
            .collect();
        (Text::from("a".repeat(tokens + 1)), offsets)
    }

    /// The tokens each chunk holds, read off the cuts: a token belongs to the
    /// chunk its own offset falls in.
    fn held(cuts: &[usize], offsets: &[(usize, usize)], end: usize) -> Vec<usize> {
        let mut bounds = vec![0];
        bounds.extend_from_slice(cuts);
        bounds.push(end);
        bounds
            .windows(2)
            .filter_map(|pair| match pair {
                [from, to] => Some(
                    offsets
                        .iter()
                        .filter(|(start, _)| start >= from && start < to)
                        .count(),
                ),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_run_of_tokens_at_a_cut_does_not_lengthen_the_chunk_after_it() {
        let slice = slice();
        let budget = slice.budget.get();
        // Three tokens on one character, straddling the first cut — which is
        // what the tokenizer does to a Hangul syllable, measured.
        let (text, offsets) = offsets(budget * 3, budget - 1, 3);
        let Ok(cuts) = slice.cuts(&text, &offsets) else {
            unreachable!()
        };
        assert!(
            held(&cuts, &offsets, text.len())
                .iter()
                .all(|chunk| *chunk <= budget)
        );
    }

    #[test]
    fn a_window_of_tokens_inside_one_character_is_an_error_and_not_a_chunk() {
        let slice = slice();
        let budget = slice.budget.get();
        // The adversary: a run long enough that two cuts in a row have nowhere
        // to land. Under a grid those two would have handed three windows to
        // one chunk - 254 x 3 + 2 specials against a limit of 512. There is no
        // text this tokenizer answers this way, so the rule is driven directly.
        let (text, offsets) = offsets(budget * 5, budget, budget * 2 + 2);
        assert!(slice.cuts(&text, &offsets).is_err());
    }

    #[test]
    fn no_chunk_holds_more_than_a_window_of_tokens() {
        let slice = slice();
        let text = long(400);
        let Ok(encoding) = slice.tokenizer.encode(text.as_str(), false) else {
            unreachable!()
        };
        let Ok(cuts) = slice.cuts(&text, encoding.get_offsets()) else {
            unreachable!()
        };
        let chunks = held(&cuts, encoding.get_offsets(), text.len());
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| *chunk <= slice.budget.get()));
    }
}
