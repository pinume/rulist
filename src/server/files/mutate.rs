use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::filesystem::{BatchError, ConflictPolicy};
use crate::permissions::{COPY, DELETE, MOVE, OVERWRITE, RENAME, WRITE_CONTENT};
use crate::server::{
    SharedState, api_error, api_success, authenticate_user, normalize_request_path,
    permission_denied, permitted,
};

use super::{filesystem_error_details, filesystem_error_response, user_fs};

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct FsRenameReq {
    path: String,
    name: String,
    #[serde(default)]
    overwrite: bool,
}
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct FsDirNamesReq {
    #[serde(default)]
    dir: String,
    #[serde(default)]
    names: Vec<String>,
}
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct FsMoveCopyReq {
    #[serde(default)]
    src_dir: String,
    #[serde(default)]
    dst_dir: String,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    conflict_policy: ConflictPolicy,
}

use super::PathReq;

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct BatchRenameItem {
    src_name: String,
    new_name: String,
}
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct BatchRenameReq {
    src_dir: String,
    rename_objects: Vec<BatchRenameItem>,
}

pub(crate) async fn mkdir_handler(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<PathReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "unauthorized");
    };
    if !permitted(&user, WRITE_CONTENT) {
        return permission_denied();
    }
    if req.path.trim_matches('/').is_empty() {
        return api_error(
            StatusCode::BAD_REQUEST,
            400,
            "directory path cannot be empty",
        );
    }
    let path = match normalize_request_path(&req.path) {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };
    match fs.mkdir(&path).await {
        Ok(_) => api_success(serde_json::Value::Null),
        Err(err) => {
            tracing::debug!(path = %path, "failed to create directory");
            filesystem_error_response(&err, "mkdir")
        }
    }
}

pub(crate) async fn rename_handler(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<FsRenameReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "unauthorized");
    };
    if !permitted(&user, RENAME) || (req.overwrite && !permitted(&user, OVERWRITE)) {
        return permission_denied();
    }
    let path = match normalize_request_path(&req.path) {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };

    match fs.rename_safe(&path, &req.name, req.overwrite).await {
        Ok(_) => api_success(serde_json::Value::Null),
        Err(err) => filesystem_error_response(&err, "rename"),
    }
}

fn batch_response(
    result: Result<usize, BatchError>,
    action: &'static str,
    past: &'static str,
) -> Response {
    match result {
        Ok(_) => api_success(serde_json::Value::Null),
        Err(error) => match error.path {
            Some(path) => {
                let (status, code, _) = filesystem_error_details(&error.cause, action);
                api_error(
                    status,
                    code,
                    format!(
                        "{action} failed for {path}; {} item(s) already {past}",
                        error.completed
                    ),
                )
            }
            None => filesystem_error_response(&error.cause, action),
        },
    }
}

pub(crate) async fn move_handler(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<FsMoveCopyReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "unauthorized");
    };
    if !permitted(&user, MOVE)
        || (req.conflict_policy == ConflictPolicy::Overwrite && !permitted(&user, OVERWRITE))
    {
        return permission_denied();
    }
    let (src_dir, dst_dir) = match (
        normalize_request_path(&req.src_dir),
        normalize_request_path(&req.dst_dir),
    ) {
        (Ok(src), Ok(dst)) => (src, dst),
        (Err(err), _) | (_, Err(err)) => {
            return filesystem_error_response(&err, "normalize path");
        }
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };

    batch_response(
        fs.move_many(&src_dir, &dst_dir, &req.names, req.conflict_policy)
            .await,
        "Move",
        "moved",
    )
}

pub(crate) async fn copy_handler(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<FsMoveCopyReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "unauthorized");
    };
    if !permitted(&user, COPY)
        || (req.conflict_policy == ConflictPolicy::Overwrite && !permitted(&user, OVERWRITE))
    {
        return permission_denied();
    }
    let (src_dir, dst_dir) = match (
        normalize_request_path(&req.src_dir),
        normalize_request_path(&req.dst_dir),
    ) {
        (Ok(src), Ok(dst)) => (src, dst),
        (Err(err), _) | (_, Err(err)) => {
            return filesystem_error_response(&err, "normalize path");
        }
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };

    batch_response(
        fs.copy_many(&src_dir, &dst_dir, &req.names, req.conflict_policy)
            .await,
        "Copy",
        "copied",
    )
}

pub(crate) async fn remove_handler(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<FsDirNamesReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "unauthorized");
    };
    if !permitted(&user, DELETE) {
        return permission_denied();
    }
    let dir = match normalize_request_path(&req.dir) {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };
    batch_response(fs.remove_many(&dir, &req.names).await, "Delete", "deleted")
}

pub(crate) async fn batch_rename_handler(
    headers: HeaderMap,
    State(state): State<SharedState>,
    Json(req): Json<BatchRenameReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "Authentication required");
    };
    if !permitted(&user, RENAME) {
        return permission_denied();
    }
    let src_dir = match normalize_request_path(&req.src_dir) {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };

    let pairs: Vec<(String, String)> = req
        .rename_objects
        .into_iter()
        .map(|o| (o.src_name, o.new_name))
        .collect();

    match fs.batch_rename(&src_dir, &pairs).await {
        Ok(_) => api_success(()),
        Err(err) => filesystem_error_response(&err, "batch rename"),
    }
}
