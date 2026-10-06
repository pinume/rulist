use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use std::future::Future;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::fs;

use crate::filesystem::{BatchError, ConflictPolicy, FileEntry, FsError, valid_name};

use super::ops::{
    copy_path_safe, entry_exists, move_path_safe, remove_path_recursive, rename_no_replace,
};

const LISTING_CACHE_TTL: Duration = Duration::from_secs(2);
const LISTING_CACHE_MAX_ENTRIES: usize = 100_000;

struct CachedListing {
    path: PathBuf,
    show_hidden: bool,
    stamp: (u64, u64, i64, i64, i64, i64),
    created: Instant,
    files: Arc<[FileEntry]>,
}

struct ListingCache {
    generation: u64,
    entry: Option<CachedListing>,
}

// ponytail: cache one directory; use a bounded per-directory cache if concurrent browsing needs it.
static LISTING_CACHE: Mutex<ListingCache> = Mutex::new(ListingCache {
    generation: 0,
    entry: None,
});

async fn run_mutation_to_completion<F, T>(operation: F) -> Result<T>
where
    F: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    // Keep multi-step mutations alive if the caller drops its request future.
    tokio::spawn(operation)
        .await
        .context("filesystem mutation task failed")?
}

struct ListingMutation;

impl ListingMutation {
    fn new() -> Self {
        invalidate_listing_cache();
        Self
    }
}

impl Drop for ListingMutation {
    fn drop(&mut self) {
        invalidate_listing_cache();
    }
}

fn invalidate_listing_cache() {
    let mut cache = LISTING_CACHE.lock().unwrap();
    cache.generation = cache.generation.wrapping_add(1);
    cache.entry = None;
}

#[derive(Debug, Clone)]
pub struct LocalFs {
    root_path: PathBuf,
    show_hidden: bool,
}

impl LocalFs {
    pub fn new(root_path: impl AsRef<Path>, show_hidden: bool) -> Result<Self> {
        let root_path = root_path.as_ref();
        if root_path.as_os_str().is_empty() {
            return Err(anyhow!("local_path cannot be empty"));
        }
        if !root_path.is_absolute() {
            return Err(anyhow!("local_path must be an absolute path"));
        }

        Ok(Self {
            root_path: root_path.to_path_buf(),
            show_hidden,
        })
    }

    fn hidden_name_denied(&self, name: &str) -> bool {
        !self.show_hidden && name.starts_with('.')
    }

    fn safe_resolve(&self, subpath: &str) -> Result<PathBuf> {
        let clean = subpath.trim_matches('/');
        let mut target = self.root_path.clone();

        for component in Path::new(clean).components() {
            match component {
                Component::Normal(value) => {
                    if self.hidden_name_denied(&value.to_string_lossy()) {
                        return Err(FsError::Forbidden.into());
                    }
                    target.push(value);
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(FsError::InvalidPath.into());
                }
                _ => {
                    return Err(FsError::InvalidPath.into());
                }
            }

            match std::fs::symlink_metadata(&target) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(FsError::Forbidden.into());
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }

        Ok(target)
    }

    pub async fn entry_exists(&self, subpath: &str) -> Result<bool> {
        let clean = subpath.trim_matches('/');
        if clean.is_empty() {
            return entry_exists(&self.root_path).await;
        }

        let relative = Path::new(clean);
        let name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| valid_name(name))
            .ok_or(FsError::InvalidPath)?;
        if self.hidden_name_denied(name) {
            return Err(FsError::Forbidden.into());
        }
        let parent = relative.parent().and_then(Path::to_str).unwrap_or_default();
        let parent = self.safe_resolve(parent)?;
        entry_exists(&parent.join(name)).await
    }

    pub async fn list(&self, subpath: &str) -> Result<Vec<FileEntry>> {
        Ok(self.list_snapshot(subpath).await?.to_vec())
    }

    pub(crate) async fn list_snapshot(&self, subpath: &str) -> Result<Arc<[FileEntry]>> {
        let full_path = self.safe_resolve(subpath)?;
        let show_hidden = self.show_hidden;
        let meta = fs::metadata(&full_path).await?;
        let stamp = (
            meta.dev(),
            meta.ino(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec(),
        );
        let generation = {
            let cache = LISTING_CACHE.lock().unwrap();
            if let Some(entry) = &cache.entry {
                if entry.path == full_path
                    && entry.show_hidden == show_hidden
                    && entry.stamp == stamp
                    && entry.created.elapsed() < LISTING_CACHE_TTL
                {
                    return Ok(Arc::clone(&entry.files));
                }
            }
            cache.generation
        };
        let scan_path = full_path.clone();

        let items = tokio::task::spawn_blocking(move || -> Result<Vec<FileEntry>> {
            let read_dir = std::fs::read_dir(&scan_path)
                .with_context(|| format!("failed to read directory: {:?}", scan_path))?;
            let mut items = Vec::new();

            for entry in read_dir.flatten() {
                let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
                    tracing::warn!(
                        path = ?entry.path(),
                        "skipping non-UTF-8 filename"
                    );
                    continue;
                };
                if !show_hidden && file_name.starts_with('.') {
                    continue;
                }

                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if file_type.is_symlink() {
                    continue;
                }
                if !file_type.is_file() && !file_type.is_dir() {
                    tracing::debug!(
                        path = ?entry.path(),
                        "skipping unsupported filesystem entry"
                    );
                    continue;
                }
                let Ok(meta) = std::fs::metadata(entry.path()) else {
                    continue;
                };

                let is_dir = meta.is_dir();
                let size = if is_dir { 0 } else { meta.len() as i64 };
                let modified = meta
                    .modified()
                    .ok()
                    .map(|time| DateTime::<Utc>::from(time).to_rfc3339())
                    .unwrap_or_default();

                let permissions = crate::filesystem::format_mode(meta.permissions().mode(), is_dir);

                let mut item = FileEntry::new(file_name, size, is_dir, modified);
                item.permissions = Some(permissions);
                items.push(item);
            }

            Ok(items)
        })
        .await
        .context("directory scan task panicked or failed")??;
        let items: Arc<[FileEntry]> = items.into();
        if items.len() <= LISTING_CACHE_MAX_ENTRIES {
            let mut cache = LISTING_CACHE.lock().unwrap();
            if cache.generation == generation {
                cache.entry = Some(CachedListing {
                    path: full_path,
                    show_hidden,
                    stamp,
                    created: Instant::now(),
                    files: Arc::clone(&items),
                });
            }
        }
        Ok(items)
    }

    pub async fn get(&self, subpath: &str) -> Result<FileEntry> {
        let full_path = self.safe_resolve(subpath)?;
        let meta = fs::symlink_metadata(&full_path)
            .await
            .with_context(|| format!("file not found: {:?}", full_path))?;
        let file_type = meta.file_type();
        if !file_type.is_file() && !file_type.is_dir() {
            return Err(FsError::InvalidPath.into());
        }
        let file_name = match full_path.file_name() {
            Some(name) => name
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("filename is not valid UTF-8"))?,
            None => "/".to_string(),
        };
        let is_dir = meta.is_dir();
        let size = if is_dir { 0 } else { meta.len() as i64 };
        let modified = meta
            .modified()
            .ok()
            .map(|time| DateTime::<Utc>::from(time).to_rfc3339())
            .unwrap_or_default();

        let permissions = crate::filesystem::format_mode(meta.permissions().mode(), is_dir);

        let mut item = FileEntry::new(file_name, size, is_dir, modified);
        item.permissions = Some(permissions);
        Ok(item)
    }

    pub async fn open(&self, subpath: &str) -> Result<fs::File> {
        let full_path = self.safe_resolve(subpath)?;
        let meta = fs::symlink_metadata(&full_path)
            .await
            .with_context(|| format!("failed to inspect file: {:?}", full_path))?;
        if !meta.file_type().is_file() {
            return Err(FsError::InvalidPath.into());
        }
        fs::File::open(&full_path)
            .await
            .with_context(|| format!("failed to open file: {:?}", full_path))
    }

    /// Write a complete stream through a hidden staging file. Cancellation or errors
    /// remove the stage; the destination is only changed after the stream finishes.
    pub async fn write(
        &self,
        subpath: &str,
        input: impl tokio::io::AsyncRead + Unpin,
        overwrite: bool,
    ) -> Result<()> {
        self.write_limited(subpath, input, overwrite, 100 * 1024 * 1024 * 1024)
            .await
    }

    async fn write_limited(
        &self,
        subpath: &str,
        input: impl tokio::io::AsyncRead + Unpin,
        overwrite: bool,
        limit: u64,
    ) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _mutation = ListingMutation::new();
        if subpath.trim_matches('/').is_empty() {
            return Err(FsError::InvalidPath.into());
        }
        let target = self.safe_resolve(subpath)?;
        if !overwrite && self.entry_exists(subpath).await? {
            return Err(FsError::Conflict.into());
        }
        let parent = target.parent().ok_or(FsError::InvalidPath)?;
        fs::create_dir_all(parent).await?;
        let temp =
            target.with_file_name(format!(".rulist-upload-{}", crate::auth::rand_string(24)));
        let stage_path = temp.clone();
        let (mut file, _stage) = tokio::task::spawn_blocking(move || -> Result<_> {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&stage_path)?;
            Ok((fs::File::from_std(file), UploadStage(stage_path)))
        })
        .await??;
        let mut input = input.take(limit + 1);
        if tokio::io::copy(&mut input, &mut file).await? > limit {
            return Err(FsError::TooLarge.into());
        }
        file.flush().await?;
        drop(file);
        // Recheck the parent before publishing; all physical paths stay internal.
        self.safe_resolve(subpath)?;
        if overwrite {
            fs::rename(&temp, &target).await?;
        } else {
            fs::hard_link(&temp, &target).await?;
        }
        Ok(())
    }

    pub async fn move_many(
        &self,
        src_dir: &str,
        dst_dir: &str,
        names: &[String],
        policy: ConflictPolicy,
    ) -> std::result::Result<usize, BatchError> {
        self.transfer_many(src_dir, dst_dir, names, policy, true)
            .await
    }

    pub async fn copy_many(
        &self,
        src_dir: &str,
        dst_dir: &str,
        names: &[String],
        policy: ConflictPolicy,
    ) -> std::result::Result<usize, BatchError> {
        self.transfer_many(src_dir, dst_dir, names, policy, false)
            .await
    }

    async fn transfer_many(
        &self,
        src_dir: &str,
        dst_dir: &str,
        names: &[String],
        policy: ConflictPolicy,
        moving: bool,
    ) -> std::result::Result<usize, BatchError> {
        let mut unique = std::collections::HashSet::new();
        let mut transfers = Vec::new();
        for name in names {
            if !valid_name(name) || !unique.insert(name) {
                return Err(anyhow::Error::from(FsError::InvalidPath).into());
            }
            let src = format!("{}/{}", src_dir.trim_end_matches('/'), name);
            let dst = format!("{}/{}", dst_dir.trim_end_matches('/'), name);
            if src == dst || dst.starts_with(&format!("{src}/")) {
                return Err(anyhow::Error::from(FsError::InvalidPath).into());
            }
            if self.entry_exists(&dst).await? {
                match policy {
                    ConflictPolicy::Cancel => {
                        return Err(anyhow::Error::from(FsError::Conflict).into());
                    }
                    ConflictPolicy::Skip => continue,
                    ConflictPolicy::Overwrite => {}
                }
            }
            transfers.push((src, dst));
        }
        let total = transfers.len();
        for (completed, (src, dst)) in transfers.into_iter().enumerate() {
            let result = if moving {
                self.move_to_safe(&src, &dst, policy == ConflictPolicy::Overwrite)
                    .await
            } else {
                self.copy_to_safe(&src, &dst, policy == ConflictPolicy::Overwrite)
                    .await
            };
            if let Err(cause) = result {
                return Err(BatchError {
                    completed,
                    path: Some(src),
                    cause,
                });
            }
        }
        Ok(total)
    }

    pub async fn remove_many(
        &self,
        dir: &str,
        names: &[String],
    ) -> std::result::Result<usize, BatchError> {
        if names.iter().any(|name| !valid_name(name)) {
            return Err(anyhow::Error::from(FsError::InvalidPath).into());
        }
        for (completed, name) in names.iter().enumerate() {
            let path = format!("{}/{}", dir.trim_end_matches('/'), name);
            if let Err(cause) = self.remove(&path).await {
                return Err(BatchError {
                    completed,
                    path: Some(path),
                    cause,
                });
            }
        }
        Ok(names.len())
    }

    pub async fn mkdir(&self, subpath: &str) -> Result<()> {
        let _mutation = ListingMutation::new();
        let full_path = self.safe_resolve(subpath)?;
        fs::create_dir_all(&full_path)
            .await
            .with_context(|| format!("failed to create directory: {:?}", full_path))?;
        Ok(())
    }

    pub async fn remove(&self, subpath: &str) -> Result<()> {
        let _mutation = ListingMutation::new();
        if subpath.trim_matches('/').is_empty() {
            return Err(FsError::InvalidPath.into());
        }
        let full_path = self.safe_resolve(subpath)?;
        remove_path_recursive(&full_path).await
    }

    pub async fn rename_safe(&self, subpath: &str, new_name: &str, overwrite: bool) -> Result<()> {
        let fs = self.clone();
        let subpath = subpath.to_owned();
        let new_name = new_name.to_owned();
        run_mutation_to_completion(async move {
            fs.rename_safe_inner(&subpath, &new_name, overwrite).await
        })
        .await
    }

    async fn rename_safe_inner(
        &self,
        subpath: &str,
        new_name: &str,
        overwrite: bool,
    ) -> Result<()> {
        let _mutation = ListingMutation::new();
        if subpath.trim_matches('/').is_empty() {
            return Err(FsError::InvalidPath.into());
        }
        if !valid_name(new_name) {
            return Err(FsError::InvalidPath.into());
        }
        if self.hidden_name_denied(new_name) {
            return Err(FsError::Forbidden.into());
        }

        let src_path = self.safe_resolve(subpath)?;
        let parent = src_path.parent().ok_or(FsError::InvalidPath)?;
        let dst_path = parent.join(new_name);
        if src_path == dst_path {
            return Ok(());
        }
        fs::symlink_metadata(&src_path).await?;

        let dst_exists = entry_exists(&dst_path).await?;
        if !dst_exists {
            if overwrite {
                fs::rename(&src_path, &dst_path).await?;
            } else {
                rename_no_replace(&src_path, &dst_path).await?;
            }
            return Ok(());
        }

        let src_canon = fs::canonicalize(&src_path).await.ok();
        let dst_canon = fs::canonicalize(&dst_path).await.ok();
        if src_canon.is_some() && src_canon == dst_canon {
            let temp = parent.join(format!(
                ".rulist-rename-case-{}",
                crate::auth::rand_string(24)
            ));
            rename_no_replace(&src_path, &temp).await?;
            let result = if overwrite {
                fs::rename(&temp, &dst_path).await
            } else {
                rename_no_replace(&temp, &dst_path).await
            };
            if let Err(error) = result {
                if let Err(restore_error) = rename_no_replace(&temp, &src_path).await {
                    tracing::error!(
                        error = %restore_error,
                        "CRITICAL: failed to restore source after case rename failure"
                    );
                }
                return Err(error.into());
            }
            return Ok(());
        }

        if !overwrite {
            return Err(FsError::Conflict.into());
        }

        let backup = parent.join(format!(
            ".rulist-rename-backup-{}",
            crate::auth::rand_string(24)
        ));
        rename_no_replace(&dst_path, &backup).await?;

        match fs::rename(&src_path, &dst_path).await {
            Ok(()) => {
                if let Err(error) = remove_path_recursive(&backup).await {
                    tracing::warn!(
                        error = %error,
                        path = ?backup,
                        "failed to remove backup after successful rename"
                    );
                }
                Ok(())
            }
            Err(error) => {
                if let Err(restore_error) = rename_no_replace(&backup, &dst_path).await {
                    tracing::error!(
                        error = %restore_error,
                        "CRITICAL: failed to restore rename backup"
                    );
                }
                Err(error.into())
            }
        }
    }

    pub async fn batch_rename(
        &self,
        src_dir_subpath: &str,
        pairs: &[(String, String)],
    ) -> Result<()> {
        let fs = self.clone();
        let src_dir_subpath = src_dir_subpath.to_owned();
        let pairs = pairs.to_vec();
        run_mutation_to_completion(
            async move { fs.batch_rename_inner(&src_dir_subpath, &pairs).await },
        )
        .await
    }

    async fn batch_rename_inner(
        &self,
        src_dir_subpath: &str,
        pairs: &[(String, String)],
    ) -> Result<()> {
        let _mutation = ListingMutation::new();
        let dir_path = self.safe_resolve(src_dir_subpath)?;
        if pairs.is_empty() {
            return Ok(());
        }

        for (src_name, new_name) in pairs {
            if !valid_name(src_name) || !valid_name(new_name) {
                return Err(FsError::InvalidPath.into());
            }
            if self.hidden_name_denied(src_name) || self.hidden_name_denied(new_name) {
                return Err(FsError::Forbidden.into());
            }
        }

        let mut src_set = std::collections::HashSet::new();
        for (src, _) in pairs {
            if !src_set.insert(src.as_str()) {
                return Err(FsError::Conflict.into());
            }
        }
        let mut dst_set = std::collections::HashSet::new();
        for (_, dst) in pairs {
            if !dst_set.insert(dst.as_str()) {
                return Err(FsError::Conflict.into());
            }
        }

        for (src, _) in pairs {
            self.safe_resolve(&format!(
                "{}/{}",
                src_dir_subpath.trim_end_matches('/'),
                src
            ))?;
            let src_path = dir_path.join(src);
            fs::symlink_metadata(src_path).await?;
        }
        for (_, dst) in pairs {
            if entry_exists(&dir_path.join(dst)).await? && !src_set.contains(dst.as_str()) {
                return Err(FsError::Conflict.into());
            }
        }

        let active_pairs: Vec<_> = pairs.iter().filter(|(src, dst)| src != dst).collect();
        if active_pairs.is_empty() {
            return Ok(());
        }

        let mut staged: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new();
        for (index, (src, dst)) in active_pairs.iter().enumerate() {
            let src_path = dir_path.join(src);
            let temp_path = dir_path.join(format!(
                ".rulist-rename-stage-{index}-{}",
                crate::auth::rand_string(16)
            ));
            let final_path = dir_path.join(dst);

            if let Err(error) = rename_no_replace(&src_path, &temp_path).await {
                for (original, staged_temp, _) in staged.iter().rev() {
                    if let Err(rollback_error) = rename_no_replace(staged_temp, original).await {
                        tracing::error!(
                            error = %rollback_error,
                            "CRITICAL: batch rename rollback failed during staging"
                        );
                    }
                }
                return Err(error.into());
            }
            staged.push((src_path, temp_path, final_path));
        }

        let mut finalized: Vec<(PathBuf, PathBuf)> = Vec::new();
        for (_, temp_path, final_path) in &staged {
            if let Err(error) = rename_no_replace(temp_path, final_path).await {
                for (temp, final_path) in finalized.iter().rev() {
                    let _ = rename_no_replace(final_path, temp).await;
                }
                for (original, temp, _) in staged.iter().rev() {
                    if let Err(rollback_error) = rename_no_replace(temp, original).await {
                        tracing::error!(
                            error = %rollback_error,
                            "CRITICAL: batch rename rollback failed restoring original file"
                        );
                    }
                }
                return Err(error.into());
            }
            finalized.push((temp_path.clone(), final_path.clone()));
        }

        Ok(())
    }

    pub async fn move_to(&self, src_subpath: &str, dst_subpath: &str) -> Result<()> {
        self.move_to_safe(src_subpath, dst_subpath, true).await
    }

    pub async fn move_to_safe(
        &self,
        src_subpath: &str,
        dst_subpath: &str,
        overwrite: bool,
    ) -> Result<()> {
        let fs = self.clone();
        let src_subpath = src_subpath.to_owned();
        let dst_subpath = dst_subpath.to_owned();
        run_mutation_to_completion(async move {
            fs.move_to_safe_inner(&src_subpath, &dst_subpath, overwrite)
                .await
        })
        .await
    }

    async fn move_to_safe_inner(
        &self,
        src_subpath: &str,
        dst_subpath: &str,
        overwrite: bool,
    ) -> Result<()> {
        let _mutation = ListingMutation::new();
        if src_subpath.trim_matches('/').is_empty() || dst_subpath.trim_matches('/').is_empty() {
            return Err(FsError::InvalidPath.into());
        }
        let src_path = self.safe_resolve(src_subpath)?;
        let dst_path = self.safe_resolve(dst_subpath)?;
        move_path_safe(&src_path, &dst_path, overwrite).await
    }

    pub async fn copy_to(&self, src_subpath: &str, dst_subpath: &str) -> Result<()> {
        self.copy_to_safe(src_subpath, dst_subpath, true).await
    }

    pub async fn copy_to_safe(
        &self,
        src_subpath: &str,
        dst_subpath: &str,
        overwrite: bool,
    ) -> Result<()> {
        let fs = self.clone();
        let src_subpath = src_subpath.to_owned();
        let dst_subpath = dst_subpath.to_owned();
        run_mutation_to_completion(async move {
            fs.copy_to_safe_inner(&src_subpath, &dst_subpath, overwrite)
                .await
        })
        .await
    }

    async fn copy_to_safe_inner(
        &self,
        src_subpath: &str,
        dst_subpath: &str,
        overwrite: bool,
    ) -> Result<()> {
        let _mutation = ListingMutation::new();
        if src_subpath.trim_matches('/').is_empty() || dst_subpath.trim_matches('/').is_empty() {
            return Err(FsError::InvalidPath.into());
        }
        let src_path = self.safe_resolve(src_subpath)?;
        let dst_path = self.safe_resolve(dst_subpath)?;
        copy_path_safe(&src_path, &dst_path, overwrite).await
    }
}

// Unix unlink also cleans a stage when an async write is cancelled.
struct UploadStage(PathBuf);

impl Drop for UploadStage {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%error, path = ?self.0, "failed to remove upload stage");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn interrupted_and_oversized_writes_clean_stages_without_changing_destination() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("file.txt"), b"original").unwrap();
        let fs = LocalFs::new(temp.path(), false).unwrap();
        let error = fs
            .write_limited("file.txt", &b"too large"[..], true, 3)
            .await
            .unwrap_err();
        assert_eq!(error.downcast_ref::<FsError>(), Some(&FsError::TooLarge));
        assert_eq!(
            std::fs::read(temp.path().join("file.txt")).unwrap(),
            b"original"
        );
        let (input, _writer) = tokio::io::duplex(8);
        let write = tokio::spawn(async move { fs.write("file.txt", input, true).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if std::fs::read_dir(temp.path()).unwrap().any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".rulist-upload-")
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        write.abort();
        assert!(write.await.unwrap_err().is_cancelled());
        assert_eq!(
            std::fs::read(temp.path().join("file.txt")).unwrap(),
            b"original"
        );
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn listing_cache_reuses_scans_and_refreshes_after_changes() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("visible.txt"), b"old").unwrap();
        std::fs::write(temp.path().join(".hidden.txt"), b"hidden").unwrap();
        let fs = LocalFs::new(temp.path(), false).unwrap();

        let snapshot = fs.list_snapshot("/").await.unwrap();
        assert!(Arc::ptr_eq(
            &snapshot,
            &fs.list_snapshot("/").await.unwrap()
        ));
        let response = fs.list("/").await.unwrap();
        assert_eq!(snapshot[0].name, "visible.txt");
        assert_eq!(response[0].name, "visible.txt");
        assert_eq!(fs.list("/").await.unwrap().len(), 1);
        let created = LISTING_CACHE
            .lock()
            .unwrap()
            .entry
            .as_ref()
            .unwrap()
            .created;
        assert_eq!(fs.list("/").await.unwrap()[0].size, 3);
        assert_eq!(
            LISTING_CACHE
                .lock()
                .unwrap()
                .entry
                .as_ref()
                .unwrap()
                .created,
            created
        );

        // Updating an existing file does not change its parent's timestamp.
        std::fs::write(temp.path().join("visible.txt"), b"updated contents").unwrap();
        assert_eq!(fs.list("/").await.unwrap()[0].size, 3);
        tokio::time::sleep(LISTING_CACHE_TTL + Duration::from_millis(10)).await;
        assert_eq!(fs.list("/").await.unwrap()[0].size, 16);

        let hidden = LocalFs::new(temp.path(), true).unwrap();
        assert_eq!(hidden.list("/").await.unwrap().len(), 2);
        assert_eq!(fs.list("/").await.unwrap().len(), 1);
        std::fs::write(temp.path().join("external.txt"), b"external").unwrap();
        assert_eq!(fs.list("/").await.unwrap().len(), 2);

        fs.mkdir("nested").await.unwrap();
        assert!(LISTING_CACHE.lock().unwrap().entry.is_none());
        assert_eq!(fs.list("/").await.unwrap().len(), 3);
        fs.rename_safe("external.txt", "renamed.txt", false)
            .await
            .unwrap();
        let entries = fs.list("/").await.unwrap();
        assert!(entries.iter().any(|entry| entry.name == "renamed.txt"));
        assert!(!entries.iter().any(|entry| entry.name == "external.txt"));
    }
}
