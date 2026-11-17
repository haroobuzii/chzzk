//! 로그인 화면, 회원가입 화면, 머리글의 로그인 상태.
//!
//! 계정 API(`/api/auth/*`)는 브라우저의 `fetch`로 부른다. 같은 출처 요청이라 세션 쿠키가 저절로 실린다.
//! 쿠키는 `HttpOnly`라 화면 코드가 읽을 수 없으므로, 누가 로그인했는지는 `GET /api/auth/me`로 묻는다.

use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::Route;

/// 머리글과 화면들이 함께 보는 로그인 상태.
#[derive(Clone, Debug, PartialEq)]
pub enum LoginState {
    /// 아직 서버에 묻지 않았다(서버가 그린 첫 화면).
    Unknown,
    /// 로그인하지 않았다.
    Anonymous,
    /// 이 아이디로 로그인했다.
    User(String),
}

/// 화면 전체에 로그인 상태를 둔다. `App`에서 한 번 부른다.
pub fn provide_login_state() {
    use_context_provider(|| Signal::new(LoginState::Unknown));
}

/// 화면 전체가 함께 보는 로그인 상태.
pub fn use_login_state() -> Signal<LoginState> {
    use_context::<Signal<LoginState>>()
}

/// 서버에 지금 로그인 상태를 물어 `state`에 넣는다. 실패하면 그대로 둔다.
async fn refresh_login_state(mut state: Signal<LoginState>) {
    if let Ok((200, body)) = call_api("GET", "/api/auth/me", None).await {
        state.set(match body["username"].as_str() {
            Some(username) => LoginState::User(username.to_string()),
            None => LoginState::Anonymous,
        });
    }
}

/// 머리글 오른쪽의 로그인 칸: 로그인 전에는 "로그인", 후에는 "아이디님"과 "로그아웃".
#[component]
pub fn HeaderLogin() -> Element {
    let mut state = use_login_state();
    let route = use_route::<Route>();
    // 첫 화면이 뜨면 한 번 묻는다. 화면을 옮겨 다녀도 다시 묻지 않는다.
    use_effect(move || {
        if *state.peek() == LoginState::Unknown {
            spawn(refresh_login_state(state));
        }
    });

    let logout = move |_| {
        spawn(async move {
            if let Ok((204, _)) = call_api("POST", "/api/auth/logout", None).await {
                state.set(LoginState::Anonymous);
            }
        });
    };

    // 로그인 화면에서 다시 "로그인"을 누르면, 원래 돌아갈 주소를 그대로 둔다.
    let next = match &route {
        Route::Login { next } | Route::Signup { next } => next.clone(),
        other => other.to_string(),
    };

    match state() {
        LoginState::Unknown => rsx! {},
        LoginState::Anonymous => rsx! {
            Link { class: "button secondary-button", to: Route::Login { next }, "로그인" }
        },
        LoginState::User(username) => rsx! {
            span { class: "header-user", title: "{username}", "{username}님" }
            button { class: "button secondary-button", r#type: "button", onclick: logout, "로그아웃" }
        },
    }
}

/// 로그인한 사람에게만 `children`을 보여 준다(녹화 올리기, 라이브 시작).
/// 비로그인이면 로그인 화면(`/login?next=<지금 주소>`)으로 보내므로, 로그인 뒤 이 화면으로 돌아온다.
/// 로그인 상태를 묻는 동안에는 안내 문구만 보여 준다. 그래서 비로그인에게 카메라 권한 등을 묻지 않는다.
#[component]
pub fn RequireLogin(children: Element) -> Element {
    let state = use_login_state();
    let route = use_route::<Route>();
    let navigator = use_navigator();
    // 로그인 상태가 바뀔 때마다(처음 확인, 로그아웃) 다시 본다.
    use_effect(move || {
        if state() == LoginState::Anonymous {
            navigator.replace(Route::Login {
                next: route.to_string(),
            });
        }
    });

    match state() {
        LoginState::User(_) => children,
        _ => rsx! {
            p { "로그인 상태를 확인하는 중…" }
        },
    }
}

/// 로그인 뒤에 돌아갈 이 서비스 안의 화면. `next`가 `/`로 시작하고 `//`로 시작하지 않을 때만 받는다.
/// 아니면 첫 화면이다. 다른 사이트로 보내는 링크에 속지 않기 위해서다.
pub fn safe_next(next: &str) -> Route {
    let inside = next.starts_with('/') && !next.starts_with("//") && !next.starts_with("/\\");
    if inside {
        next.parse().unwrap_or(Route::BroadcastList {})
    } else {
        Route::BroadcastList {}
    }
}

/// 로그인 화면(`/login?next=<원래 주소>`).
#[component]
pub fn Login(next: String) -> Element {
    rsx! {
        h1 { "로그인" }
        CredentialsForm { kind: FormKind::Login, next: next.clone() }
        p { class: "form-switch",
            "아직 사용자가 아니라면 "
            Link { to: Route::Signup { next }, "회원가입" }
        }
    }
}

/// 회원가입 화면(`/signup`). 가입이 끝나면 바로 로그인된다.
#[component]
pub fn Signup(next: String) -> Element {
    rsx! {
        h1 { "회원가입" }
        CredentialsForm { kind: FormKind::Signup, next: next.clone() }
        p { class: "form-switch",
            "이미 가입했다면 "
            Link { to: Route::Login { next }, "로그인" }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum FormKind {
    Login,
    Signup,
}

/// 아이디·비밀번호 입력 칸. 성공하면 로그인 상태를 바꾸고 `next`로 간다.
#[component]
fn CredentialsForm(kind: FormKind, next: String) -> Element {
    let mut state = use_login_state();
    let navigator = use_navigator();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let (path, success, button, pending_label) = match kind {
        FormKind::Login => ("/api/auth/login", 200, "로그인", "로그인하는 중…"),
        FormKind::Signup => ("/api/auth/signup", 201, "회원가입", "가입하는 중…"),
    };

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        pending.set(true);
        error.set(None);
        let next = next.clone();
        spawn(async move {
            let body = json!({ "username": username(), "password": password() });
            match call_api("POST", path, Some(body)).await {
                Ok((status, body)) if status == success => {
                    if let Some(name) = body["username"].as_str() {
                        state.set(LoginState::User(name.to_string()));
                    }
                    navigator.replace(safe_next(&next));
                }
                Ok((_, body)) => error.set(Some(
                    body["message"]
                        .as_str()
                        .unwrap_or("요청을 처리하지 못했어요. 잠시 뒤 다시 시도해 주세요.")
                        .to_string(),
                )),
                Err(message) => error.set(Some(message)),
            }
            pending.set(false);
        });
    };

    rsx! {
        form { class: "register-form", onsubmit: submit,
            label {
                span { "아이디" }
                input {
                    name: "username",
                    r#type: "text",
                    autocomplete: "username",
                    value: "{username}",
                    disabled: pending(),
                    oninput: move |e| username.set(e.value()),
                }
                if kind == FormKind::Signup {
                    small { class: "hint", "2~20자의 한글, 영문, 숫자, 밑줄(_)" }
                }
            }
            label {
                span { "비밀번호" }
                input {
                    name: "password",
                    r#type: "password",
                    autocomplete: if kind == FormKind::Signup { "new-password" } else { "current-password" },
                    value: "{password}",
                    disabled: pending(),
                    oninput: move |e| password.set(e.value()),
                }
                if kind == FormKind::Signup {
                    small { class: "hint", "8자 이상" }
                }
            }
            button { class: "button", r#type: "submit", disabled: pending(),
                if pending() { "{pending_label}" } else { "{button}" }
            }
            if let Some(message) = error() {
                p { class: "error", role: "alert", "{message}" }
            }
        }
    }
}

/// 브라우저의 `fetch`로 JSON API를 부르고 `(상태 코드, 응답 JSON)`을 돌려준다.
/// 서버에 닿지 못하면 사용자에게 보여 줄 문구를 돌려준다.
async fn call_api(
    method: &str,
    url: &str,
    body: Option<serde_json::Value>,
) -> Result<(u16, serde_json::Value), String> {
    const UNREACHABLE: &str = "서버에 연결하지 못했어요. 잠시 뒤 다시 시도해 주세요.";
    let mut script = document::eval(FETCH_SCRIPT);
    script
        .send(json!({ "method": method, "url": url, "body": body }))
        .map_err(|_| UNREACHABLE.to_string())?;
    let reply: FetchReply = script.recv().await.map_err(|_| UNREACHABLE.to_string())?;
    if reply.status == 0 {
        return Err(UNREACHABLE.into());
    }
    Ok((
        reply.status,
        serde_json::from_str(&reply.body).unwrap_or_default(),
    ))
}

#[derive(Deserialize)]
struct FetchReply {
    /// 0이면 서버에 닿지 못했다.
    status: u16,
    body: String,
}

/// Rust 쪽에서 `{method, url, body}`를 받아 `fetch`로 보내고 `{status, body}`를 돌려준다.
const FETCH_SCRIPT: &str = r#"
const { method, url, body } = await dioxus.recv();
try {
  const res = await fetch(url, {
    method,
    credentials: "same-origin",
    headers: body == null ? {} : { "Content-Type": "application/json" },
    body: body == null ? undefined : JSON.stringify(body),
  });
  dioxus.send({ status: res.status, body: await res.text() });
} catch (e) {
  dioxus.send({ status: 0, body: "" });
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 이_서비스_안의_주소로만_돌아간다() {
        assert!(safe_next("/new") == Route::NewBroadcast {});
        assert!(
            safe_next("/broadcasts/3")
                == Route::BroadcastDetail {
                    segment: "3".into()
                }
        );
        for outside in [
            "",
            "https://evil.example",
            "//evil.example",
            "/\\evil.example",
            "evil.example/new",
            "javascript:alert(1)",
        ] {
            assert!(
                safe_next(outside) == Route::BroadcastList {},
                "{outside:?}는 첫 화면으로 가야 한다"
            );
        }
    }
}
