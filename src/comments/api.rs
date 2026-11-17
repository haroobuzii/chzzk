//! 댓글 HTTP 라우트. 읽기는 누구나, 쓰기는 로그인한 사용자, 지우기는 작성자만 한다.
//! 권한 검사는 모두 [`CurrentUser`]를 거친다.

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;
use sqlx::PgPool;

use super::{rules::validate_body, store, Comment};
use crate::auth::{forbidden, CurrentUser};
use crate::number::parse_number;
use crate::routes::{self, bad_request, failure, not_found};

/// 댓글 API 라우터.
pub fn api(pool: PgPool) -> Router {
    Router::new()
        .route("/api/broadcasts/{number}/comments", get(list).post(write))
        .route("/api/comments/{number}", delete(remove))
        .with_state(pool)
}

/// 댓글 쓰기 요청 본문.
#[derive(Deserialize)]
struct NewComment {
    body: String,
}

/// `GET /api/broadcasts/{번호}/comments`: 누구나. 오래된 순 `[{id, author, body, created_at, mine}]`.
/// 없거나 보이지 않는 방송은 404다.
async fn list(
    State(pool): State<PgPool>,
    current: CurrentUser,
    Path(segment): Path<String>,
) -> Response {
    let Some(broadcast) = parse_number(&segment) else {
        return not_found();
    };
    match store::broadcast_exists(&pool, broadcast).await {
        Ok(true) => {}
        Ok(false) => return not_found(),
        Err(e) => return server_error("댓글 목록 조회", e),
    }
    let viewer = current.user().map(|user| user.id);
    match store::list(&pool, broadcast).await {
        Ok(rows) => {
            let comments: Vec<Comment> = rows
                .into_iter()
                .map(|row| to_comment(row, viewer))
                .collect();
            Json(comments).into_response()
        }
        Err(e) => server_error("댓글 목록 조회", e),
    }
}

/// `POST /api/broadcasts/{번호}/comments` `{body}`: 로그인 필요. 성공하면 201과 그 댓글.
/// 본문 위반은 400, 없는 방송은 404다.
async fn write(
    State(pool): State<PgPool>,
    current: CurrentUser,
    Path(segment): Path<String>,
    body: Result<Json<NewComment>, JsonRejection>,
) -> Response {
    let user = match current.require() {
        Ok(user) => user,
        Err(response) => return response,
    };
    let Some(broadcast) = parse_number(&segment) else {
        return not_found();
    };
    let Ok(Json(new)) = body else {
        return bad_request("invalid_body", "댓글 내용을 보내 주세요.");
    };
    let text = match validate_body(&new.body) {
        Ok(text) => text,
        Err(e) => return bad_request(e.code(), e.message()),
    };
    match store::insert(&pool, broadcast, user.id, &text).await {
        Ok(Some(row)) => {
            (StatusCode::CREATED, Json(to_comment(row, Some(user.id)))).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => server_error("댓글 쓰기", e),
    }
}

/// `DELETE /api/comments/{댓글 번호}`: 작성자면 204, 아니면 403, 비로그인은 401, 없으면 404.
async fn remove(
    State(pool): State<PgPool>,
    current: CurrentUser,
    Path(segment): Path<String>,
) -> Response {
    let user = match current.require() {
        Ok(user) => user,
        Err(response) => return response,
    };
    let Some(comment) = parse_number(&segment) else {
        return comment_not_found();
    };
    match store::author_of(&pool, comment).await {
        Ok(Some(author)) if author == user.id => {}
        Ok(Some(_)) => return forbidden(),
        Ok(None) => return comment_not_found(),
        Err(e) => return server_error("댓글 삭제", e),
    }
    match store::delete(&pool, comment).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => server_error("댓글 삭제", e),
    }
}

fn to_comment(row: store::Row, viewer: Option<i64>) -> Comment {
    Comment {
        id: row.id,
        mine: viewer == Some(row.author_id),
        author: row.author,
        body: row.body,
        created_at: row.created_at,
    }
}

fn comment_not_found() -> Response {
    failure(
        StatusCode::NOT_FOUND,
        "not_found",
        "댓글을 찾을 수 없습니다.",
    )
}

fn server_error(what: &str, e: sqlx::Error) -> Response {
    routes::server_error(what, e, "요청을 처리하지 못했어요.")
}
