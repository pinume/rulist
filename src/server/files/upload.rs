use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, ReadBuf};

use super::user_fs;
use crate::permissions::{OVERWRITE, WRITE_CONTENT};
use crate::server::stream::percent_decode;
use crate::server::{
    SharedState, api_error, api_success, authenticate_user, filesystem_error_response,
    normalize_request_path, permission_denied, permitted,
};

// HTTP frames are adapted to the filesystem's ordinary AsyncRead interface.
struct BodyReader {
    body: axum::body::Body,
    pending: axum::body::Bytes,
}

impl AsyncRead for BodyReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        use axum::body::HttpBody;
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        while self.pending.is_empty() {
            match std::task::ready!(Pin::new(&mut self.body).poll_frame(cx)) {
                Some(Ok(frame)) => {
                    if let Ok(bytes) = frame.into_data() {
                        self.pending = bytes;
                    }
                }
                Some(Err(error)) => return Poll::Ready(Err(std::io::Error::other(error))),
                None => return Poll::Ready(Ok(())),
            }
        }
        let len = buf.remaining().min(self.pending.len());
        buf.put_slice(&self.pending.split_to(len));
        Poll::Ready(Ok(()))
    }
}

pub(crate) async fn upload_handler(
    State(state): State<SharedState>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let Some(user) = authenticate_user(&headers, &state).await else {
        return api_error(StatusCode::UNAUTHORIZED, 401, "unauthorized");
    };
    let overwrite = headers.get("Overwrite").and_then(|h| h.to_str().ok()) == Some("true");
    if !permitted(&user, WRITE_CONTENT) || (overwrite && !permitted(&user, OVERWRITE)) {
        return permission_denied();
    }
    let path = match headers.get("File-Path").and_then(|h| h.to_str().ok()) {
        Some(path) => percent_decode(path),
        None => return api_error(StatusCode::BAD_REQUEST, 400, "missing File-Path header"),
    };
    let path = match normalize_request_path(&path) {
        Ok(path) => path,
        Err(error) => return filesystem_error_response(&error, "normalize upload path"),
    };
    let fs = match user_fs(&user) {
        Ok(fs) => fs,
        Err(_) => return api_error(StatusCode::NOT_FOUND, 404, "File root not found"),
    };
    let input = BodyReader {
        body: request.into_body(),
        pending: axum::body::Bytes::new(),
    };
    match fs.write(&path, input, overwrite).await {
        Ok(()) => api_success(serde_json::Value::Null),
        Err(error) => filesystem_error_response(&error, "upload"),
    }
}
