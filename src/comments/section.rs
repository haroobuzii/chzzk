//! 상세 화면 아래의 댓글 칸: 목록, 쓰기, 내 댓글 지우기.
//!
//! 댓글 API는 브라우저의 `fetch`로 부른다. 같은 출처 요청이라 세션 쿠키가 저절로 실린다.
//! 목록은 서버가 그리지 않고 화면이 뜬 뒤에 불러온다. `mine`이 쿠키에 따라 달라지기 때문이다.

use dioxus::prelude::*;
use serde_json::json;

use super::Comment;
use crate::account::{call_api, use_login_state, LoginState};
use crate::refresh::{sleep_ms, COMMENT_REFRESH_MS};
use crate::{format_kst, Route};

/// 댓글 목록 상태. `None`이면 아직 불러오지 못했다.
type Loaded = Option<Result<Vec<Comment>, String>>;

/// 댓글 칸. `live`가 참(방송 중인 라이브 방송)인 동안 3초마다 목록을 다시 불러온다.
#[component]
pub fn CommentSection(broadcast_id: ReadSignal<i64>, live: ReadSignal<bool>) -> Element {
    let mut login = use_login_state();
    let route = use_route::<Route>();
    let comments = use_signal(|| None as Loaded);
    let mut draft = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // 방송이 바뀌거나 로그인 상태가 바뀌면(`mine`이 달라진다) 다시 불러온다.
    use_effect(move || {
        let id = broadcast_id();
        let _ = login();
        spawn(load(id, comments));
    });

    // 방송 중인 동안 3초마다 다시 불러온다. 방송이 끝나면 멈춘다.
    use_effect(move || {
        if live() {
            spawn(async move {
                while *live.peek() {
                    sleep_ms(COMMENT_REFRESH_MS).await;
                    if *live.peek() {
                        load(*broadcast_id.peek(), comments).await;
                    }
                }
            });
        }
    });

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        pending.set(true);
        error.set(None);
        spawn(async move {
            let id = *broadcast_id.peek();
            let url = format!("/api/broadcasts/{id}/comments");
            match call_api("POST", &url, Some(json!({ "body": draft() }))).await {
                Ok((201, _)) => {
                    draft.set(String::new());
                    load(id, comments).await;
                }
                Ok((401, body)) => {
                    // 다른 창에서 로그아웃했거나 로그인이 만료됐다. 로그인 안내로 바꾼다.
                    login.set(LoginState::Anonymous);
                    error.set(Some(message_of(&body)));
                }
                Ok((_, body)) => error.set(Some(message_of(&body))),
                Err(message) => error.set(Some(message)),
            }
            pending.set(false);
        });
    };

    let remove = move |comment: i64| {
        spawn(async move {
            match call_api("DELETE", &format!("/api/comments/{comment}"), None).await {
                Ok((204, _)) | Ok((404, _)) => {
                    error.set(None);
                    load(*broadcast_id.peek(), comments).await;
                }
                Ok((_, body)) => error.set(Some(message_of(&body))),
                Err(message) => error.set(Some(message)),
            }
        });
    };

    let list = match comments() {
        None => rsx! {
            p { class: "comment-empty", "댓글을 불러오는 중…" }
        },
        Some(Err(message)) => rsx! {
            p { class: "error", role: "alert", "{message}" }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            p { class: "comment-empty", "아직 댓글이 없습니다" }
        },
        Some(Ok(list)) => rsx! {
            ul { class: "comment-list",
                for comment in list {
                    li { key: "{comment.id}", class: "comment",
                        div { class: "comment-meta",
                            span { class: "comment-author", "{comment.author}" }
                            span { class: "card-time", "{format_kst(&comment.created_at)}" }
                            if comment.mine {
                                button {
                                    class: "comment-delete",
                                    r#type: "button",
                                    onclick: move |_| remove(comment.id),
                                    "삭제"
                                }
                            }
                        }
                        p { class: "comment-body", "{comment.body}" }
                    }
                }
            }
        },
    };

    let compose = match login() {
        // 서버가 그린 첫 화면에서는 로그인 여부를 아직 모른다.
        LoginState::Unknown => rsx! {},
        LoginState::Anonymous => rsx! {
            p { class: "comment-login",
                "로그인하고 댓글을 남겨 보세요 "
                Link {
                    class: "button secondary-button",
                    to: Route::Login { next: route.to_string() },
                    "로그인"
                }
            }
        },
        LoginState::User(_) => rsx! {
            form { class: "comment-form", onsubmit: submit,
                textarea {
                    name: "body",
                    rows: 2,
                    maxlength: 300,
                    placeholder: "댓글을 남겨 보세요 (300자까지)",
                    value: "{draft}",
                    disabled: pending(),
                    oninput: move |e| draft.set(e.value()),
                }
                button { class: "button", r#type: "submit", disabled: pending(),
                    if pending() { "등록하는 중…" } else { "등록" }
                }
            }
        },
    };

    rsx! {
        section { class: "comments",
            h2 { class: "section-title", "댓글" }
            {list}
            {compose}
            if let Some(message) = error() {
                p { class: "error", role: "alert", "{message}" }
            }
        }
    }
}

/// 방송의 댓글 목록을 불러와 `comments`에 넣는다.
/// 이미 목록이 있으면 실패해도 그대로 두고, 다음에 다시 묻는다.
async fn load(id: i64, mut comments: Signal<Loaded>) {
    let failed = match call_api("GET", &format!("/api/broadcasts/{id}/comments"), None).await {
        Ok((200, body)) => match serde_json::from_value::<Vec<Comment>>(body) {
            Ok(list) => {
                if comments.peek().as_ref() != Some(&Ok(list.clone())) {
                    comments.set(Some(Ok(list)));
                }
                return;
            }
            Err(_) => "댓글을 불러오지 못했어요. 잠시 뒤 다시 시도해 주세요.".to_string(),
        },
        Ok((_, body)) => message_of(&body),
        Err(message) => message,
    };
    if !matches!(*comments.peek(), Some(Ok(_))) {
        comments.set(Some(Err(failed)));
    }
}

/// 실패 응답 `{code, message}`에서 사용자에게 보여 줄 문구를 꺼낸다.
fn message_of(body: &serde_json::Value) -> String {
    body["message"]
        .as_str()
        .unwrap_or("요청을 처리하지 못했어요. 잠시 뒤 다시 시도해 주세요.")
        .to_string()
}
