//! 로그인 API 라우터 테스트.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)을 쓴다. 서버를 띄우지 않고 라우터에 요청을 직접 넣어
//! 응답과 그 뒤의 조회 결과(`GET /api/auth/me`)로 판단한다.

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use sqlx::PgPool;
use tower::ServiceExt;

use super::personas::{Persona, PASSWORD};
use super::{CurrentUser, User};

/// 실제 서버처럼 `Origin` 확인을 씌운 로그인 API 라우터.
fn router(pool: &PgPool) -> Router {
    super::guard_origin(super::api(pool.clone()))
}

async fn send(router: &Router, request: Request<Body>) -> Response {
    router.clone().oneshot(request).await.unwrap()
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

fn credentials(path: &str, username: &str, password: &str) -> Request<Body> {
    Request::post(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "username": username, "password": password }).to_string(),
        ))
        .unwrap()
}

fn me_request() -> Request<Body> {
    Request::get("/api/auth/me").body(Body::empty()).unwrap()
}

/// 이 손님으로 `GET /api/auth/me`를 불러 아이디(비로그인이면 `None`)를 돌려준다.
async fn me(router: &Router, persona: &Persona) -> Option<String> {
    let response = send(router, persona.sign(me_request())).await;
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await["username"]
        .as_str()
        .map(str::to_string)
}

fn set_cookie(response: &Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("Set-Cookie가 와야 한다")
        .to_str()
        .unwrap()
        .to_string()
}

#[sqlx::test]
async fn 가입하면_201과_HttpOnly_SameSite_Lax_쿠키가_오고_그_쿠키로_me가_아이디를_준다(
    pool: PgPool,
) {
    let router = router(&pool);

    let response = send(&router, credentials("/api/auth/signup", "하루_1", PASSWORD)).await;

    assert_eq!(response.status(), StatusCode::CREATED);
    let cookie = set_cookie(&response);
    assert!(cookie.starts_with("chzzk_session="), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Lax"), "{cookie}");
    assert!(cookie.contains("Path=/"), "{cookie}");
    assert!(cookie.contains("Max-Age=604800"), "7일 유지: {cookie}");
    assert!(
        !cookie.contains("Secure"),
        "HTTP 요청에는 Secure가 없다: {cookie}"
    );
    assert_eq!(body_json(response).await["username"], "하루_1");

    let pair = cookie.split(';').next().unwrap();
    let request = Request::get("/api/auth/me")
        .header(header::COOKIE, pair)
        .body(Body::empty())
        .unwrap();
    let response = send(&router, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["username"], "하루_1");
}

#[sqlx::test]
async fn 요청이_HTTPS로_들어왔으면_쿠키에_Secure를_붙인다(pool: PgPool) {
    let router = router(&pool);
    let mut request = credentials("/api/auth/signup", "haroo", PASSWORD);
    request
        .headers_mut()
        .insert("x-forwarded-proto", "https".parse().unwrap());

    let response = send(&router, request).await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(set_cookie(&response).contains("Secure"));
}

#[sqlx::test]
async fn 비로그인이면_me가_null이다(pool: PgPool) {
    let router = router(&pool);
    assert_eq!(me(&router, &Persona::anonymous()).await, None);
}

#[sqlx::test]
async fn 규칙에_어긋난_아이디와_짧은_비밀번호는_400이다(pool: PgPool) {
    let router = router(&pool);
    for (username, password, code) in [
        ("a", PASSWORD, "invalid_username"),
        ("이름에 공백", PASSWORD, "invalid_username"),
        ("haroo🙂", PASSWORD, "invalid_username"),
        ("haroo", "1234567", "password_too_short"),
    ] {
        let response = send(&router, credentials("/api/auth/signup", username, password)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{username}");
        assert!(response.headers().get(header::SET_COOKIE).is_none());
        assert_eq!(body_json(response).await["code"], code, "{username}");
    }
    // 거절된 아이디는 만들어지지 않았다. 같은 아이디로 다시 가입할 수 있다.
    let response = send(&router, credentials("/api/auth/signup", "haroo", PASSWORD)).await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn 이미_쓰는_아이디는_대소문자가_달라도_409다(pool: PgPool) {
    let router = router(&pool);
    Persona::sign_up(&pool, "Haroo").await;

    for username in ["Haroo", "haroo", "HAROO"] {
        let response = send(&router, credentials("/api/auth/signup", username, PASSWORD)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "{username}");
        let json = body_json(response).await;
        assert_eq!(json["code"], "username_taken");
        assert_eq!(json["message"], "이미 쓰는 아이디예요.");
    }
}

#[sqlx::test]
async fn 로그인에_성공하면_200과_쿠키가_오고_그_쿠키로_로그인된다(
    pool: PgPool,
) {
    let router = router(&pool);
    Persona::sign_up(&pool, "Haroo").await;

    // 대소문자가 달라도 같은 아이디다. 응답에는 가입할 때의 아이디가 온다.
    let response = send(&router, credentials("/api/auth/login", "haroo", PASSWORD)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(set_cookie(&response).contains("HttpOnly"));
    assert_eq!(body_json(response).await["username"], "Haroo");

    let again = Persona::log_in(&pool, "Haroo").await;
    assert_eq!(me(&router, &again).await.as_deref(), Some("Haroo"));
}

#[sqlx::test]
async fn 없는_아이디와_틀린_비밀번호는_둘_다_401과_같은_메시지다(
    pool: PgPool,
) {
    let router = router(&pool);
    Persona::sign_up(&pool, "haroo").await;

    let wrong_password = send(
        &router,
        credentials("/api/auth/login", "haroo", "wrong-password"),
    )
    .await;
    let no_user = send(&router, credentials("/api/auth/login", "nobody", PASSWORD)).await;

    assert_eq!(wrong_password.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(no_user.status(), StatusCode::UNAUTHORIZED);
    assert!(wrong_password.headers().get(header::SET_COOKIE).is_none());
    let wrong_password = body_json(wrong_password).await;
    assert_eq!(wrong_password, body_json(no_user).await);
    assert_eq!(wrong_password["code"], "bad_credentials");
    assert_eq!(
        wrong_password["message"],
        "아이디 또는 비밀번호가 맞지 않아요."
    );
}

fn logout_request() -> Request<Body> {
    Request::post("/api/auth/logout")
        .body(Body::empty())
        .unwrap()
}

#[sqlx::test]
async fn 로그아웃하면_204와_만료된_쿠키가_오고_같은_쿠키로_me가_null이다(
    pool: PgPool,
) {
    let router = router(&pool);
    let user = Persona::user_a(&pool).await;
    assert_eq!(me(&router, &user).await, user.username);

    let response = send(&router, user.sign(logout_request())).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(set_cookie(&response).contains("Max-Age=0"));
    assert_eq!(me(&router, &user).await, None);
}

#[sqlx::test]
async fn 로그아웃은_이_브라우저의_세션만_지운다(pool: PgPool) {
    let router = router(&pool);
    let here = Persona::user_a(&pool).await;
    let elsewhere = Persona::log_in(&pool, "user_a").await;

    send(&router, here.sign(logout_request())).await;

    assert_eq!(me(&router, &here).await, None);
    assert_eq!(me(&router, &elsewhere).await.as_deref(), Some("user_a"));
}

#[sqlx::test]
async fn 로그인하지_않았어도_로그아웃은_204다(pool: PgPool) {
    let router = router(&pool);
    let response = send(&router, Persona::anonymous().sign(logout_request())).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn 만료_시각이_지난_세션은_로그인으로_치지_않는다(pool: PgPool) {
    let router = router(&pool);
    let user = Persona::user_a(&pool).await;
    let other = Persona::user_b(&pool).await;

    // 7일을 기다릴 수 없으므로 DB에서 user_a의 세션 만료 시각만 지난 시각으로 바꾼다.
    sqlx::query(
        "UPDATE sessions SET expires_at = now() - interval '1 second'
         WHERE user_id = (SELECT id FROM users WHERE username = 'user_a')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(me(&router, &user).await, None);
    assert_eq!(me(&router, &other).await.as_deref(), Some("user_b"));
}

#[sqlx::test]
async fn DB에는_비밀번호_원문과_세션_값_원문이_없다(pool: PgPool) {
    let user = Persona::user_a(&pool).await;
    let token = user
        .cookie()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1
        .to_string();

    // ADR 0006이 요구하는 저장 방식은 공개 API로 볼 수 없으므로 DB를 직접 읽는다.
    let password_hash: String = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!password_hash.contains(PASSWORD));
    assert!(password_hash.starts_with("$argon2id$"), "{password_hash}");

    let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_ne!(stored[0], token.as_bytes());
    assert!(!hex::encode(&stored[0]).contains(&token));
}

/// 다른 사이트에서 보낸 것처럼 `Origin`을 단다.
fn from_origin(mut request: Request<Body>, origin: &str) -> Request<Body> {
    let headers = request.headers_mut();
    headers.insert(header::HOST, "localhost:8080".parse().unwrap());
    headers.insert(header::ORIGIN, origin.parse().unwrap());
    request
}

/// 브라우저가 스스로 판단해 붙이는 `Sec-Fetch-Site`를 단다. 웹 페이지가 이 헤더를 지어낼 수는 없다.
fn fetch_site(mut request: Request<Body>, site: &str) -> Request<Body> {
    request
        .headers_mut()
        .insert("sec-fetch-site", site.parse().unwrap());
    request
}

#[sqlx::test]
async fn 개발_서버_프록시가_Host를_바꿔도_브라우저가_같은_출처라고_한_요청은_통과한다(
    pool: PgPool,
) {
    let router = router(&pool);
    let user = Persona::user_a(&pool).await;
    // `dx serve`는 요청을 앱 서버로 넘기면서 Host를 내부 주소로 바꾼다.
    let mut request = from_origin(user.sign(logout_request()), "http://localhost:8031");
    request
        .headers_mut()
        .insert(header::HOST, "127.0.0.1:51492".parse().unwrap());

    let response = send(&router, fetch_site(request, "same-origin")).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(me(&router, &user).await, None);
}

#[sqlx::test]
async fn 브라우저가_다른_사이트라고_한_요청은_403이다(pool: PgPool) {
    let router = router(&pool);
    let user = Persona::user_a(&pool).await;

    for site in ["cross-site", "same-site"] {
        let request = from_origin(user.sign(logout_request()), "https://evil.example");
        let response = send(&router, fetch_site(request, site)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{site}");
    }
    assert_eq!(me(&router, &user).await.as_deref(), Some("user_a"));
}

#[sqlx::test]
async fn 다른_출처_Origin을_단_상태_변경_요청은_쿠키가_있어도_403이다(
    pool: PgPool,
) {
    let router = router(&pool);
    let user = Persona::user_a(&pool).await;

    for origin in ["https://evil.example", "http://localhost:9999", "null"] {
        let response = send(&router, from_origin(user.sign(logout_request()), origin)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{origin}");
        assert_eq!(body_json(response).await["code"], "forbidden");
    }
    // 로그아웃되지 않았다.
    assert_eq!(me(&router, &user).await.as_deref(), Some("user_a"));

    let signup = credentials("/api/auth/signup", "attacker", PASSWORD);
    let response = send(&router, from_origin(signup, "https://evil.example")).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn 같은_출처_Origin을_단_요청은_통과한다(pool: PgPool) {
    let router = router(&pool);
    let user = Persona::user_a(&pool).await;

    let request = from_origin(user.sign(logout_request()), "http://localhost:8080");
    let response = send(&router, request).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(me(&router, &user).await, None);
}

#[sqlx::test]
async fn 터널처럼_HTTPS로_들어온_요청은_https_출처가_같은_출처다(pool: PgPool) {
    let router = router(&pool);
    let user = Persona::user_a(&pool).await;
    let mut request = from_origin(user.sign(logout_request()), "https://localhost:8080");
    request
        .headers_mut()
        .insert("x-forwarded-proto", "https".parse().unwrap());

    let response = send(&router, request).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// 로그인이 필요한 라우트를 흉내 낸다. 02~05의 핸들러가 현재 사용자 확인을 쓰는 모양과 같다.
async fn whoami(current: CurrentUser) -> Response {
    match current.require() {
        Ok(user) => Json(serde_json::json!({ "username": user.username })).into_response(),
        Err(response) => response,
    }
}

#[sqlx::test]
async fn 로그인이_필요한_요청은_비로그인과_로그아웃한_쿠키에_401이다(
    pool: PgPool,
) {
    let login_only = Router::new()
        .route("/test/login-only", post(whoami))
        .with_state(pool.clone());
    let request = || {
        Request::post("/test/login-only")
            .body(Body::empty())
            .unwrap()
    };
    let user = Persona::user_a(&pool).await;

    let response = send(&login_only, user.sign(request())).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["username"], "user_a");

    let response = send(&login_only, Persona::anonymous().sign(request())).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["code"], "login_required");

    send(&router(&pool), user.sign(logout_request())).await;
    let response = send(&login_only, user.sign(request())).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn 소유자_확인은_소유자만_통과시키고_소유자_없는_방송은_누구도_통과하지_못한다() {
    let user = User {
        id: 1,
        username: "user_a".into(),
    };
    assert!(user.require_owner(Some(1)).is_ok());
    for owner in [Some(2), None] {
        let response = user.require_owner(owner).unwrap_err();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{owner:?}");
    }
}

#[sqlx::test]
async fn 비밀번호는_128자까지이고_더_길면_가입은_400이다(pool: PgPool) {
    let router = router(&pool);

    let too_long = "a".repeat(129);
    let response = send(&router, credentials("/api/auth/signup", "haroo", &too_long)).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    assert_eq!(body_json(response).await["code"], "password_too_long");

    let longest = "a".repeat(128);
    let response = send(&router, credentials("/api/auth/signup", "haroo", &longest)).await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn 비밀번호가_128자보다_긴_비밀번호로_로그인하면_틀린_비밀번호와_같은_401이다(
    pool: PgPool,
) {
    let router = router(&pool);
    Persona::sign_up(&pool, "haroo").await;

    let response = send(
        &router,
        credentials("/api/auth/login", "haroo", &"a".repeat(129)),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    assert_eq!(body_json(response).await["code"], "bad_credentials");
}

#[sqlx::test]
async fn 로그인하면_만료된_세션은_DB에서_지운다(pool: PgPool) {
    let router = router(&pool);
    Persona::user_a(&pool).await;
    Persona::user_b(&pool).await;

    // 7일을 기다릴 수 없으므로 DB에서 두 사람의 세션 만료 시각을 지난 시각으로 바꾼다.
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();

    let response = send(&router, credentials("/api/auth/login", "user_a", PASSWORD)).await;
    assert_eq!(response.status(), StatusCode::OK);

    // 만료된 세션이 쌓이지 않는지는 공개 API로 볼 수 없으므로 DB를 직접 센다.
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 1, "방금 만든 세션만 남는다");
}
