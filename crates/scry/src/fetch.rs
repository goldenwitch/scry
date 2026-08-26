//! `fetch`: an origin read into text, and the moment it was read.
//!
//! This is the only place an origin becomes bytes, the only place the network
//! is spoken to, and the only place `fetched_at` is taken. scry does the
//! fetching so an agent does not: what leaves here is a document's text or one
//! of two refusals, never a status code, a timeout, or an errno for someone
//! else to interpret.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::origin::Origin;
use crate::refusal::AddRefusal;
use crate::text::Text;

/// The most bytes scry reads from one origin.
///
/// A read has to be bounded or a url can end the process instead of answering,
/// and the two readers have to be bounded alike or the same document would
/// answer differently from a path and from a url. 64 MiB is far above any text
/// document — a long book is about one — and its embeddings are half as much
/// again in memory, so the number is a bound on damage rather than a policy
/// about size.
const CAP: u64 = 64 * 1024 * 1024;

/// The longest a url may take.
///
/// `add` has to return. Without a bound, a server that accepts a connection
/// and then says nothing is a hang an agent holds, which is exactly what this
/// part exists to prevent. Five minutes against [`CAP`] is 1.8 Mbit/s for the
/// largest thing scry will read, and thousandths of that for a real document.
const TIMEOUT: Duration = Duration::from_secs(300);

/// Reads what is at `origin`, and the moment it was read.
///
/// A path is read with the standard library and a url with ureq — both
/// synchronous, so no async runtime enters for the sake of one call.
///
/// # Errors
///
/// [`NotFound`](AddRefusal::NotFound) when no bytes arrive, whatever stopped
/// them: an absent file, a refused connection, a 404, a redirect that never
/// lands, a server that will not answer inside [`TIMEOUT`].
///
/// [`NotText`](AddRefusal::NotText) when bytes arrive that scry will not
/// index: they are not UTF-8, or there are more of them than [`CAP`].
pub(crate) fn fetch(origin: &Origin) -> Result<(Text, SystemTime), AddRefusal> {
    read(origin, CAP, TIMEOUT)
}

/// The whole of `fetch`, with the two bounds as arguments so a test can state
/// one small enough to reach.
fn read(origin: &Origin, cap: u64, timeout: Duration) -> Result<(Text, SystemTime), AddRefusal> {
    // Taken before the read rather than after it, so the moment never claims
    // the material is newer than it is: the bytes are at least as new as this.
    let fetched_at = SystemTime::now();
    let bytes = match origin.as_url() {
        Some(url) => over_the_network(url, cap, timeout),
        // An origin is a path or a url, so there is no third case here, and a
        // third case would be no bytes — which is what this branch says.
        None => origin.as_path().and_then(|path| from_the_disk(path, cap)),
    };
    let Some(bytes) = bytes else {
        return Err(AddRefusal::NotFound(origin.clone()));
    };
    // One byte past the cap is what both readers stop at, so this one check
    // answers for both of them.
    if bytes.len() as u64 > cap {
        return Err(AddRefusal::NotText(origin.clone()));
    }
    let Some(text) = Text::from_utf8(bytes) else {
        return Err(AddRefusal::NotText(origin.clone()));
    };
    Ok((text, fetched_at))
}

/// The bytes at a path, or `None` if they do not arrive.
///
/// A directory, a file that will not open, and a read that fails partway are
/// all the same answer: there are no bytes here to index.
fn from_the_disk(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(cap + 1).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The bytes at a url, or `None` if they do not arrive.
///
/// The reader is bounded by [`Read::take`] rather than by ureq's own limit,
/// which refuses the read instead of stopping it — a path stops, so a url
/// stops, and the one byte past the cap is what the caller sees in both cases.
///
/// Nothing here reads the response's headers. A charset is a conversion scry
/// does not do, so bytes that are not UTF-8 are refused whatever the server
/// calls them; a content type is the server's opinion of bytes scry has its
/// own test for. Redirects are followed, and the origin stays the url the
/// agent named rather than the one that answered.
fn over_the_network(url: &str, cap: u64, timeout: Duration) -> Option<Vec<u8>> {
    let mut response = ureq::get(url)
        .config()
        .timeout_global(Some(timeout))
        .build()
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicU64, Ordering};
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use super::{CAP, TIMEOUT, fetch, read};
    use crate::origin::Origin;
    use crate::refusal::AddRefusal;
    use crate::scaffold::origin;

    /// Distinguishes the scratch files of concurrent tests.
    static SCRATCH: AtomicU64 = AtomicU64::new(0);

    /// A directory of this test's own, under the platform's temporary one.
    fn directory() -> PathBuf {
        let unique = SCRATCH.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("scry-fetch-{}-{unique}", std::process::id()));
        match std::fs::create_dir_all(&directory) {
            Ok(()) => directory,
            Err(error) => unreachable!("{error}"),
        }
    }

    /// A file holding `bytes`, and the origin naming it.
    fn file(bytes: &[u8]) -> Origin {
        let path = directory().join("doc");
        match std::fs::write(&path, bytes) {
            Ok(()) => {}
            Err(error) => unreachable!("{error}"),
        }
        let Some(path) = path.to_str() else {
            unreachable!()
        };
        origin(path)
    }

    /// A loopback server that answers one request with `response`, so the url
    /// path is measured rather than assumed, and measured offline.
    fn serve(response: &'static [u8]) -> Origin {
        listen(move |stream| {
            let _ = stream.write_all(response);
            let _ = stream.flush();
        })
    }

    /// A loopback server that accepts a connection and then says nothing.
    fn silence() -> Origin {
        listen(|_| std::thread::sleep(Duration::from_secs(2)))
    }

    fn listen(answer: impl FnOnce(&mut TcpStream) + Send + 'static) -> Origin {
        let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
            unreachable!()
        };
        let Ok(address) = listener.local_addr() else {
            unreachable!()
        };
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            request(&mut stream);
            answer(&mut stream);
        });
        origin(&format!("http://{address}/doc"))
    }

    /// Reads the request, so the response is not written into a socket the
    /// other side is still filling.
    fn request(stream: &mut TcpStream) {
        use std::io::Read as _;

        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while let Ok(1) = stream.read(&mut byte) {
            request.push(byte[0]);
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
    }

    fn text(origin: &Origin) -> String {
        match fetch(origin) {
            Ok((text, _)) => text.as_str().to_owned(),
            Err(refusal) => unreachable!("{refusal}"),
        }
    }

    #[test]
    fn a_text_file_round_trips() {
        let origin = file("one two three".as_bytes());
        assert_eq!(text(&origin), "one two three");
    }

    #[test]
    fn an_empty_file_is_a_document_holding_nothing() {
        let origin = file(b"");
        assert_eq!(text(&origin), "");
    }

    #[test]
    fn bytes_that_are_not_text_are_refused_rather_than_replaced() {
        let origin = file(&[0xff, 0xfe, b'h', b'i']);
        assert_eq!(fetch(&origin), Err(AddRefusal::NotText(origin)));
    }

    #[test]
    fn nothing_at_that_path_is_not_found() {
        let origin = origin(&directory().join("absent").to_string_lossy());
        assert_eq!(fetch(&origin), Err(AddRefusal::NotFound(origin)));
    }

    #[test]
    fn a_directory_holds_no_document() {
        let origin = origin(&directory().to_string_lossy());
        assert_eq!(fetch(&origin), Err(AddRefusal::NotFound(origin)));
    }

    #[test]
    fn more_bytes_than_the_cap_are_refused() {
        let origin = file(b"12345678");
        assert_eq!(
            read(&origin, 4, TIMEOUT),
            Err(AddRefusal::NotText(origin.clone()))
        );
        // The same file is a document under the cap scry ships with, so it is
        // the bound that refused it and not the file.
        assert!(read(&origin, CAP, TIMEOUT).is_ok());
    }

    #[test]
    fn fetched_at_is_the_moment_the_read_happened() {
        let origin = file(b"now");
        let before = SystemTime::now();
        let Ok((_, fetched_at)) = fetch(&origin) else {
            unreachable!()
        };
        let after = SystemTime::now();
        assert!(fetched_at >= before);
        assert!(fetched_at <= after);
    }

    #[test]
    fn a_url_round_trips() {
        let origin = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\none two three");
        assert_eq!(text(&origin), "one two three");
    }

    #[test]
    fn a_url_that_answers_with_a_status_is_not_found() {
        let origin = serve(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(fetch(&origin), Err(AddRefusal::NotFound(origin)));
    }

    #[test]
    fn a_url_whose_bytes_are_not_text_is_refused() {
        let origin = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n\xff\xfe");
        assert_eq!(fetch(&origin), Err(AddRefusal::NotText(origin)));
    }

    #[test]
    fn a_url_answering_with_more_than_the_cap_is_refused() {
        let origin = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n12345678");
        assert_eq!(
            read(&origin, 4, TIMEOUT),
            Err(AddRefusal::NotText(origin.clone()))
        );
    }

    #[test]
    fn a_server_that_will_not_answer_is_not_found() {
        let origin = silence();
        assert_eq!(
            read(&origin, CAP, Duration::from_millis(100)),
            Err(AddRefusal::NotFound(origin.clone()))
        );
    }
}
