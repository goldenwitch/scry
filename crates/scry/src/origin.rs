//! `origin`: a path or a url, and the identity of a document.

use core::fmt;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// The two schemes `fetch` reads over the network. A string naming any other
/// scheme is not a url scry can read, so it is not a url here.
const HTTP: &str = "http";
const HTTPS: &str = "https";

/// A path or a url: the document's identity.
///
/// Adding the same origin replaces the document, and `delete` names the same
/// key `add` did, so every verb that speaks of a document speaks of one of
/// these. [`Origin::parse`] is the only constructor, so the normalisation is
/// not something a caller can route around: two spellings of one document
/// arrive here as one key, or they never meet.
///
/// Normalisation is idempotent, and [`Display`](fmt::Display) writes a
/// spelling that parses back to the same origin, so an origin read out of the
/// store re-enters through the same constructor and lands where it started.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Kind {
    Path(PathBuf),
    Url(String),
}

impl Origin {
    /// Parses a path or a url into the key every verb uses, or `None` if the
    /// input names no document.
    ///
    /// A string beginning `http://` or `https://`, in any case, is a url;
    /// everything else is a path, so a scheme and a drive letter are never
    /// confused for one another.
    ///
    /// A url is normalised by scheme, host, default port, empty path and
    /// fragment, so `HTTPS://Example.COM:443/Doc#top` and
    /// `https://example.com/Doc` are one document. A query is left alone,
    /// since it changes which bytes come back.
    ///
    /// A path is made absolute against the current directory and rebuilt from
    /// its components, so the spellings of one file are one key wherever scry
    /// is run from.
    ///
    /// `None` says the input names nothing scry could hold: it is empty, it is
    /// a url with no host, it is a url carrying credentials — which are not
    /// part of a document's identity and would otherwise be written to the
    /// store — or the current directory could not be read, so a relative path
    /// has no absolute spelling.
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        match scheme_of(input) {
            Some(scheme) => normalise_url(scheme, input).map(|url| Self {
                kind: Kind::Url(url),
            }),
            None => normalise_path(input).map(|path| Self {
                kind: Kind::Path(path),
            }),
        }
    }

    /// The path, if this origin is one.
    #[must_use]
    pub fn as_path(&self) -> Option<&Path> {
        match &self.kind {
            Kind::Path(path) => Some(path),
            Kind::Url(_) => None,
        }
    }

    /// The url, if this origin is one.
    #[must_use]
    pub fn as_url(&self) -> Option<&str> {
        match &self.kind {
            Kind::Url(url) => Some(url),
            Kind::Path(_) => None,
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            // Lossless: `parse` refuses a path that is not UTF-8, so this
            // spelling is the path itself and parses back to it.
            Kind::Path(path) => write!(f, "{}", path.display()),
            Kind::Url(url) => write!(f, "{url}"),
        }
    }
}

impl Ord for Origin {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_string().cmp(&other.to_string())
    }
}

impl PartialOrd for Origin {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The scheme, if the input names one scry fetches. Anything else — including
/// a Windows drive letter, which carries no `//` — is a path.
fn scheme_of(input: &str) -> Option<&'static str> {
    let (scheme, _) = input.split_once("://")?;
    if scheme.eq_ignore_ascii_case(HTTPS) {
        Some(HTTPS)
    } else if scheme.eq_ignore_ascii_case(HTTP) {
        Some(HTTP)
    } else {
        None
    }
}

/// Syntax-based normalisation of a url, and nothing that re-encodes what the
/// caller wrote: percent-encoding is left byte for byte, since a server is
/// free to read `%2F` and `/` as different things.
fn normalise_url(scheme: &'static str, input: &str) -> Option<String> {
    let (_, rest) = input.split_once("://")?;
    // A fragment names a place inside a document and is never sent, so two
    // urls differing only there name one document.
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(at) => rest.split_at_checked(at)?,
        None => (rest, ""),
    };
    let authority = normalise_authority(scheme, authority)?;
    let (path, query) = match tail.split_once('?') {
        Some((path, query)) => (path, query),
        None => (tail, ""),
    };
    let path = if path.is_empty() { "/" } else { path };
    let mut url = format!("{scheme}://{authority}{path}");
    if !query.is_empty() {
        url.push('?');
        url.push_str(query);
    }
    Some(url)
}

/// Host lowercased, default and empty ports dropped, credentials refused.
fn normalise_authority(scheme: &str, authority: &str) -> Option<String> {
    if authority.contains('@') {
        return None;
    }
    let (host, port) = split_host_port(authority)?;
    if host.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let Some(port) = port else {
        return Some(host);
    };
    if port.is_empty() || port == default_port(scheme) {
        return Some(host);
    }
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!("{host}:{port}"))
}

/// Splits `host:port`, keeping the brackets of an IPv6 literal on the host so
/// the colons inside it are not read as a port.
///
/// A bracket that opens and never closes is no authority at all: read as a
/// bare host it would split at its last colon, so `[::1` would come back as
/// the host `[:` with the port `1`, and be written out again as a spelling
/// that parses to something else. `None` says the input named no document,
/// which is what an unclosed bracket does.
fn split_host_port(authority: &str) -> Option<(&str, Option<&str>)> {
    let Some(inside) = authority.strip_prefix('[') else {
        return Some(match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        });
    };
    let close = inside.find(']').filter(|end| *end > 0)? + "[]".len();
    let (host, rest) = authority.split_at_checked(close)?;
    if rest.is_empty() {
        return Some((host, None));
    }
    rest.strip_prefix(':').map(|port| (host, Some(port)))
}

fn default_port(scheme: &str) -> &'static str {
    if scheme == HTTPS { "443" } else { "80" }
}

/// Absolute against the current directory, then rebuilt from its components,
/// which drops `.`, collapses repeated separators, drops a trailing one, and
/// writes the platform's separator throughout.
///
/// The filesystem is not consulted: an origin has to normalise before `fetch`
/// can report that there is nothing at it, and a symlink is not resolved
/// because normalising a name must not change which file the name reaches.
fn normalise_path(input: &str) -> Option<PathBuf> {
    if input.is_empty() {
        return None;
    }
    let absolute = std::path::absolute(input).ok()?;
    let normalised: PathBuf = absolute.components().collect();
    // A path that is not UTF-8 has no lossless spelling, and an origin is kept
    // as its spelling, so it is refused rather than mangled.
    normalised.to_str()?;
    Some(normalised)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::Origin;

    fn key(input: &str) -> Origin {
        let Some(origin) = Origin::parse(input) else {
            unreachable!()
        };
        origin
    }

    #[test]
    fn paths_that_differ_only_in_normalisation_are_one_key() {
        let Ok(cwd) = std::env::current_dir() else {
            unreachable!()
        };
        let absolute = cwd.join("docs").join("a.md");
        let Some(absolute) = absolute.to_str() else {
            unreachable!()
        };
        let one = key("docs/a.md");
        for spelling in [
            "./docs/a.md",
            "docs//a.md",
            "docs/./a.md",
            "docs/a.md/",
            absolute,
        ] {
            assert_eq!(key(spelling), one, "{spelling}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_reads_both_separators_and_resolves_a_parent() {
        let one = key("docs/a.md");
        assert_eq!(key(r"docs\a.md"), one);
        assert_eq!(key(r".\docs\a.md"), one);
        // `std::path::absolute` resolves `..` on Windows and leaves it on
        // Unix, so this is claimed only where it is true.
        assert_eq!(key("docs/other/../a.md"), one);
    }

    #[test]
    fn urls_that_differ_only_in_normalisation_are_one_key() {
        let one = key("https://example.com/Doc");
        for spelling in [
            "HTTPS://example.com/Doc",
            "https://Example.COM/Doc",
            "https://example.com:443/Doc",
            "https://example.com:/Doc",
            "https://example.com/Doc#section",
            "https://example.com/Doc#",
        ] {
            assert_eq!(key(spelling), one, "{spelling}");
        }
    }

    #[test]
    fn a_url_naming_no_path_names_the_root() {
        let one = key("http://example.com/");
        for spelling in [
            "http://example.com",
            "http://example.com:80",
            "http://example.com?",
            "http://example.com#top",
        ] {
            assert_eq!(key(spelling), one, "{spelling}");
        }
    }

    #[test]
    fn a_query_names_a_different_document() {
        assert_ne!(
            key("https://example.com/d?v=2"),
            key("https://example.com/d")
        );
    }

    #[test]
    fn a_port_that_is_not_the_default_is_part_of_the_key() {
        assert_ne!(
            key("https://example.com:8443/d"),
            key("https://example.com/d")
        );
    }

    #[test]
    fn an_ipv6_host_keeps_its_colons() {
        assert_eq!(key("http://[::1]:80/d"), key("http://[::1]/d"));
        assert_ne!(key("http://[::1]:8080/d"), key("http://[::1]/d"));
    }

    #[test]
    fn a_bracket_that_never_closes_is_not_an_origin() {
        // Read as a bare host, `[::1` splits at its last colon and comes back
        // as the host `[:` with the port `1`, which is a key whose spelling
        // names a different place from the one that was asked for.
        assert_eq!(Origin::parse("http://[::1"), None);
        assert_eq!(Origin::parse("http://[::1/doc"), None);
        assert_eq!(Origin::parse("http://[::1?q=1"), None);
        assert_eq!(Origin::parse("https://[]/doc"), None);
    }

    #[test]
    fn nothing_at_all_is_not_an_origin() {
        assert_eq!(Origin::parse(""), None);
    }

    #[test]
    fn a_url_without_a_host_is_not_an_origin() {
        assert_eq!(Origin::parse("https://"), None);
        assert_eq!(Origin::parse("http:///doc"), None);
    }

    #[test]
    fn a_url_carrying_credentials_is_not_an_origin() {
        assert_eq!(Origin::parse("https://user:token@example.com/doc"), None);
    }

    #[test]
    fn a_scheme_scry_does_not_fetch_is_not_a_url() {
        assert!(
            Origin::parse("ftp://example.com/doc").is_none_or(|origin| origin.as_url().is_none())
        );
    }

    #[test]
    fn a_path_and_a_url_are_never_the_same_key() {
        assert_ne!(key("https://example.com/d"), key("https/example.com/d"));
    }

    #[test]
    fn an_origin_read_back_lands_where_it_started() {
        for spelling in [
            "docs/a.md",
            "https://example.com/Doc?v=2",
            "http://example.com",
            "http://[::1]:8080/d",
        ] {
            let origin = key(spelling);
            assert_eq!(
                Origin::parse(&origin.to_string()),
                Some(origin),
                "{spelling}"
            );
        }
    }

    #[test]
    fn a_set_iterates_in_canonical_spelling_order() {
        let mut origins = BTreeSet::new();
        origins.insert(key("z.md"));
        origins.insert(key("https://example.com/doc"));
        origins.insert(key("a.md"));

        let actual = origins.iter().map(ToString::to_string).collect::<Vec<_>>();
        let mut expected = actual.clone();
        expected.sort();
        assert_eq!(actual, expected);
    }

    #[test]
    fn normalized_equivalent_origins_are_one_set_member() {
        let mut origins = BTreeSet::new();
        origins.insert(key("HTTPS://Example.COM:443/doc#section"));
        origins.insert(key("https://example.com/doc"));
        assert_eq!(origins.len(), 1);
    }
}
