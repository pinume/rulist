use std::path::Path;

use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio_util::io::ReaderStream;

use crate::db::get_user_by_id;
use crate::filesystem::local::LocalFs;
use crate::server::links::verify_user_link;
use crate::server::{SharedState, authenticate_user, encode_url_path};

#[derive(Debug, Deserialize)]
pub struct SignQuery {
    pub sign: Option<String>,
    pub uid: Option<i64>,
}

pub async fn raw_download_handler(
    State(state): State<SharedState>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<SignQuery>,
    headers: HeaderMap,
) -> Response {
    stream_file(state, path, query, headers, true).await
}

pub async fn raw_preview_handler(
    State(state): State<SharedState>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<SignQuery>,
    headers: HeaderMap,
) -> Response {
    stream_file(state, path, query, headers, false).await
}

#[derive(Debug, PartialEq, Eq)]
pub enum StreamError {
    InvalidPath,
    Unauthorized,
    InvalidSignature,
    ExpiredOrInvalidSignature,
    NotFound,
    IsDirectory,
    MetadataFailed,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamError::InvalidPath => write!(f, "Invalid path"),
            StreamError::Unauthorized => write!(f, "Unauthorized"),
            StreamError::InvalidSignature => write!(f, "Invalid download link signature"),
            StreamError::ExpiredOrInvalidSignature => {
                write!(f, "Invalid or expired download link signature")
            }
            StreamError::NotFound => write!(f, "File not found"),
            StreamError::IsDirectory => write!(f, "Cannot download directory directly"),
            StreamError::MetadataFailed => write!(f, "Failed to read file metadata"),
        }
    }
}

impl std::error::Error for StreamError {}

impl IntoResponse for StreamError {
    fn into_response(self) -> Response {
        let mut resp = match self {
            StreamError::InvalidPath => (StatusCode::BAD_REQUEST, "Invalid path").into_response(),
            StreamError::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
            StreamError::InvalidSignature => {
                (StatusCode::FORBIDDEN, "Invalid download link signature").into_response()
            }
            StreamError::ExpiredOrInvalidSignature => (
                StatusCode::FORBIDDEN,
                "Invalid or expired download link signature",
            )
                .into_response(),
            StreamError::NotFound => (StatusCode::NOT_FOUND, "File not found").into_response(),
            StreamError::IsDirectory => (
                StatusCode::BAD_REQUEST,
                "Cannot download directory directly",
            )
                .into_response(),
            StreamError::MetadataFailed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read file metadata",
            )
                .into_response(),
        };
        let h = resp.headers_mut();
        h.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
        h.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
        resp
    }
}

pub struct FileStream {
    file: tokio::fs::File,
    file_size: u64,
    content_type: &'static str,
    filename: String,
    as_attachment: bool,
}

impl FileStream {
    pub async fn open(fs: &LocalFs, path: &str, as_attachment: bool) -> Result<Self, StreamError> {
        let file = match fs.open(path).await {
            Ok(f) => f,
            Err(_) => return Err(StreamError::NotFound),
        };
        Self::from_file(file, path, as_attachment).await
    }

    pub async fn from_file(
        file: tokio::fs::File,
        path: &str,
        as_attachment: bool,
    ) -> Result<Self, StreamError> {
        let meta = match file.metadata().await {
            Ok(m) => m,
            Err(_) => return Err(StreamError::MetadataFailed),
        };

        if meta.is_dir() {
            return Err(StreamError::IsDirectory);
        }

        let file_size = meta.len();
        let content_type = crate::preview::detector::detect_from_path(path).1;
        let filename = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();

        Ok(Self {
            file,
            file_size,
            content_type,
            filename,
            as_attachment,
        })
    }

    pub async fn into_response(mut self, range_header: Option<&str>) -> Response {
        let disposition = safe_content_disposition(&self.filename, self.as_attachment);

        if let Some(range_val) = range_header {
            if let Some((start, end)) = parse_range(range_val, self.file_size) {
                let part_len = end - start + 1;
                if self.file.seek(SeekFrom::Start(start)).await.is_err() {
                    return range_not_satisfiable(self.file_size);
                }

                let stream = ReaderStream::new(self.file.take(part_len));
                let mut resp =
                    (StatusCode::PARTIAL_CONTENT, Body::from_stream(stream)).into_response();
                let range_str = format!("bytes {start}-{end}/{}", self.file_size);
                apply_stream_headers(
                    &mut resp,
                    self.content_type,
                    part_len,
                    disposition,
                    self.as_attachment,
                    Some(&range_str),
                );
                return resp;
            } else {
                return range_not_satisfiable(self.file_size);
            }
        }

        let stream = ReaderStream::new(self.file.take(self.file_size));
        let mut resp = (StatusCode::OK, Body::from_stream(stream)).into_response();
        apply_stream_headers(
            &mut resp,
            self.content_type,
            self.file_size,
            disposition,
            self.as_attachment,
            None,
        );
        resp
    }
}

async fn resolve_stream_fs(
    state: &SharedState,
    clean_path: &str,
    query: &SignQuery,
    headers: &HeaderMap,
) -> Result<LocalFs, StreamError> {
    if let Some(ref s) = query.sign {
        let Some(uid) = query.uid else {
            return Err(StreamError::InvalidSignature);
        };
        let user = match get_user_by_id(&state.pool, uid).await {
            Ok(Some(user)) if !user.disabled => user,
            _ => return Err(StreamError::InvalidSignature),
        };
        if verify_user_link(&state.config.jwt_secret, &user, clean_path, s).is_err() {
            return Err(StreamError::ExpiredOrInvalidSignature);
        }
        LocalFs::new(&user.local_path, false).map_err(|_| StreamError::NotFound)
    } else {
        let Some(user) = authenticate_user(headers, state).await else {
            return Err(StreamError::Unauthorized);
        };
        LocalFs::new(&user.local_path, false).map_err(|_| StreamError::NotFound)
    }
}

async fn stream_file(
    state: SharedState,
    raw_path: String,
    query: SignQuery,
    headers: HeaderMap,
    as_attachment: bool,
) -> Response {
    let clean_path = format!("/{}", raw_path.trim_start_matches('/'));
    if clean_path.split(['/', '\\']).any(|p| p == "." || p == "..") {
        return StreamError::InvalidPath.into_response();
    }

    let fs = match resolve_stream_fs(&state, &clean_path, &query, &headers).await {
        Ok(fs) => fs,
        Err(err) => return err.into_response(),
    };

    let stream = match FileStream::open(&fs, &clean_path, as_attachment).await {
        Ok(stream) => stream,
        Err(err) => return err.into_response(),
    };

    let range_header = headers.get("range").and_then(|r| r.to_str().ok());
    stream.into_response(range_header).await
}

fn apply_stream_headers(
    resp: &mut Response,
    content_type: &str,
    len: u64,
    disposition: HeaderValue,
    as_attachment: bool,
    content_range: Option<&str>,
) {
    let h = resp.headers_mut();
    h.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert(CONTENT_LENGTH, HeaderValue::from(len));
    h.insert(CONTENT_DISPOSITION, disposition);
    if let Some(range) = content_range {
        if let Ok(val) = HeaderValue::from_str(range) {
            h.insert(CONTENT_RANGE, val);
        }
    }
    if !as_attachment
        && (content_type.starts_with("text/html") || content_type.starts_with("image/svg+xml"))
    {
        h.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    }
    h.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    h.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
}

fn range_not_satisfiable(file_size: u64) -> Response {
    let mut resp = (StatusCode::RANGE_NOT_SATISFIABLE, "Range Not Satisfiable").into_response();
    let h = resp.headers_mut();
    h.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Ok(val) = HeaderValue::from_str(&format!("bytes */{}", file_size)) {
        h.insert(CONTENT_RANGE, val);
    }
    h.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    h.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    resp
}

fn safe_content_disposition(filename: &str, as_attachment: bool) -> HeaderValue {
    let disp_type = if as_attachment {
        "attachment"
    } else {
        "inline"
    };
    let encoded = encode_url_path(filename);
    let ascii_fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let header_str =
        format!("{disp_type}; filename=\"{ascii_fallback}\"; filename*=UTF-8''{encoded}");
    HeaderValue::from_str(&header_str)
        .unwrap_or_else(|_| HeaderValue::from_static("attachment; filename=\"file\""))
}

pub(crate) fn parse_range(range: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let bytes_prefix = "bytes=";
    if !range.starts_with(bytes_prefix) {
        return None;
    }
    let s = &range[bytes_prefix.len()..];
    let mut parts = s.split('-');
    let start_str = parts.next()?.trim();
    let end_str = parts.next()?.trim();
    if parts.next().is_some() {
        return None;
    }

    if start_str.is_empty() {
        // Suffix range: -N means last N bytes
        let len: u64 = end_str.parse().ok()?;
        if len == 0 {
            return None;
        }
        let start = total.saturating_sub(len);
        Some((start, total - 1))
    } else {
        let start: u64 = start_str.parse().ok()?;
        if start >= total {
            return None;
        }
        let end = if end_str.is_empty() {
            total - 1
        } else {
            let parsed_end: u64 = end_str.parse().ok()?;
            if parsed_end < start {
                return None;
            }
            parsed_end.min(total - 1)
        };
        Some((start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range_empty_file() {
        assert_eq!(parse_range("bytes=0-0", 0), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
        assert_eq!(parse_range("bytes=-0", 0), None);
        assert_eq!(parse_range("bytes=-5", 0), None);
    }

    #[test]
    fn test_parse_range_bytes_neg_zero() {
        assert_eq!(parse_range("bytes=-0", 100), None);
        assert_eq!(parse_range("bytes=-0", 1), None);
    }

    #[test]
    fn test_parse_range_suffix() {
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-1", 100), Some((99, 99)));
        assert_eq!(parse_range("bytes=-100", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=-200", 100), Some((0, 99)));
    }

    #[test]
    fn test_parse_range_start_out_of_bounds() {
        assert_eq!(parse_range("bytes=100-150", 100), None);
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=200-", 100), None);
    }

    #[test]
    fn test_parse_range_standard() {
        assert_eq!(parse_range("bytes=0-49", 100), Some((0, 49)));
        assert_eq!(parse_range("bytes=50-", 100), Some((50, 99)));
        assert_eq!(parse_range("bytes=50-200", 100), Some((50, 99)));
        assert_eq!(parse_range("bytes=50-40", 100), None);
        assert_eq!(parse_range("invalid", 100), None);
        assert_eq!(parse_range("bytes=--", 100), None);
        assert_eq!(parse_range("bytes=0-1-2", 100), None);
    }

    #[tokio::test]
    async fn test_file_stream_full_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("full.txt");
        tokio::fs::write(&path, b"hello streaming world")
            .await
            .unwrap();

        let file = tokio::fs::File::open(&path).await.unwrap();
        let stream = FileStream::from_file(file, "full.txt", false)
            .await
            .unwrap();
        let resp = stream.into_response(None).await;

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get(ACCEPT_RANGES).unwrap(), "bytes");
        assert_eq!(resp.headers().get(CONTENT_LENGTH).unwrap(), "21");
        assert_eq!(
            resp.headers().get(CACHE_CONTROL).unwrap(),
            "private, no-store"
        );
        assert_eq!(resp.headers().get(REFERRER_POLICY).unwrap(), "no-referrer");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"hello streaming world");
    }

    #[tokio::test]
    async fn test_file_stream_range_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("range.txt");
        tokio::fs::write(&path, b"0123456789").await.unwrap();

        let file = tokio::fs::File::open(&path).await.unwrap();
        let stream = FileStream::from_file(file, "range.txt", true)
            .await
            .unwrap();
        let resp = stream.into_response(Some("bytes=3-7")).await;

        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resp.headers().get(CONTENT_RANGE).unwrap(), "bytes 3-7/10");
        assert_eq!(resp.headers().get(CONTENT_LENGTH).unwrap(), "5");
        assert_eq!(
            resp.headers().get(CACHE_CONTROL).unwrap(),
            "private, no-store"
        );
        assert_eq!(resp.headers().get(REFERRER_POLICY).unwrap(), "no-referrer");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"34567");
    }

    #[tokio::test]
    async fn test_file_stream_unsatisfiable_range() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("invalid.txt");
        tokio::fs::write(&path, b"0123456789").await.unwrap();

        let file = tokio::fs::File::open(&path).await.unwrap();
        let stream = FileStream::from_file(file, "invalid.txt", false)
            .await
            .unwrap();
        let resp = stream.into_response(Some("bytes=50-100")).await;

        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(resp.headers().get(CONTENT_RANGE).unwrap(), "bytes */10");
        assert_eq!(
            resp.headers().get(CACHE_CONTROL).unwrap(),
            "private, no-store"
        );
        assert_eq!(resp.headers().get(REFERRER_POLICY).unwrap(), "no-referrer");
    }
}
