//! Why a network exchange failed, worded so the UI can be honest about it.
//!
//! Every reqwest error, unreadable answer and unrecognised failure becomes an
//! `Error` here, and each constructor writes the whole cause chain to
//! `lucerna.log`: reqwest's own `Display` stops at "error sending request for
//! url (…)", and the cause (name lookup, refused, TLS, timeout) sits in
//! `source()`. `NetworkFailure::Other` may be produced nowhere else
//! (`structural_network_failure_other.rs`): the UI sends the user to Logs for
//! it, so it must always come with a line there.
//!
//! A non-2xx status is not built here — see `Error::http_status`.

use crate::error::{Error, NetworkFailure};
use std::error::Error as StdError;
use std::io::ErrorKind;

type Cause<'a> = &'a (dyn StdError + 'static);

/// `e` and every `source()` below it, outermost first.
fn chain_of(e: Cause<'_>) -> Vec<Cause<'_>> {
    let mut out = vec![e];
    let mut next = e.source();
    while let Some(cause) = next {
        out.push(cause);
        next = cause.source();
    }
    out
}

/// Every I/O error kind in the chain, including the ones nested INSIDE an
/// `io::Error`. `io::Error::source()` skips its own payload (it returns the
/// payload's source), and hyper-rustls wraps the handshake's
/// `io::Error(InvalidData)` in `io::Error::other` (hyper-rustls 0.27
/// `connector.rs`), so the kind that names a TLS failure is reachable only
/// through `get_ref()`.
fn io_kinds(chain: &[Cause<'_>]) -> Vec<ErrorKind> {
    let mut kinds = Vec::new();
    for cause in chain {
        let mut io = cause.downcast_ref::<std::io::Error>();
        while let Some(e) = io {
            kinds.push(e.kind());
            io = e
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<std::io::Error>());
        }
    }
    kinds
}

/// Decide the failure from reqwest's flags and the cause chain. Pure, so the
/// order of the checks is pinned by unit tests without a network.
///
/// A timeout wins: a connect timeout is still "did not answer in time". A
/// connect failure is TLS when the chain holds an `InvalidData` I/O error —
/// tokio-rustls reports every rustls handshake failure that way, and reqwest's
/// connector sets up TLS inside connect — and unreachable otherwise (a peer
/// that hangs up mid-handshake included: nothing says it was TLS's fault). A
/// reset or an early end of the answer is an interruption: reqwest reports a
/// body-read failure as a decode error, so the I/O kind is what tells it. Any
/// other failure is not recognised.
pub(crate) fn classify(is_timeout: bool, is_connect: bool, chain: &[Cause<'_>]) -> NetworkFailure {
    if is_timeout {
        return NetworkFailure::Timeout;
    }
    let kinds = io_kinds(chain);
    if is_connect {
        return if kinds.contains(&ErrorKind::InvalidData) {
            NetworkFailure::Tls
        } else {
            NetworkFailure::Unreachable
        };
    }
    let dropped = kinds.iter().any(|k| {
        matches!(
            k,
            ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::BrokenPipe
                | ErrorKind::UnexpectedEof
        )
    });
    if dropped {
        return NetworkFailure::Interrupted;
    }
    NetworkFailure::Other
}

/// Every level of the chain, `": "`-joined; a level that repeats the one above
/// it verbatim is dropped.
pub(crate) fn describe_chain(chain: &[Cause<'_>]) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(chain.len());
    for cause in chain {
        let text = cause.to_string();
        if parts.last() != Some(&text) {
            parts.push(text);
        }
    }
    parts.join(": ")
}

/// A reqwest failure (send, body read, stream chunk) as a typed, logged
/// `Error::Network`. `action` is the verb for the log line (`GET`, `POST`,
/// `download`).
pub(crate) fn transport(initiator: &str, action: &str, url: &str, e: reqwest::Error) -> Error {
    let chain = chain_of(&e);
    let failure = classify(e.is_timeout(), e.is_connect(), &chain);
    let details = describe_chain(&chain);
    crate::diag!("network: {initiator} {action} {url} — {failure:?}: {details}");
    Error::Network {
        url: url.to_string(),
        failure,
        details,
    }
}

/// The site answered 2xx, but the body could not be read as what was asked
/// for (JSON, UTF-8).
pub(crate) fn bad_response(initiator: &str, url: &str, details: String) -> Error {
    crate::diag!("network: {initiator} {url} — unreadable answer: {details}");
    Error::Network {
        url: url.to_string(),
        failure: NetworkFailure::BadResponse,
        details,
    }
}

/// A failure this module cannot name. Logged, so the UI's pointer to Logs
/// holds.
pub(crate) fn unexpected(initiator: &str, url: &str, details: &str) -> Error {
    crate::diag!("network: {initiator} {url} — {details}");
    Error::Network {
        url: url.to_string(),
        failure: NetworkFailure::Other,
        details: details.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    /// A chain level that is not an I/O error, with an optional cause below.
    #[derive(Debug)]
    struct Layer(&'static str, Option<std::io::Error>);

    impl fmt::Display for Layer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl StdError for Layer {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            self.1.as_ref().map(|e| e as &(dyn StdError + 'static))
        }
    }

    fn io(kind: ErrorKind, text: &'static str) -> std::io::Error {
        std::io::Error::new(kind, text)
    }

    /// The shape a failed TLS handshake really has: tokio-rustls turns the
    /// rustls error into `io::Error(InvalidData)`, and hyper-rustls wraps THAT
    /// in `io::Error::other` — whose `source()` skips its own payload.
    fn tls_handshake_failure() -> Layer {
        Layer(
            "client error (Connect)",
            Some(std::io::Error::other(io(
                ErrorKind::InvalidData,
                "invalid peer certificate: UnknownIssuer",
            ))),
        )
    }

    #[test]
    fn a_timeout_wins_over_a_connect_failure() {
        let tls = tls_handshake_failure();
        assert_eq!(
            classify(true, true, &chain_of(&tls)),
            NetworkFailure::Timeout
        );
    }

    #[test]
    fn a_tls_handshake_failure_is_tls_though_hyper_rustls_wraps_it() {
        let tls = tls_handshake_failure();
        assert_eq!(classify(false, true, &chain_of(&tls)), NetworkFailure::Tls);
    }

    #[test]
    fn a_connect_failure_without_invalid_data_is_unreachable() {
        let refused = Layer(
            "tcp connect error",
            Some(io(ErrorKind::ConnectionRefused, "refused")),
        );
        assert_eq!(
            classify(false, true, &chain_of(&refused)),
            NetworkFailure::Unreachable
        );
        let dns = Layer("dns error", None);
        assert_eq!(
            classify(false, true, &chain_of(&dns)),
            NetworkFailure::Unreachable
        );
        // A peer that hangs up mid-handshake: nothing says it was TLS's fault.
        let hung_up = Layer(
            "client error (Connect)",
            Some(std::io::Error::other(io(
                ErrorKind::UnexpectedEof,
                "tls handshake eof",
            ))),
        );
        assert_eq!(
            classify(false, true, &chain_of(&hung_up)),
            NetworkFailure::Unreachable
        );
    }

    #[test]
    fn a_reset_or_an_early_end_of_the_answer_is_an_interruption() {
        for kind in [
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::BrokenPipe,
            ErrorKind::UnexpectedEof,
        ] {
            let e = Layer("error decoding response body", Some(io(kind, "cut")));
            assert_eq!(
                classify(false, false, &chain_of(&e)),
                NetworkFailure::Interrupted,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn invalid_data_outside_connect_is_not_called_tls() {
        let e = Layer(
            "error decoding response body",
            Some(io(ErrorKind::InvalidData, "bad chunk size")),
        );
        assert_eq!(classify(false, false, &chain_of(&e)), NetworkFailure::Other);
    }

    #[test]
    fn the_chain_keeps_every_level_and_drops_a_repeat() {
        let tls = tls_handshake_failure();
        assert_eq!(
            describe_chain(&chain_of(&tls)),
            "client error (Connect): invalid peer certificate: UnknownIssuer"
        );
        let same = Layer("cut", Some(io(ErrorKind::Other, "cut")));
        assert_eq!(describe_chain(&chain_of(&same)), "cut");
    }
}
