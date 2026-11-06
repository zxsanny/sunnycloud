//! The `/api/v1` routes `sync-core` calls (`api_v1_surface.md`), identified by method and path template.

use hyper::Method;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    ServerInfo,
    RootNode,
    GetNode,
    ListChildren,
    CreateNode,
    UpdateNode,
    DeleteNode,
    GetRevision,
    LookupRevisions,
    RevisionContent,
    StartUpload,
    GetUpload,
    PutBlock,
    StagedUpload,
    Commit,
    BatchCommit,
    Changes,
    ChangesHead,
    DeviceGrants,
    DeviceTokens,
    MobileCodeExchange,
    RefreshTokens,
    SignOut,
}

/// The route a request addresses, or `None` for a path or method outside the list.
pub fn resolve(method: &Method, path: &str) -> Option<Route> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let ["api", "v1", rest @ ..] = segments.as_slice() else {
        return None;
    };
    let route = match (method.as_str(), rest) {
        ("GET", ["server-info"]) => Route::ServerInfo,
        ("GET", ["nodes", "root"]) => Route::RootNode,
        ("POST", ["nodes"]) => Route::CreateNode,
        ("GET", ["nodes", _]) => Route::GetNode,
        ("PATCH", ["nodes", _]) => Route::UpdateNode,
        ("DELETE", ["nodes", _]) => Route::DeleteNode,
        ("GET", ["nodes", _, "children"]) => Route::ListChildren,
        ("GET", ["revisions", _]) => Route::GetRevision,
        ("GET", ["revisions", _, "content"]) => Route::RevisionContent,
        ("POST", ["revisions:lookup"]) => Route::LookupRevisions,
        ("POST", ["uploads"]) => Route::StartUpload,
        ("GET", ["uploads", _]) => Route::GetUpload,
        ("PUT", ["uploads", _, "blocks", _]) => Route::PutBlock,
        ("PUT", ["staged-uploads", _]) => Route::StagedUpload,
        ("POST", ["commits"]) => Route::Commit,
        ("POST", ["commits:batch"]) => Route::BatchCommit,
        ("GET", ["changes"]) => Route::Changes,
        ("GET", ["changes", "head"]) => Route::ChangesHead,
        ("POST", ["device-grants"]) => Route::DeviceGrants,
        ("POST", ["device-tokens"]) => Route::DeviceTokens,
        ("POST", ["auth", "mobile-code-exchanges"]) => Route::MobileCodeExchange,
        ("POST", ["auth", "refresh-tokens"]) => Route::RefreshTokens,
        ("DELETE", ["devices", "me"]) => Route::SignOut,
        _ => return None,
    };
    Some(route)
}
