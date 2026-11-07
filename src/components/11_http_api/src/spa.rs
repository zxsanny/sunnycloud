use std::collections::BTreeMap;

use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::Response;

/// The bundled web app: its index and the static files next to it. The bundle is built outside this crate
/// and handed to [`crate::HttpServer::with_spa`].
#[derive(Debug, Clone)]
pub struct SpaBundle {
    index: Bytes,
    assets: BTreeMap<String, SpaAsset>,
}

#[derive(Debug, Clone)]
struct SpaAsset {
    content_type: &'static str,
    body: Bytes,
}

impl SpaBundle {
    pub fn new(index: impl Into<Bytes>) -> SpaBundle {
        SpaBundle {
            index: index.into(),
            assets: BTreeMap::new(),
        }
    }

    /// Adds a static file served at `path` (with its leading slash).
    pub fn with_asset(
        mut self,
        path: &str,
        content_type: &'static str,
        body: impl Into<Bytes>,
    ) -> SpaBundle {
        self.assets.insert(
            path.to_owned(),
            SpaAsset {
                content_type,
                body: body.into(),
            },
        );
        self
    }

    /// The file at `path`, or the index for any other path: the client router owns it.
    pub(crate) fn serve(&self, method: &Method, path: &str) -> Response {
        let (content_type, body) = match self.assets.get(path) {
            Some(asset) => (asset.content_type, &asset.body),
            None => ("text/html; charset=utf-8", &self.index),
        };
        let length = body.len();
        let body = if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(body.clone())
        };
        let mut response = Response::new(body);
        *response.status_mut() = StatusCode::OK;
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
        response
    }
}
