use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;

use crate::auth::parse_jwt;
use crate::db::User;
use crate::db::get_user_by_id;
use crate::filesystem::FsError;

use super::AppState;

pub(crate) async fn authenticate_user(headers: &HeaderMap, state: &AppState) -> Option<User> {
    let auth_header = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);

    let claims = parse_jwt(token, &state.config.jwt_secret).ok()?;
    if crate::db::is_token_revoked(&state.pool, &claims.jti)
        .await
        .ok()?
    {
        return None;
    }

    let user = get_user_by_id(&state.pool, claims.user_id)
        .await
        .ok()
        .flatten()?;
    if user.username != claims.username || user.disabled || user.pwd_ts != claims.pwd_ts {
        return None;
    }

    Some(user)
}

pub(crate) fn normalize_request_path(requested: &str) -> anyhow::Result<String> {
    if requested
        .split(['/', '\\'])
        .any(|part| part == "." || part == "..")
    {
        return Err(FsError::InvalidPath.into());
    }

    let relative = requested.trim_matches('/');
    Ok(if relative.is_empty() {
        "/".to_string()
    } else {
        format!("/{relative}")
    })
}

pub(crate) fn permitted(user: &User, bit: i32) -> bool {
    user.is_admin() || user.permission & (1 << bit) != 0
}

pub(crate) fn encode_url_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub(crate) fn percent_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let input = s.as_bytes();
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            if let Ok(hex) = std::str::from_utf8(&input[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    bytes.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        bytes.push(input[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
