//! What the fixture records: every request, the connections and the counters pacing tests assert on.

use std::collections::BTreeSet;

use bytes::Bytes;
use rustls::ProtocolVersion;

use super::route::Route;

/// The fault the fixture applied (or will apply) to one answer, at a byte count of the response body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedFault {
    pub kind: &'static str,
    pub at_byte: usize,
}

/// One request as received. `host` is not recorded: the port differs between runs, the rest does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggedRequest {
    pub seq: u64,
    pub connection: u64,
    pub route: Option<Route>,
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
    /// False when the request matched no scripted answer and got the "unscripted" problem.
    pub scripted: bool,
    pub answered: Option<u16>,
    pub fault: Option<AppliedFault>,
}

impl LoggedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Block index of a block `PUT` (`.../blocks/{index}`).
    pub fn block_index(&self) -> Option<u32> {
        let path = self.target.split('?').next()?;
        path.rsplit_once("/blocks/")?.1.parse().ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnInfo {
    pub tls_version: Option<ProtocolVersion>,
    pub alpn: Option<String>,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub connections: Vec<ConnInfo>,
    pub handshake_failures: Vec<String>,
    pub connection_errors: Vec<String>,
    pub request_errors: Vec<String>,
    pub block_puts_in_flight: u32,
    pub max_block_puts_in_flight: u32,
    pub staged_uncommitted: BTreeSet<String>,
    pub max_staged_uncommitted: usize,
}

/// Decrements the in-flight block counter when the response has ended or the request failed.
pub struct InFlightBlock(pub std::sync::Arc<super::server::Shared>);

impl Drop for InFlightBlock {
    fn drop(&mut self) {
        self.0.stats().block_puts_in_flight -= 1;
    }
}
