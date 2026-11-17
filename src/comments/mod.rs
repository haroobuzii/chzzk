//! 댓글: 로그인한 사용자가 방송 하나에 남기는 1~300자 글. 쓰기, 방송별 목록(오래된 순), 지우기.
//!
//! 서버 쪽은 `GET`·`POST /api/broadcasts/{번호}/comments`와 `DELETE /api/comments/{댓글 번호}` 라우트,
//! 화면 쪽은 상세 화면 아래의 댓글 칸([`CommentSection`])이다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
mod api;
#[cfg(feature = "server")]
pub mod rules;
mod section;
#[cfg(feature = "server")]
mod store;
#[cfg(test)]
mod tests;

#[cfg(feature = "server")]
pub use api::api;
pub use section::CommentSection;

/// 댓글 하나. API 응답과 화면이 같은 모양을 쓴다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    /// 댓글 번호.
    pub id: i64,
    /// 작성자 아이디.
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    /// 요청한 사람이 작성자인지. 비로그인이면 늘 거짓이다.
    pub mine: bool,
}
