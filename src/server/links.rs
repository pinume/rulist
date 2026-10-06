use super::{SharedState, encode_url_path};
use crate::db::User;
use crate::filesystem::FileEntry;
use crate::sign::{sign_path, verify_sign};

#[derive(serde::Serialize)]
pub(crate) struct SignedFileEntry {
    #[serde(flatten)]
    entry: FileEntry,
    sign: String,
    raw_url: String,
}

pub(crate) fn signed_file_entry(
    state: &SharedState,
    user: &User,
    path: &str,
    entry: FileEntry,
) -> anyhow::Result<SignedFileEntry> {
    let (sign, raw_url) = if entry.is_dir {
        (String::new(), String::new())
    } else {
        signed_preview_url(state, user, path)?
    };
    Ok(SignedFileEntry {
        entry,
        sign,
        raw_url,
    })
}

fn user_context(user: &User) -> String {
    format!(
        "uid={}:pwd_ts={}:root={}",
        user.id, user.pwd_ts, user.local_path
    )
}

pub(crate) fn signed_preview_url(
    state: &SharedState,
    user: &User,
    path: &str,
) -> anyhow::Result<(String, String)> {
    signed_url(state, user, path, "/p")
}

pub(crate) fn signed_download_url(
    state: &SharedState,
    user: &User,
    path: &str,
) -> anyhow::Result<String> {
    signed_url(state, user, path, "/d").map(|(_, url)| url)
}

fn signed_url(
    state: &SharedState,
    user: &User,
    path: &str,
    prefix: &str,
) -> anyhow::Result<(String, String)> {
    let sign = sign_path(&state.config.jwt_secret, path, &user_context(user))?;
    let url = format!(
        "{prefix}{}?sign={sign}&uid={}",
        encode_url_path(path),
        user.id
    );
    Ok((sign, url))
}

pub(crate) fn verify_user_link(
    secret: &str,
    user: &User,
    path: &str,
    sign: &str,
) -> anyhow::Result<()> {
    verify_sign(secret, path, &user_context(user), sign)
}
