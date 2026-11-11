//! The content-serving header mapper (E-21/c10, IT-11-14 and the stream part of IT-11-20): a stub
//! `DownloadService` behind a route that answers through `content_response`.

mod abort;
mod errors;
mod headers;
mod ranges;
pub mod support;
