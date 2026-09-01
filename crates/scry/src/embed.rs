//! `embed`: the seam that binds a model to a vector.
//!
//! Everything fastembed is kept behind this file, so no other part of scry
//! holds an inference session, a tokenizer, or a bare `Vec<f32>`.

use core::sync::atomic::{AtomicU64, Ordering};
use std::fs;
use std::io;
use std::path::Path;

use fastembed::{
    InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};

use crate::embedding::Embedding;
use crate::limit::Limit;
use crate::model::Model;
use crate::query::Query;
use crate::slice::SlicedText;
use crate::vector::Vector;

/// The repository the weights come from: the ONNX export of
/// `BAAI/bge-small-en-v1.5` that fastembed itself pulls for its default model.
const REPOSITORY: &str = "Xenova/bge-small-en-v1.5";

/// The commit the weights are taken at.
///
/// A revision is what makes the model's name mean the same bytes twice. It is
/// part of the name for that reason: re-pinning produces a model that scores
/// in a different space, and the store is entitled to notice.
const REVISION: &str = "ea104dacec62c0de699686887e3f920caeb4f3e3";

/// The name the model travels under, revision included.
const NAME: &str = "Xenova/bge-small-en-v1.5@ea104dacec62c0de699686887e3f920caeb4f3e3";

/// How many floats one of its vectors holds.
const DIMENSION: usize = 384;

/// The most input it accepts, in tokens.
const LIMIT: usize = 512;

/// The internal fastembed batch size whose raw outputs are retained by one
/// embedding call until pooling completes.
const FASTEMBED_BATCH_SIZE: usize = 256;

/// The passage batch sent through one fastembed call.
pub const PASSAGE_MICROBATCH_SIZE: usize = 32;

/// What a query is prefixed with, from the model list table on the BAAI model
/// card. A passage takes no prefix.
const QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";

/// A file at the pinned revision: where it lives in the repository, and how
/// many bytes it is there.
type File = (&'static str, u64);

const ONNX: File = ("onnx/model.onnx", 133_093_490);
const TOKENIZER: File = ("tokenizer.json", 711_396);
const CONFIG: File = ("config.json", 683);
const SPECIAL_TOKENS_MAP: File = ("special_tokens_map.json", 125);
const TOKENIZER_CONFIG: File = ("tokenizer_config.json", 366);

/// Distinguishes the half-written files of concurrent downloads.
static PARTIAL: AtomicU64 = AtomicU64::new(0);

/// The embed seam: text in, an `Embedding` out.
///
/// A vector never leaves here on its own. It leaves normalised and married to
/// the [`Model`] that produced it, so cosine is a dot product downstream and
/// two embeddings from different models cannot be compared by accident.
///
/// A query and a passage are embedded by different methods because the model
/// requires different input for each: `query` applies the model's instruction
/// prefix, `passages` applies nothing. Neither is a caller's to reach: the
/// verbs are, and they hold this seam between them.
///
/// Nothing leaves here describing only the beginning of what it was given.
/// Text the model would truncate is refused instead, so a vector that stands
/// for a prefix of its own text does not exist to be stored or scored.
pub struct Embed {
    model: Model,
    tokenizer_file: Vec<u8>,
    embedder: TextEmbedding,
}

impl Embed {
    /// Loads the pinned model, fetching its files into `cache` if they are not
    /// there already, and returns the seam.
    ///
    /// The second call with the same `cache` reads from disk and touches no
    /// network. Where that directory is belongs to whoever owns a path — embed
    /// cannot know the caller's disk.
    ///
    /// # Errors
    ///
    /// The network, the disk, and the ONNX Runtime, all of which are weather
    /// rather than any of the refusals scry names.
    pub fn load(cache: &Path) -> io::Result<Self> {
        let model = model().ok_or_else(|| {
            io::Error::other("the pinned dimension and limit do not describe a model")
        })?;
        let tokenizer_file = file(cache, TOKENIZER)?;
        let defined = UserDefinedEmbeddingModel::new(
            file(cache, ONNX)?,
            TokenizerFiles {
                tokenizer_file: tokenizer_file.clone(),
                config_file: file(cache, CONFIG)?,
                special_tokens_map_file: file(cache, SPECIAL_TOKENS_MAP)?,
                tokenizer_config_file: file(cache, TOKENIZER_CONFIG)?,
            },
        )
        .with_pooling(Pooling::Cls);
        let embedder = TextEmbedding::try_new_from_user_defined(
            defined,
            InitOptionsUserDefined::new().with_max_length(LIMIT),
        )
        .map_err(io::Error::other)?;
        Ok(Self {
            model,
            tokenizer_file,
            embedder,
        })
    }

    /// Returns the padded sequence length the model tokenizer assigns to a
    /// benchmark batch without running inference.
    ///
    /// # Errors
    ///
    /// The pinned tokenizer refusing a passage or the batch containing no
    /// passages.
    #[cfg(feature = "benchmark-instrumentation")]
    pub fn benchmark_padded_sequence_length(&self, texts: &[String]) -> io::Result<usize> {
        let inputs = texts.iter().map(String::as_str).collect::<Vec<_>>();
        let encodings = self
            .embedder
            .tokenizer
            .encode_batch(inputs, true)
            .map_err(io::Error::other)?;
        let Some(encoding) = encodings.first() else {
            return Err(io::Error::other("the benchmark passage batch was empty"));
        };
        Ok(encoding.len())
    }

    /// The model these embeddings are frozen to.
    #[must_use]
    pub const fn model(&self) -> &Model {
        &self.model
    }

    /// The bytes of the pinned `tokenizer.json`.
    ///
    /// The tokenizer this seam holds truncates at the model's limit, so it
    /// counts a long text as 512 tokens rather than counting it. Slice needs
    /// to count, so it gets the bytes and configures its own.
    #[must_use]
    pub(crate) fn tokenizer_file(&self) -> &[u8] {
        &self.tokenizer_file
    }

    /// Embeds passages together, with no prefix, which is how a document's
    /// chunks arrive. It is the only passage path, since `add` embeds a whole
    /// document at once and a batch of one is the same call.
    ///
    /// # Errors
    ///
    /// The inference session, which is weather; and text longer than the
    /// model accepts, which would otherwise be answered with an embedding of
    /// its beginning.
    #[cfg(test)]
    pub(crate) fn passages(&mut self, texts: &[&str]) -> io::Result<Vec<Embedding>> {
        self.validated_many(texts)
    }

    /// Embeds text and bounded passages produced by [`Slice`](crate::Slice).
    ///
    /// The paired value preserves the slicer's model-limit guarantee without
    /// copying the document text or accepting an unchecked string slice. Each
    /// microbatch is pooled before the next one is sent to fastembed.
    pub(crate) fn passages_from_slice(
        &mut self,
        sliced: &SlicedText,
    ) -> io::Result<Vec<Embedding>> {
        let passages = sliced.passages()?;
        let mut embeddings = Vec::with_capacity(passages.len());
        for microbatch in passages.chunks(PASSAGE_MICROBATCH_SIZE) {
            embeddings.extend(self.many(microbatch)?);
        }
        Ok(embeddings)
    }

    /// Embeds a query, which takes the model's instruction prefix.
    ///
    /// # Errors
    ///
    /// The inference session, which is weather; and a query longer than the
    /// model accepts, measured with the prefix on it, since that is what the
    /// model reads.
    pub(crate) fn query(&mut self, query: &Query) -> io::Result<Embedding> {
        let prefixed = format!("{QUERY_PREFIX}{}", query.text().as_str());
        self.one(&prefixed)
    }

    fn one(&mut self, text: &str) -> io::Result<Embedding> {
        self.validated_many(&[text])?
            .pop()
            .ok_or_else(|| io::Error::other("the model returned no vector"))
    }

    fn validated_many(&mut self, texts: &[&str]) -> io::Result<Vec<Embedding>> {
        for text in texts {
            self.whole(text)?;
        }
        self.many(texts)
    }

    fn many(&mut self, texts: &[&str]) -> io::Result<Vec<Embedding>> {
        let input_bytes = texts
            .iter()
            .try_fold(0usize, |total, text| total.checked_add(text.len()));
        let Some(input_bytes) = input_bytes else {
            return Err(io::Error::other("embedding input byte count overflowed"));
        };
        crate::benchmark::record_embedding_call(input_bytes);
        crate::benchmark::record_embedding_call_size(texts.len());
        crate::benchmark::record_embedding_batches(texts.len().div_ceil(FASTEMBED_BATCH_SIZE));
        let values = self
            .embedder
            .embed(texts, Some(FASTEMBED_BATCH_SIZE))
            .map_err(io::Error::other)?;
        crate::benchmark::record_embedding_vectors(values.len());
        crate::benchmark::record_owned_logical_bytes(input_bytes);
        values
            .into_iter()
            .map(|values| self.embedding(values))
            .collect()
    }

    /// Refuses text the model would read only the beginning of.
    ///
    /// The tokenizer here is configured to truncate at the model's limit, so
    /// text over it is otherwise answered with an embedding of its prefix and
    /// nothing says so. A truncated encoding carries what was dropped, so the
    /// question is asked of the same tokenizer that would do the dropping —
    /// and this file restates no number the model's own configuration holds.
    ///
    /// It is a mistake rather than one of the five refusals: slice cuts to
    /// this limit, so a chunk that reaches here is a defect, and a query that
    /// does is a caller handing the model more than it accepts.
    fn whole(&self, text: &str) -> io::Result<()> {
        let encoding = self
            .embedder
            .tokenizer
            .encode(text, true)
            .map_err(io::Error::other)?;
        if encoding.get_overflowing().is_empty() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the text is longer than the model accepts",
            ))
        }
    }

    fn embedding(&self, values: Vec<f32>) -> io::Result<Embedding> {
        let vector = Vector::normalise(values).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the model returned a vector with no direction",
            )
        })?;
        Embedding::new(self.model.clone(), vector).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the model returned a vector of another dimension",
            )
        })
    }
}

/// The pinned model, or `None` if the constants above stopped describing one.
fn model() -> Option<Model> {
    Model::new(NAME, DIMENSION, Limit::new(LIMIT)?)
}

/// Reads a pinned file from `cache`, fetching it first if it is not there.
///
/// The file is cached under its revision, so two pins can sit side by side,
/// and it is written under another name and renamed into place, so a download
/// that stops halfway is not read back as the model.
fn file(cache: &Path, (remote, size): File) -> io::Result<Vec<u8>> {
    let name = remote.rsplit('/').next().unwrap_or(remote);
    let directory = cache.join(REVISION);
    let local = directory.join(name);
    if let Ok(bytes) = fs::read(&local)
        && bytes.len() as u64 == size
    {
        return Ok(bytes);
    }
    let bytes = download(remote, size)?;
    if bytes.len() as u64 != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{remote} is not the size the pinned revision says it is"),
        ));
    }
    fs::create_dir_all(&directory)?;
    let unique = PARTIAL.fetch_add(1, Ordering::Relaxed);
    let partial = directory.join(format!("{name}.{}.{unique}.partial", std::process::id()));
    fs::write(&partial, &bytes)?;
    fs::rename(&partial, &local)?;
    Ok(bytes)
}

/// Fetches one file at the pinned revision.
///
/// The pinned size bounds the read, so a server that answers with something
/// other than the pinned bytes cannot answer with an unbounded amount of them.
/// The bound is one byte past the pin because the reader refuses the read that
/// follows an exhausted limit, and the size is checked exactly by the caller.
fn download(remote: &str, size: u64) -> io::Result<Vec<u8>> {
    let url = format!("https://huggingface.co/{REPOSITORY}/resolve/{REVISION}/{remote}");
    ureq::get(&url)
        .call()
        .map_err(io::Error::other)?
        .body_mut()
        .with_config()
        .limit(size + 1)
        .read_to_vec()
        .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "benchmark-instrumentation")]
    use super::PASSAGE_MICROBATCH_SIZE;
    use super::{DIMENSION, Embed, LIMIT, QUERY_PREFIX, model};
    use crate::embedding::Embedding;
    use crate::query::Query;
    use crate::scaffold::{embed, seams};
    use crate::score::Score;
    use crate::text::Text;

    /// One passage, taken out of the batch — which is the only passage path
    /// there is, since `add` embeds a document's chunks together.
    fn passage(embed: &mut Embed, text: &str) -> std::io::Result<Embedding> {
        embed
            .passages(&[text])?
            .pop()
            .ok_or_else(|| std::io::Error::other("the model returned no vector"))
    }

    #[test]
    fn the_pinned_constants_describe_a_model() {
        let Some(model) = model() else { unreachable!() };
        assert_eq!(model.dimension().get(), DIMENSION);
    }

    #[test]
    fn a_passage_is_itself_under_the_model_it_names() {
        let mut embed = embed();
        let Ok(passage) = passage(&mut embed, "The kettle is on.") else {
            unreachable!()
        };
        assert_eq!(passage.model(), embed.model());
        assert_eq!(passage.vector().len(), DIMENSION);
        let Some(score) = Score::cosine(&passage, &passage) else {
            unreachable!()
        };
        assert!((score.get() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_query_takes_the_prefix_and_a_passage_does_not() {
        let mut embed = embed();
        let asked = "what is on the stove";
        let Ok(unprefixed) = passage(&mut embed, asked) else {
            unreachable!()
        };
        let Ok(query) = embed.query(&Query::new(Text::from(asked.to_owned()))) else {
            unreachable!()
        };
        assert_ne!(query.vector(), unprefixed.vector());
        let Ok(prefixed) = passage(&mut embed, &format!("{QUERY_PREFIX}{asked}")) else {
            unreachable!()
        };
        assert_eq!(query.vector(), prefixed.vector());
    }

    /// A text of `words` words, which is at least `words` tokens.
    fn long(words: usize) -> String {
        "the kettle boiled and the room filled with steam "
            .repeat(words.div_ceil(9))
            .trim_end()
            .to_owned()
    }

    #[test]
    fn a_passage_longer_than_the_model_accepts_is_refused() {
        let mut embed = embed();
        let Err(error) = passage(&mut embed, &long(2000)) else {
            unreachable!("a text the model would truncate was embedded")
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn a_batch_is_refused_for_the_one_passage_that_is_too_long() {
        let mut embed = embed();
        let long = long(2000);
        assert!(embed.passages(&["The kettle is on.", &long]).is_err());
    }

    #[cfg(feature = "benchmark-instrumentation")]
    #[test]
    fn a_large_sliced_batch_is_split_before_embedding() {
        let (mut embed, slice) = seams();
        let Ok(sliced) = slice.cut(Text::from(long(9000))) else {
            unreachable!()
        };
        let passage_count = match sliced.passages() {
            Ok(passages) => passages.len(),
            Err(error) => unreachable!("{error}"),
        };
        let collector = crate::BenchmarkCollector::start();
        let embeddings = match embed.passages_from_slice(&sliced) {
            Ok(embeddings) => embeddings,
            Err(error) => unreachable!("{error}"),
        };
        let snapshot = collector.finish();
        assert_eq!(embeddings.len(), passage_count);
        assert_eq!(
            snapshot.embedding_calls(),
            passage_count.div_ceil(PASSAGE_MICROBATCH_SIZE) as u64
        );
        assert_eq!(snapshot.embedding_batches(), snapshot.embedding_calls());
        assert_eq!(
            snapshot.max_embedding_call_size(),
            passage_count.min(PASSAGE_MICROBATCH_SIZE) as u64
        );
    }

    #[test]
    fn a_query_longer_than_the_model_accepts_is_refused() {
        let mut embed = embed();
        let query = Query::new(Text::from(long(2000)));
        assert!(embed.query(&query).is_err());
    }

    #[test]
    fn the_refusal_falls_exactly_at_the_model_limit() {
        // The known answer this instrument is checked against. `kettle` is one
        // token under this pin, so 510 of them and the two the tokenizer adds
        // are the 512 the model accepts, and one more is not. Measured by
        // bisection: 511 is the first word count refused. What decides is the
        // tokenizer's own truncation, so this file holds no threshold of its
        // own — this is what says the tokenizer's is the model's.
        let embed = embed();
        let fits = "kettle ".repeat(LIMIT - 2);
        let over = "kettle ".repeat(LIMIT - 1);
        assert!(embed.whole(fits.trim_end()).is_ok());
        assert!(embed.whole(over.trim_end()).is_err());
    }
}
