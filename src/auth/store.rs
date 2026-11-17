//! 사용자와 세션을 DB에 쓰고 읽는다. 비밀번호와 세션 값은 원문을 저장하지 않는다(ADR 0006).

use std::sync::OnceLock;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, TimeDelta, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use super::User;

/// 로그인 유지 기간: 7일.
pub const SESSION_DAYS: i64 = 7;

/// 세션 값의 무작위 바이트 수. 256비트라 짐작으로 맞힐 수 없다.
const SESSION_BYTES: usize = 32;

/// 비밀번호의 Argon2id 해시(PHC 문자열)를 만든다. CPU를 쓰므로 블로킹 작업으로 돌린다.
pub async fn hash_password(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|e| anyhow::anyhow!("비밀번호 해시 실패: {e}"))
    })
    .await?
}

/// 비밀번호가 저장된 해시와 맞는지 본다. 해시가 없으면(없는 아이디) 가짜 해시와 한 번 비교하고 `false`다.
/// 없는 아이디와 틀린 비밀번호의 응답 시간 차이를 줄이기 위해서다.
async fn verify_password(password: String, stored: Option<String>) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let exists = stored.is_some();
        let stored = stored.unwrap_or_else(|| dummy_hash().to_string());
        let parsed = PasswordHash::new(&stored)
            .map_err(|e| anyhow::anyhow!("저장된 비밀번호 해시를 읽지 못했습니다: {e}"))?;
        let matches = Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok();
        Ok(exists && matches)
    })
    .await?
}

/// 없는 아이디로 로그인할 때 비교할 해시. 처음 한 번만 만든다.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(b"not-a-real-password", &salt)
            .expect("고정 입력의 해시는 실패하지 않는다")
            .to_string()
    })
}

/// 사용자를 만든다. 아이디가 이미 있으면(대소문자 무시) `None`.
pub async fn create_user(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
) -> sqlx::Result<Option<User>> {
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, $2)
         ON CONFLICT ((lower(username))) DO NOTHING
         RETURNING id",
    )
    .bind(username)
    .bind(password_hash)
    .fetch_optional(pool)
    .await?;
    Ok(id.map(|id| User {
        id,
        username: username.to_string(),
    }))
}

/// 아이디(대소문자 무시)와 비밀번호가 맞으면 그 사용자를 돌려준다. 이유는 구분하지 않는다.
pub async fn check_credentials(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> anyhow::Result<Option<User>> {
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT id, username, password_hash FROM users WHERE lower(username) = lower($1)",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    let (user, stored) = match row {
        Some((id, username, hash)) => (Some(User { id, username }), Some(hash)),
        None => (None, None),
    };
    if verify_password(password.to_string(), stored).await? {
        Ok(user)
    } else {
        Ok(None)
    }
}

/// 사용자의 새 세션을 만들고 세션 값 원문을 돌려준다. 원문은 쿠키에만 담긴다.
///
/// 만들기 전에 만료된 세션(누구의 것이든)을 지운다. 만료된 세션은 다시 쓰이지 않으므로 쌓이지 않게 한다.
pub async fn create_session(
    pool: &PgPool,
    user_id: i64,
    now: DateTime<Utc>,
) -> sqlx::Result<String> {
    sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
        .bind(now)
        .execute(pool)
        .await?;
    let mut bytes = [0u8; SESSION_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(hash_token(&token))
    .bind(user_id)
    .bind(now)
    .bind(now + TimeDelta::days(SESSION_DAYS))
    .execute(pool)
    .await?;
    Ok(token)
}

/// 세션 값으로 사용자를 찾는다. 없거나 만료된 세션이면 `None`.
pub async fn find_session_user(
    pool: &PgPool,
    token: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<User>> {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT users.id, users.username
         FROM sessions JOIN users ON users.id = sessions.user_id
         WHERE sessions.token_hash = $1 AND sessions.expires_at > $2",
    )
    .bind(hash_token(token))
    .bind(now)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, username)| User { id, username }))
}

/// 세션을 지운다. 그 세션 값은 그 즉시 무효가 된다.
pub async fn delete_session(pool: &PgPool, token: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(hash_token(token))
        .execute(pool)
        .await?;
    Ok(())
}

/// 세션 값 원문의 SHA-256 해시. DB에는 이것만 저장한다.
fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
