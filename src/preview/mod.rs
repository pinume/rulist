pub mod detector;
pub mod processor;
pub mod types;

pub use detector::detect_from_path;
pub use types::*;

use crate::filesystem::local::LocalFs;

#[derive(Debug)]
pub enum PreviewError {
    IsDirectory,
    Fs(anyhow::Error),
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IsDirectory => write!(f, "Directory cannot be previewed"),
            Self::Fs(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for PreviewError {}

pub async fn inspect_file(
    fs: &LocalFs,
    path: &str,
    raw_url_provider: impl FnOnce() -> Result<String, anyhow::Error>,
) -> Result<PreviewResponse, PreviewError> {
    let file = fs.get(path).await.map_err(PreviewError::Fs)?;

    if file.is_dir {
        return Err(PreviewError::IsDirectory);
    }

    let raw_url = raw_url_provider().map_err(PreviewError::Fs)?;

    let (preview_type, mime_type) = detector::detect_from_path(&file.name);
    let strategy = preview_type.strategy();

    let meta = PreviewMeta {
        name: file.name,
        path: path.to_string(),
        size: file.size.max(0) as u64,
        modified: file.modified,
        mime_type: mime_type.to_string(),
        preview_type,
        strategy,
        raw_url,
        permissions: file.permissions,
    };

    let (content, error) = if strategy == PreviewStrategy::Processed {
        match processor::process_file(fs, path, preview_type, file.size).await {
            Ok(content) => (Some(content), None),
            Err(err_code) => (None, Some(err_code.to_string())),
        }
    } else {
        (None, None)
    };

    Ok(PreviewResponse {
        meta,
        content,
        error,
    })
}
