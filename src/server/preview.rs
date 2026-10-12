use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::filesystem::local::LocalFs;
use crate::preview::{PreviewError, PreviewReq, inspect_file};
use crate::server::{
    SharedState, api_error, api_success, authenticate_user, filesystem_error_response,
    normalize_request_path, signed_preview_url,
};

pub async fn preview_handler(
    headers: HeaderMap,
    State(state): State<SharedState>,
    Json(req): Json<PreviewReq>,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "Authentication required");
    };
    let path = match normalize_request_path(&req.path) {
        Ok(path) => path,
        Err(err) => return filesystem_error_response(&err, "normalize path"),
    };
    let fs = match LocalFs::new(&user.local_path, false) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };

    match inspect_file(&fs, &path, || {
        let (_, raw_url) = signed_preview_url(&state, &user, &path)?;
        Ok(raw_url)
    })
    .await
    {
        Ok(response) => api_success(response),
        Err(PreviewError::IsDirectory) => api_error(
            StatusCode::BAD_REQUEST,
            400,
            "Directory cannot be previewed",
        ),
        Err(PreviewError::Fs(err)) => filesystem_error_response(&err, "preview"),
    }
}
