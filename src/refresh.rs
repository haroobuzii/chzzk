//! 목록과 시청 화면이 함께 쓰는 "잠깐 기다렸다 다시 묻기".

use dioxus::prelude::*;

/// 상태를 다시 확인하는 간격.
pub const REFRESH_MS: u32 = 5000;

/// 방송 중인 라이브 방송의 댓글을 다시 불러오는 간격. 채팅처럼 대화를 따라가도록 상태 확인보다 짧다.
pub const COMMENT_REFRESH_MS: u32 = 3000;

/// 브라우저에서 `ms`밀리초 기다린다.
pub async fn sleep_ms(ms: u32) {
    let mut wait = document::eval(&format!("setTimeout(() => dioxus.send(true), {ms});"));
    let _ = wait.recv::<bool>().await;
}
