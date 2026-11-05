use serde::Serialize;
use serde_json::Value;

use crate::methods::MethodCall;
use crate::rpc::{Request, RequestId, Response, RpcError};

/// One message as one line: compact JSON (newlines inside strings are escaped) plus `\n`.
pub fn encode_line<T: Serialize>(message: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    Ok(line)
}

/// Splits a byte stream into lines, whatever the chunk boundaries are.
#[derive(Debug, Default)]
pub struct LineBuffer {
    pending: Vec<u8>,
}

impl LineBuffer {
    pub fn push(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
    }

    /// The next complete line without its `\n`, or `None` until more bytes arrive.
    pub fn next_line(&mut self) -> Option<Vec<u8>> {
        let end = self.pending.iter().position(|b| *b == b'\n')?;
        let mut line: Vec<u8> = self.pending.drain(..=end).collect();
        line.pop();
        Some(line)
    }
}

/// A request line decoded by the daemon.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub id: RequestId,
    pub call: MethodCall,
}

/// Decodes one request line. On failure the `Err` is the response to send back:
/// -32700 (not JSON), -32600 (JSON but not a request), -32601 (unknown method),
/// -32602 (params do not decode).
pub fn decode_call(line: &[u8]) -> Result<Call, Response> {
    let value: Value = serde_json::from_slice(line)
        .map_err(|e| Response::failure(None, RpcError::parse_error(e)))?;
    let id = value
        .get("id")
        .and_then(|id| serde_json::from_value::<RequestId>(id.clone()).ok());
    let request: Request = serde_json::from_value(value)
        .map_err(|e| Response::failure(id.clone(), RpcError::invalid_request(e)))?;
    MethodCall::decode(&request.method, request.params)
        .map(|call| Call {
            id: request.id.clone(),
            call,
        })
        .map_err(|e| Response::failure(Some(request.id), e))
}
