//! 서버 함수로 만들지 않고 직접 덧붙이는 HTTP 라우트: 방송 등록 API와 영상 전송.
//!
//! 등록은 큰 영상 파일을 받으므로 본문을 메모리에 모으는 서버 함수 대신
//! axum 라우트에서 multipart를 조각조각 읽어 임시 파일에 바로 쓴다.

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, FromRef, Multipart, Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use sqlx::PgPool;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{
    auth::{CurrentUser, User},
    config::Config,
    number::parse_number,
    title::{validate_title, InvalidTitle},
    videos::VideoStore,
};

/// 라우트들이 함께 쓰는 것: DB와 영상 보관소, 설정.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub videos: VideoStore,
    pub max_upload_bytes: u64,
}

impl AppState {
    pub fn new(pool: PgPool, config: &Config) -> Self {
        Self {
            pool,
            videos: VideoStore::new(&config.data_dir),
            max_upload_bytes: config.max_upload_bytes,
        }
    }
}

/// 현재 사용자 확인([`CurrentUser`])이 세션을 찾을 DB.
impl FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

/// multipart 경계선과 제목 칸에 쓰일 여유분. 영상 크기는 따로 센다.
const MULTIPART_OVERHEAD: u64 = 1024 * 1024;

/// 등록 API와 영상 전송 라우터.
pub fn api(state: AppState) -> Router {
    let body_limit = state.max_upload_bytes.saturating_add(MULTIPART_OVERHEAD);
    Router::new()
        .route(
            "/api/broadcasts",
            // 이 라우트만 기본 본문 크기 제한(2 MB)을 업로드 최대 크기에 맞춰 늘린다.
            post(register).layer(DefaultBodyLimit::max(
                usize::try_from(body_limit).unwrap_or(usize::MAX),
            )),
        )
        .route("/api/broadcasts/{number}/video", get(video))
        .with_state(state)
}

/// `GET /api/broadcasts/{번호}/video`: 영상 원본을 변환 없이 보낸다(ADR 0001).
///
/// Range 요청에는 206과 요청한 부분만 보내므로, 브라우저가 끝까지 내려받지 않고도 건너뛸 수 있다.
async fn video(
    State(state): State<AppState>,
    Path(segment): Path<String>,
    request: Request<Body>,
) -> Response {
    let Some(id) = parse_number(&segment) else {
        return not_found();
    };
    let key = match crate::db::find_video_key(&state.pool, id).await {
        Ok(Some(key)) => key,
        Ok(None) => return not_found(),
        Err(e) => return server_error("영상 위치 조회", e, "영상을 불러오지 못했어요."),
    };

    // Range 처리는 검증된 구현(tower-http)에 맡긴다. 확장자가 .mp4라서 Content-Type은 video/mp4가 된다.
    let file = ServeFile::new(state.videos.path_of(&key));
    match file.oneshot(request).await {
        Ok(response) if response.status() == StatusCode::NOT_FOUND => not_found(),
        Ok(response) => response.map(Body::new),
        Err(never) => match never {},
    }
}

pub(crate) fn not_found() -> Response {
    failure(
        StatusCode::NOT_FOUND,
        "not_found",
        "방송을 찾을 수 없습니다.",
    )
}

/// `POST /api/broadcasts`: multipart의 `title`과 `video`로 방송을 등록한다.
///
/// 로그인이 필요하다. 비로그인이면 본문을 읽기 전에 401로 거절한다.
/// 성공하면 201과 `{"id": 방송 번호}`를 돌려주고, 로그인한 사용자가 소유자가 된다.
/// 중간에 실패하거나 연결이 끊기면 임시 파일은 저절로 지워지고 방송 정보도 남지 않는다.
async fn register(
    State(state): State<AppState>,
    current: CurrentUser,
    multipart: Multipart,
) -> Response {
    let owner = match current.require() {
        Ok(user) => user,
        Err(response) => return response,
    };
    match try_register(&state, &owner, multipart).await {
        Ok(id) => (StatusCode::CREATED, Json(json!({ "id": id }))).into_response(),
        Err(response) => response,
    }
}

async fn try_register(
    state: &AppState,
    owner: &User,
    mut multipart: Multipart,
) -> Result<i64, Response> {
    let mut title = None;
    let mut upload = None;

    while let Some(field) = multipart.next_field().await.map_err(|_| broken_upload())? {
        match field.name() {
            // 제목이 영상보다 먼저 오므로(등록 화면이 그렇게 보낸다), 잘못된 제목이면 큰 영상을 받기 전에 거절한다.
            Some("title") => {
                let text = read_title(field).await?;
                title =
                    Some(validate_title(&text).map_err(|e| bad_request(e.code(), e.message()))?);
            }
            Some("video") => upload = Some(receive_video(state, field).await?),
            // 모르는 칸은 읽지 않고 넘긴다.
            _ => {}
        }
    }

    let Some(title) = title else {
        let empty = InvalidTitle::Empty;
        return Err(bad_request(empty.code(), empty.message()));
    };
    let Some(upload) = upload else {
        return Err(bad_request("missing_video", "영상 파일을 골라 주세요."));
    };

    save(state, owner, &title, upload)
        .await
        .map_err(|e| server_error("방송 등록", e, "등록하지 못했어요."))
}

/// 제목 칸으로 받을 최대 바이트. 100자 × 한 글자 최대 4바이트에 앞뒤 공백 여유를 더한 값이다.
/// 본문 크기 제한은 영상 때문에 크므로, 제목 칸을 메모리에 모을 때는 따로 막는다.
const MAX_TITLE_BYTES: usize = 4096;

/// 제목 칸을 읽는다. 너무 길면 끝까지 읽지 않고 거절한다.
async fn read_title(mut field: axum::extract::multipart::Field<'_>) -> Result<String, Response> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|_| broken_upload())? {
        if bytes.len() + chunk.len() > MAX_TITLE_BYTES {
            let too_long = InvalidTitle::TooLong;
            return Err(bad_request(too_long.code(), too_long.message()));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| broken_upload())
}

/// MP4 파일 앞부분 4~8바이트에 있는 표시.
const MP4_SIGNATURE: &[u8; 4] = b"ftyp";

/// 영상 칸을 조각조각 읽어 임시 파일에 바로 쓴다. 파일 전체를 메모리에 모으지 않는다.
///
/// 확장자와 앞부분 표시로 MP4인지 보고, 최대 크기를 넘으면 그 즉시 멈춘다. 코덱은 검사하지 않는다(ADR 0001).
/// 어느 쪽이든 실패하면 임시 파일은 저절로 지워진다.
async fn receive_video(
    state: &AppState,
    mut field: axum::extract::multipart::Field<'_>,
) -> Result<NamedTempFile, Response> {
    let has_mp4_extension = field
        .file_name()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("mp4"));
    if !has_mp4_extension {
        return Err(not_mp4());
    }

    let temp = state.videos.create_temp().map_err(video_save_error)?;
    let mut file = tokio::fs::File::from_std(temp.as_file().try_clone().map_err(video_save_error)?);
    let mut received: u64 = 0;
    // 표시를 확인할 앞부분 8바이트. 조각이 작게 나뉘어 와도 모아서 본다.
    let mut head = Vec::with_capacity(8);

    while let Some(chunk) = field.chunk().await.map_err(|_| broken_upload())? {
        received += chunk.len() as u64;
        if received > state.max_upload_bytes {
            return Err(failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                "too_large",
                &format!(
                    "영상 파일은 {}까지 올릴 수 있어요.",
                    human_size(state.max_upload_bytes)
                ),
            ));
        }
        if head.len() < 8 {
            let take = (8 - head.len()).min(chunk.len());
            head.extend_from_slice(&chunk[..take]);
        }
        file.write_all(&chunk).await.map_err(video_save_error)?;
    }

    if head.len() < 8 || &head[4..8] != MP4_SIGNATURE {
        return Err(not_mp4());
    }
    file.flush().await.map_err(video_save_error)?;
    Ok(temp)
}

/// 바이트 수를 사람이 읽기 좋게 쓴다. 예: 2 GiB → "2GB".
fn human_size(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= GB && bytes % GB == 0 {
        format!("{}GB", bytes / GB)
    } else if bytes >= MB {
        format!("{}MB", bytes / MB)
    } else {
        format!("{bytes}바이트")
    }
}

fn not_mp4() -> Response {
    bad_request("not_mp4", "MP4 영상 파일만 올릴 수 있어요.")
}

pub(crate) fn bad_request(code: &str, message: &str) -> Response {
    failure(StatusCode::BAD_REQUEST, code, message)
}

/// 방송 정보와 영상을 함께 저장한다. 둘 중 하나만 남는 일이 없게 한다.
///
/// 1. 트랜잭션 안에서 방송 정보를 넣어 번호를 받고 영상 키를 정한다.
/// 2. 임시 파일을 영상 키 위치로 옮긴다. 실패하면 트랜잭션이 되돌려진다.
/// 3. 커밋한다. 실패하면 옮긴 파일을 지운다.
async fn save(
    state: &AppState,
    owner: &User,
    title: &str,
    upload: NamedTempFile,
) -> anyhow::Result<i64> {
    let mut tx = state.pool.begin().await?;
    let id = crate::db::insert_broadcast(&mut tx, title).await?;
    crate::db::set_owner(&mut tx, id, owner.id).await?;
    let key = VideoStore::key_for(id);
    crate::db::set_video_key(&mut tx, id, &key).await?;

    state.videos.persist(upload, &key)?;

    if let Err(e) = tx.commit().await {
        state.videos.remove(&key);
        return Err(e.into());
    }
    Ok(id)
}

/// 업로드 본문이 깨졌거나 중간에 끊긴 경우.
fn broken_upload() -> Response {
    bad_request(
        "broken_upload",
        "업로드가 중간에 끊겼어요. 다시 시도해 주세요.",
    )
}

fn video_save_error(e: std::io::Error) -> Response {
    server_error("영상 저장", e, "영상을 저장하지 못했어요.")
}

/// 실패 응답: 상태 코드와 원인 코드, 한국어 메시지.
pub(crate) fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "code": code, "message": message }))).into_response()
}

/// 서버 쪽 실패(DB·파일 등)의 500 응답. `{what} 실패: {e}`를 로그에 남기고,
/// 사용자에게는 `message`(예: "방송을 지우지 못했어요.")에 다시 시도하라는 말을 붙여 보낸다.
pub(crate) fn server_error(what: &str, e: impl std::fmt::Display, message: &str) -> Response {
    dioxus::logger::tracing::error!("{what} 실패: {e}");
    failure(
        StatusCode::INTERNAL_SERVER_ERROR,
        "server_error",
        &format!("{message} 잠시 뒤 다시 시도해 주세요."),
    )
}

/// 이 테스트들은 실제 PostgreSQL이 필요하다(`#[sqlx::test]`가 테스트마다 임시 DB를 만든다).
/// 서버를 띄우지 않고 라우터에 요청을 직접 넣어 응답과 그 뒤의 조회 결과를 본다.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tempfile::TempDir;

    pub use crate::auth::personas::Persona;

    /// `ftyp` 시그니처만 갖춘 아주 작은 가짜 MP4. 코덱은 검사하지 않으므로 이것으로 충분하다.
    pub fn tiny_mp4() -> Vec<u8> {
        let mut bytes = vec![0, 0, 0, 0x18];
        bytes.extend_from_slice(b"ftypisom");
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(b"rest of the video");
        bytes
    }

    pub struct TestApp {
        pub router: Router,
        pub pool: PgPool,
        pub data_dir: TempDir,
    }

    pub fn app(pool: PgPool) -> TestApp {
        app_with_limit(pool, 2 * 1024 * 1024 * 1024)
    }

    pub fn app_with_limit(pool: PgPool, max_upload_bytes: u64) -> TestApp {
        let data_dir = tempfile::tempdir().unwrap();
        let state = AppState {
            pool: pool.clone(),
            videos: VideoStore::new(data_dir.path()),
            max_upload_bytes,
        };
        // 요청 도우미가 붙이는 테스트 사용자의 세션을, 요청이 라우터에 닿기 전에 DB에 만들어 둔다.
        // 이 함수는 동기라서 여기서 바로 DB에 쓸 수 없다.
        let router = api(state).layer(axum::middleware::from_fn_with_state(
            pool.clone(),
            ensure_tester,
        ));
        TestApp {
            router,
            pool,
            data_dir,
        }
    }

    async fn ensure_tester(
        State(pool): State<PgPool>,
        request: Request<Body>,
        next: axum::middleware::Next,
    ) -> Response {
        Persona::ensure_tester(&pool).await;
        next.run(request).await
    }

    const BOUNDARY: &str = "----chzzk-test-boundary";

    /// multipart 본문을 만든다. `video`가 `None`이면 파일 칸을 넣지 않는다.
    pub fn multipart_body(title: Option<&str>, video: Option<(&str, &[u8])>) -> Vec<u8> {
        let mut body = Vec::new();
        if let Some(title) = title {
            body.extend_from_slice(
                format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\n{title}\r\n")
                    .as_bytes(),
            );
        }
        if let Some((filename, bytes)) = video {
            body.extend_from_slice(
                format!(
                    "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"video\"; filename=\"{filename}\"\r\nContent-Type: video/mp4\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
        body
    }

    /// 등록 요청. 로그인한 테스트 사용자([`Persona::tester`])의 쿠키가 붙는다.
    pub fn register_request(body: Vec<u8>) -> Request<Body> {
        register_request_as(&Persona::tester(), body)
    }

    /// `persona`가 보내는 등록 요청. 비로그인이면 쿠키가 없다.
    pub fn register_request_as(persona: &Persona, body: Vec<u8>) -> Request<Body> {
        persona.sign(
            Request::post("/api/broadcasts")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
    }

    pub async fn send(router: &Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// 정상 등록을 하고 새 방송 번호를 돌려준다.
    pub async fn register_ok(app: &TestApp, title: &str) -> i64 {
        let body = multipart_body(Some(title), Some(("clip.mp4", &tiny_mp4())));
        let (status, json) = send(&app.router, register_request(body)).await;
        assert_eq!(status, StatusCode::CREATED, "응답: {json}");
        json["id"].as_i64().expect("응답에 방송 번호가 있어야 한다")
    }

    #[sqlx::test]
    async fn 제목과_MP4로_등록하면_201과_번호가_오고_목록과_조회에_나타난다(
        pool: PgPool,
    ) {
        let app = app(pool);

        let id = register_ok(&app, "첫 방송").await;

        let list = crate::db::list_broadcasts(&app.pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].title, "첫 방송");

        let found = crate::db::find_broadcast(&app.pool, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.title, "첫 방송");
    }

    #[sqlx::test]
    async fn 영상은_보관_폴더에_저장되고_DB에는_상대_위치가_남는다(
        pool: PgPool,
    ) {
        let app = app(pool);

        let id = register_ok(&app, "영상 확인").await;

        let key = crate::db::find_video_key(&app.pool, id)
            .await
            .unwrap()
            .unwrap();
        assert!(!key.starts_with('/'), "영상 키는 상대 위치여야 한다: {key}");
        let saved = std::fs::read(app.data_dir.path().join(&key)).unwrap();
        assert_eq!(saved, tiny_mp4());
    }

    #[sqlx::test]
    async fn 여러_개를_등록하면_목록은_최신순이다(pool: PgPool) {
        let app = app(pool);

        let first = register_ok(&app, "하나").await;
        let second = register_ok(&app, "둘").await;
        let third = register_ok(&app, "셋").await;

        let ids: Vec<i64> = crate::db::list_broadcasts(&app.pool)
            .await
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(ids, vec![third, second, first]);
    }

    /// 보관 폴더 안에 남은 파일 수(임시 파일 포함).
    fn leftover_files(dir: &std::path::Path) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        entries
            .map(|entry| entry.unwrap().path())
            .map(|path| {
                if path.is_dir() {
                    leftover_files(&path)
                } else {
                    1
                }
            })
            .sum()
    }

    /// 거절된 등록이 방송도 파일도 남기지 않았는지 본다.
    async fn assert_nothing_left(app: &TestApp) {
        assert!(
            crate::db::list_broadcasts(&app.pool)
                .await
                .unwrap()
                .is_empty(),
            "방송이 남으면 안 된다"
        );
        assert_eq!(
            leftover_files(app.data_dir.path()),
            0,
            "보관 폴더에 파일이 남으면 안 된다"
        );
    }

    async fn assert_rejected(app: &TestApp, body: Vec<u8>, status: StatusCode, code: &str) {
        let (actual, json) = send(&app.router, register_request(body)).await;
        assert_eq!(actual, status, "응답: {json}");
        assert_eq!(json["code"], code);
        assert!(
            json["message"].as_str().is_some_and(|m| !m.is_empty()),
            "한국어 이유가 있어야 한다"
        );
        assert_nothing_left(app).await;
    }

    #[sqlx::test]
    async fn 빈_제목과_공백뿐인_제목은_400이다(pool: PgPool) {
        let app = app(pool);
        for title in ["", "   "] {
            let body = multipart_body(Some(title), Some(("clip.mp4", &tiny_mp4())));
            assert_rejected(&app, body, StatusCode::BAD_REQUEST, "title_empty").await;
        }
        let body = multipart_body(None, Some(("clip.mp4", &tiny_mp4())));
        assert_rejected(&app, body, StatusCode::BAD_REQUEST, "title_empty").await;
    }

    #[sqlx::test]
    async fn 백_자를_넘는_제목은_400이다(pool: PgPool) {
        let app = app(pool);
        let body = multipart_body(Some(&"가".repeat(101)), Some(("clip.mp4", &tiny_mp4())));
        assert_rejected(&app, body, StatusCode::BAD_REQUEST, "title_too_long").await;
    }

    #[sqlx::test]
    async fn 아주_긴_제목_칸은_메모리에_모으지_않고_400이다(pool: PgPool) {
        let app = app(pool);
        let body = multipart_body(
            Some(&"a".repeat(1024 * 1024)),
            Some(("clip.mp4", &tiny_mp4())),
        );
        assert_rejected(&app, body, StatusCode::BAD_REQUEST, "title_too_long").await;
    }

    #[sqlx::test]
    async fn 앞뒤_공백은_지우고_저장한다(pool: PgPool) {
        let app = app(pool);
        let id = register_ok(&app, "  공백 제목  ").await;
        let found = crate::db::find_broadcast(&app.pool, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.title, "공백 제목");
    }

    #[sqlx::test]
    async fn 파일이_없으면_400이다(pool: PgPool) {
        let app = app(pool);
        let body = multipart_body(Some("파일 없음"), None);
        assert_rejected(&app, body, StatusCode::BAD_REQUEST, "missing_video").await;
    }

    #[sqlx::test]
    async fn 확장자가_mp4가_아니면_400이다(pool: PgPool) {
        let app = app(pool);
        for filename in ["clip.mov", "clip", "clip.mp4.txt"] {
            let body = multipart_body(Some("다른 형식"), Some((filename, &tiny_mp4())));
            assert_rejected(&app, body, StatusCode::BAD_REQUEST, "not_mp4").await;
        }
    }

    #[sqlx::test]
    async fn 확장자는_대소문자를_가리지_않는다(pool: PgPool) {
        let app = app(pool);
        let body = multipart_body(Some("대문자"), Some(("CLIP.MP4", &tiny_mp4())));
        let (status, json) = send(&app.router, register_request(body)).await;
        assert_eq!(status, StatusCode::CREATED, "응답: {json}");
    }

    #[sqlx::test]
    async fn 확장자만_mp4인_다른_파일은_400이다(pool: PgPool) {
        let app = app(pool);
        let fake_files: [&[u8]; 3] = [b"this is just text, not a video", b"", b"\0\0\0\x18ftx"];
        for bytes in fake_files {
            let body = multipart_body(Some("가짜"), Some(("clip.mp4", bytes)));
            assert_rejected(&app, body, StatusCode::BAD_REQUEST, "not_mp4").await;
        }
    }

    #[sqlx::test]
    async fn 최대_크기를_넘으면_413이다(pool: PgPool) {
        let video = tiny_mp4();
        let app = app_with_limit(pool, (video.len() - 1) as u64);
        let body = multipart_body(Some("너무 큼"), Some(("clip.mp4", &video)));
        assert_rejected(&app, body, StatusCode::PAYLOAD_TOO_LARGE, "too_large").await;
    }

    #[sqlx::test]
    async fn 최대_크기와_같으면_받는다(pool: PgPool) {
        let video = tiny_mp4();
        let app = app_with_limit(pool, video.len() as u64);
        let body = multipart_body(Some("딱 맞음"), Some(("clip.mp4", &video)));
        let (status, json) = send(&app.router, register_request(body)).await;
        assert_eq!(status, StatusCode::CREATED, "응답: {json}");
    }

    #[sqlx::test]
    async fn 업로드가_중간에_끊기면_아무것도_남지_않는다(pool: PgPool) {
        let app = app(pool);
        let mut body = multipart_body(Some("끊김"), Some(("clip.mp4", &tiny_mp4())));
        // 영상 중간에서 본문을 자른다. 마지막 경계선이 오지 않는다.
        body.truncate(body.len() - 30);
        let (status, _) = send(&app.router, register_request(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_nothing_left(&app).await;
    }

    #[sqlx::test]
    async fn 거절된_뒤에_같은_파일로_다시_등록할_수_있다(pool: PgPool) {
        let app = app(pool);
        let body = multipart_body(Some(""), Some(("clip.mp4", &tiny_mp4())));
        assert_rejected(&app, body, StatusCode::BAD_REQUEST, "title_empty").await;

        register_ok(&app, "다시 등록").await;
    }

    async fn get_video(app: &TestApp, path: &str, range: Option<&str>) -> Response {
        let mut request = Request::get(path);
        if let Some(range) = range {
            request = request.header("range", range);
        }
        app.router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    #[sqlx::test]
    async fn Range_없이_영상을_요청하면_200과_전체_파일이_온다(pool: PgPool) {
        let app = app(pool);
        let id = register_ok(&app, "재생").await;

        let response = get_video(&app, &format!("/api/broadcasts/{id}/video"), None).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "video/mp4");
        assert_eq!(response.headers()["accept-ranges"], "bytes");
        assert_eq!(body_bytes(response).await, tiny_mp4());
    }

    #[sqlx::test]
    async fn Range_요청에는_206과_정확한_바이트_범위가_온다(pool: PgPool) {
        let app = app(pool);
        let id = register_ok(&app, "건너뛰기").await;
        let total = tiny_mp4().len();

        let response = get_video(
            &app,
            &format!("/api/broadcasts/{id}/video"),
            Some("bytes=4-11"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers()["content-range"],
            format!("bytes 4-11/{total}")
        );
        assert_eq!(body_bytes(response).await, b"ftypisom");
    }

    #[sqlx::test]
    async fn 없는_번호나_번호가_아닌_값으로_영상을_요청하면_404다(
        pool: PgPool,
    ) {
        let app = app(pool);
        register_ok(&app, "하나뿐").await;

        for segment in ["999", "0", "01", "abc", "99999999999999999999"] {
            let response = get_video(&app, &format!("/api/broadcasts/{segment}/video"), None).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{segment}");
        }
    }

    #[sqlx::test]
    async fn DB에_새로_연결해도_방송과_영상이_그대로_조회된다(
        pool_options: sqlx::postgres::PgPoolOptions,
        options: sqlx::postgres::PgConnectOptions,
    ) {
        let app = app(pool_options.connect_with(options.clone()).await.unwrap());
        let id = register_ok(&app, "재시작 뒤에도").await;

        // 서버를 다시 켠 것처럼 같은 DB에 새 연결 풀을 열고, 같은 보관 폴더로 라우터를 다시 만든다.
        app.pool.close().await;
        let reopened = PgPool::connect_with(options).await.unwrap();
        let restarted = AppState {
            pool: reopened.clone(),
            videos: VideoStore::new(app.data_dir.path()),
            max_upload_bytes: 1024,
        };
        let router = api(restarted);

        let found = crate::db::find_broadcast(&reopened, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.title, "재시작 뒤에도");
        let request = Request::get(format!("/api/broadcasts/{id}/video"))
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response).await, tiny_mp4());
    }

    #[sqlx::test]
    async fn 같은_제목도_등록할_수_있다(pool: PgPool) {
        let app = app(pool);

        let a = register_ok(&app, "같은 제목").await;
        let b = register_ok(&app, "같은 제목").await;

        assert_ne!(a, b);
    }

    // ── 로그인과 소유자(3단계) ──

    #[sqlx::test]
    async fn 로그인하지_않고_등록하면_401이고_방송도_파일도_남지_않는다(
        pool: PgPool,
    ) {
        let app = app(pool);
        let body = multipart_body(Some("익명 등록"), Some(("clip.mp4", &tiny_mp4())));

        let (status, json) = send(
            &app.router,
            register_request_as(&Persona::anonymous(), body),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED, "응답: {json}");
        assert_eq!(json["code"], "login_required");
        assert_nothing_left(&app).await;
    }

    #[sqlx::test]
    async fn 로그인한_사용자가_등록한_방송의_소유자는_그_사용자다(
        pool: PgPool,
    ) {
        let app = app(pool.clone());
        let owner = Persona::user_a(&pool).await;
        let body = multipart_body(Some("내 방송"), Some(("clip.mp4", &tiny_mp4())));

        let (status, json) = send(&app.router, register_request_as(&owner, body)).await;

        assert_eq!(status, StatusCode::CREATED, "응답: {json}");
        let id = json["id"].as_i64().unwrap();
        let list = crate::db::list_broadcasts(&app.pool).await.unwrap();
        assert_eq!(list[0].owner.as_deref(), Some("user_a"));
        let found = crate::db::find_broadcast(&app.pool, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.owner.as_deref(), Some("user_a"));
    }
}
