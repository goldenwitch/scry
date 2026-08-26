# PDF ingestion

Status: design required. This document records current facts, requested
outcomes, and questions. It does not choose an implementation.

## Current facts

- `add` accepts an origin that is currently a local path or an `http://` or
  `https://` URL.
- The fetcher reads bytes and hands text to slicing and embedding.
- Bytes that are not valid UTF-8 are refused as `NotText`.
- A handle currently contains an origin, byte span, and digest of the text at
  that span.
- Provenance currently returns the origin, byte span, fetch time, and TTL.
- A direct PDF URL therefore does not currently provide an indexable text
  origin.

## Requested outcomes

- A user can add a PDF without a separate extraction workflow being visible in
  the normal harness path.
- Searchable content retains the source information needed to inspect the
  original PDF.
- Text and source locations used in an answer remain verifiable.
- The representation does not silently discard required PDF content.
- The design covers local files, remote URLs, and local URI forms used for
  network share origins.

## Questions to answer

- Does lossless mean preservation of the original PDF bytes, preservation of
  all extractable text and structure, or both?
- Where are the original bytes held, and how are they identified across a
  refresh?
- How does extracted text map to page numbers, text spans, and layout
  coordinates?
- How are images, tables, annotations, attachments, encrypted documents, and
  scanned pages represented?
- How is OCR distinguished from text extracted from the PDF, and what evidence
  is retained for OCR-derived text?
- Does a PDF handle keep the current byte-span form, or does provenance need a
  page-aware projection in addition to it?
- Which local URI forms identify a network share on each supported platform?
- What corpus size, indexing time, storage size, and search latency define the
  Analyst operating regime?
- Which PDF libraries and runtime assets can be shipped under the repository's
  dependency and platform constraints?

No PDF implementation or public contract change should be inferred from this
brief until these questions have an owner and a recorded decision.
