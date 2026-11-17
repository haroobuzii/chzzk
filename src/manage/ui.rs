//! 상세 화면의 소유자 버튼: "제목 수정"(제자리 입력)과 "삭제"(확인 창).
//!
//! 현재 사용자가 방송 소유자일 때만 보인다. 다른 소유자 버튼(예: 라이브의 "방송 종료")은
//! `children`으로 받아 같은 줄에 함께 둔다.

use dioxus::prelude::*;
use serde_json::json;

use crate::login::{call_api, use_login_state, LoginState};
use crate::{Broadcast, Route};

/// 소유자 버튼 줄. 제목을 고치면 `on_renamed`로 새 제목(앞뒤 공백을 지운 것)을 알린다.
/// 지우면 목록 화면으로 간다.
#[component]
pub fn OwnerActions(
    broadcast: ReadSignal<Broadcast>,
    on_renamed: EventHandler<String>,
    children: Element,
) -> Element {
    let mut login = use_login_state();
    let navigator = use_navigator();
    // 고치는 중이면 입력 칸의 글.
    let mut draft = use_signal(|| None::<String>);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let is_owner = match (&login(), &broadcast().owner) {
        (LoginState::User(me), Some(owner)) => me == owner,
        _ => false,
    };
    if !is_owner {
        return rsx! {};
    }

    let save = move |event: FormEvent| {
        event.prevent_default();
        let Some(title) = draft() else { return };
        if pending() {
            return;
        }
        pending.set(true);
        let id = broadcast.peek().id;
        spawn(async move {
            let url = format!("/api/broadcasts/{id}");
            match call_api("PATCH", &url, Some(json!({ "title": title }))).await {
                Ok((200, body)) => {
                    if let Some(saved) = body["title"].as_str() {
                        on_renamed.call(saved.to_string());
                    }
                    draft.set(None);
                    error.set(None);
                }
                Ok((401, body)) => {
                    login.set(LoginState::Anonymous);
                    error.set(Some(message_of(&body)));
                }
                Ok((_, body)) => error.set(Some(message_of(&body))),
                Err(message) => error.set(Some(message)),
            }
            pending.set(false);
        });
    };

    let remove = move |_| {
        if pending() {
            return;
        }
        let id = broadcast.peek().id;
        spawn(async move {
            if !confirm("정말 삭제할까요?").await {
                return;
            }
            pending.set(true);
            match call_api("DELETE", &format!("/api/broadcasts/{id}"), None).await {
                // 이미 지워졌어도 목적은 이뤘다.
                Ok((204, _)) | Ok((404, _)) => {
                    navigator.push(Route::BroadcastList {});
                }
                Ok((401, body)) => {
                    login.set(LoginState::Anonymous);
                    error.set(Some(message_of(&body)));
                }
                Ok((_, body)) => error.set(Some(message_of(&body))),
                Err(message) => error.set(Some(message)),
            }
            pending.set(false);
        });
    };

    rsx! {
        div { class: "owner-actions",
            if let Some(text) = draft() {
                form { class: "rename-form", onsubmit: save,
                    input {
                        class: "rename-input",
                        r#type: "text",
                        aria_label: "새 제목",
                        maxlength: 100,
                        value: "{text}",
                        oninput: move |event: FormEvent| draft.set(Some(event.value())),
                    }
                    button { class: "button", r#type: "submit", disabled: pending(), "저장" }
                    button {
                        class: "button",
                        r#type: "button",
                        onclick: move |_| {
                            draft.set(None);
                            error.set(None);
                        },
                        "취소"
                    }
                }
            } else {
                button {
                    class: "button",
                    r#type: "button",
                    onclick: move |_| draft.set(Some(broadcast.peek().title.clone())),
                    "제목 수정"
                }
            }
            button {
                class: "button danger-button",
                r#type: "button",
                disabled: pending(),
                onclick: remove,
                "삭제"
            }
            {children}
        }
        if let Some(message) = error() {
            p { class: "error", role: "alert", "{message}" }
        }
    }
}

/// 브라우저 확인 창을 띄우고 사용자가 "확인"을 눌렀는지 돌려준다.
async fn confirm(question: &str) -> bool {
    let question = serde_json::to_string(question).unwrap_or_default();
    let mut eval = document::eval(&format!("dioxus.send(window.confirm({question}));"));
    eval.recv::<bool>().await.unwrap_or(false)
}

fn message_of(body: &serde_json::Value) -> String {
    body["message"]
        .as_str()
        .unwrap_or("요청을 처리하지 못했어요. 잠시 뒤 다시 시도해 주세요.")
        .to_string()
}
