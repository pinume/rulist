use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::filesystem::DirectoryQuery;
use crate::server::{
    SharedState, SignedFileEntry, api_error, api_success, authenticate_user,
    normalize_request_path, signed_file_entry,
};

use super::{filesystem_error_response, signed_download_url, user_fs};

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct FsListReq {
    #[serde(default)]
    path: String,
    #[serde(default)]
    page: Option<usize>,
    #[serde(default)]
    per_page: Option<usize>,
    #[serde(default)]
    order_by: Option<String>,
    #[serde(default)]
    reverse: Option<bool>,
}
use super::PathReq;

#[derive(serde::Serialize)]
pub(crate) struct FsListResp {
    content: Vec<SignedFileEntry>,
    total: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DirItem {
    name: String,
    modified: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct FsLinkResp {
    url: String,
}

pub(crate) async fn list_handler(
    headers: HeaderMap,
    State(state): State<SharedState>,
    Json(req): Json<FsListReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "Authentication required");
    };
    let path = match normalize_request_path(&req.path) {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };

    let query = DirectoryQuery::new(
        req.order_by.as_deref(),
        req.reverse.unwrap_or(false),
        req.page,
        req.per_page,
    );

    match fs.list_query(&path, query).await {
        Ok(page) => {
            let total = page.total as i64;
            let content = match page
                .entries
                .into_iter()
                .map(|entry| {
                    let item_path = format!("{}/{}", path.trim_end_matches('/'), entry.name);
                    signed_file_entry(&state, &user, &item_path, entry)
                })
                .collect::<anyhow::Result<Vec<_>>>()
            {
                Ok(content) => content,
                Err(err) => {
                    tracing::error!(error = %err, "failed to sign file path");
                    return api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        500,
                        "Signing token is unavailable",
                    );
                }
            };

            let resp = FsListResp { content, total };
            api_success(resp)
        }
        Err(err) => {
            tracing::debug!(path = %path, "failed to list directory");
            filesystem_error_response(&err, "list")
        }
    }
}

pub(crate) async fn get_handler(
    headers: HeaderMap,
    State(state): State<SharedState>,
    Json(req): Json<PathReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "Authentication required");
    };
    let path = match normalize_request_path(&req.path) {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };

    match fs.get(&path).await {
        Ok(file) => match signed_file_entry(&state, &user, &path, file) {
            Ok(file) => api_success(file),
            Err(err) => {
                tracing::error!(error = %err, "failed to sign file path");
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    500,
                    "Signing token is unavailable",
                )
            }
        },
        Err(err) => {
            tracing::debug!(path = %path, "failed to get file");
            filesystem_error_response(&err, "get")
        }
    }
}

pub(crate) async fn dirs_handler(
    headers: HeaderMap,
    State(state): State<SharedState>,
    Json(req): Json<PathReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "Authentication required");
    };
    let path = normalize_request_path(&req.path);
    let path = match path {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };
    let files = match fs.list_snapshot(&path).await {
        Ok(f) => f,
        Err(err) => {
            tracing::debug!(path = %path, "failed to list dirs");
            return filesystem_error_response(&err, "list directories");
        }
    };

    let dirs: Vec<DirItem> = files
        .iter()
        .filter(|f| f.is_dir)
        .map(|f| DirItem {
            name: f.name.clone(),
            modified: f.modified.clone(),
        })
        .collect();

    api_success(dirs)
}

pub(crate) async fn link_handler(
    headers: HeaderMap,
    State(state): State<SharedState>,
    Json(req): Json<PathReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "Authentication required");
    };

    let clean_path = match normalize_request_path(&req.path) {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let _fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };
    let url = match signed_download_url(&state, &user, &clean_path) {
        Ok(url) => url,
        Err(err) => {
            tracing::error!(error = %err, "failed to sign file path");
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                500,
                "Signing token is unavailable",
            );
        }
    };
    api_success(FsLinkResp { url })
}
