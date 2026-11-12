//! 서버 함수로 만들지 않고 직접 덧붙이는 HTTP 라우트: 방송 등록 API와 영상 전송.
//!
//! 등록은 큰 영상 파일을 받으므로 본문을 메모리에 모으는 서버 함수 대신
//! axum 라우트에서 multipart를 조각조각 읽어 임시 파일에 바로 쓴다.

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
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

use crate::{config::Config, number::parse_number, videos::VideoStore};

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

/// multipart 경계선과 제목 칸에 쓰일 여유분. 영상 크기는 따로 센다.
const MULTIPART_OVERHEAD: u64 = 1024 * 1024;

/// 등록 API와 영상 전송 라우터.
pub fn api(state: AppState) -> Router {
    let body_limit = state.max_upload_bytes.saturating_add(MULTIPART_OVERHEAD);
    Router::new()
        .route(
            "/api/broadcasts",
            // 이 라우트만 기본 본문 크기 제한(2 MB)을 업로드 최대 크기에 맞춰 늘린다.
            post(register).layer(DefaultBodyLimit::max(usize::try_from(body_limit).unwrap_or(usize::MAX))),
        )
        .route("/api/broadcasts/{number}/video", get(video))
        .with_state(state)
}

/// `GET /api/broadcasts/{번호}/video`: 영상 원본을 변환 없이 보낸다(ADR 0001).
///
/// Range 요청에는 206과 요청한 부분만 보내므로, 브라우저가 끝까지 내려받지 않고도 건너뛸 수 있다.
async fn video(State(state): State<AppState>, Path(segment): Path<String>, request: Request<Body>) -> Response {
    let Some(id) = parse_number(&segment) else {
        return not_found();
    };
    let key = match crate::db::find_video_key(&state.pool, id).await {
        Ok(Some(key)) => key,
        Ok(None) => return not_found(),
        Err(e) => {
            dioxus::logger::tracing::error!("영상 위치 조회 실패: {e}");
            return failure(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "영상을 불러오지 못했어요.");
        }
    };

    // Range 처리는 검증된 구현(tower-http)에 맡긴다. 확장자가 .mp4라서 Content-Type은 video/mp4가 된다.
    let file = ServeFile::new(state.videos.path_of(&key));
    match file.oneshot(request).await {
        Ok(response) if response.status() == StatusCode::NOT_FOUND => not_found(),
        Ok(response) => response.map(Body::new),
        Err(never) => match never {},
    }
}

fn not_found() -> Response {
    failure(StatusCode::NOT_FOUND, "not_found", "방송을 찾을 수 없습니다.")
}

/// `POST /api/broadcasts`: multipart의 `title`과 `video`로 방송을 등록한다.
///
/// 성공하면 201과 `{"id": 방송 번호}`를 돌려준다.
/// 중간에 실패하거나 연결이 끊기면 임시 파일은 저절로 지워지고 방송 정보도 남지 않는다.
async fn register(State(state): State<AppState>, multipart: Multipart) -> Response {
    match try_register(&state, multipart).await {
        Ok(id) => (StatusCode::CREATED, Json(json!({ "id": id }))).into_response(),
        Err(response) => response,
    }
}

async fn try_register(state: &AppState, mut multipart: Multipart) -> Result<i64, Response> {
    let mut title = None;
    let mut upload = None;

    while let Some(field) = multipart.next_field().await.map_err(|_| broken_upload())? {
        match field.name() {
            Some("title") => title = Some(field.text().await.map_err(|_| broken_upload())?),
            Some("video") => upload = Some(receive_video(state, field).await?),
            // 모르는 칸은 읽지 않고 넘긴다.
            _ => {}
        }
    }

    let title = title.unwrap_or_default().trim().to_string();
    let Some(upload) = upload else {
        return Err(failure(StatusCode::BAD_REQUEST, "missing_video", "영상 파일을 골라 주세요."));
    };

    save(state, &title, upload).await.map_err(|e| {
        dioxus::logger::tracing::error!("방송 등록 실패: {e}");
        failure(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "등록하지 못했어요. 잠시 뒤 다시 시도해 주세요.")
    })
}

/// 영상 칸을 조각조각 읽어 임시 파일에 바로 쓴다. 파일 전체를 메모리에 모으지 않는다.
async fn receive_video(
    state: &AppState,
    mut field: axum::extract::multipart::Field<'_>,
) -> Result<NamedTempFile, Response> {
    let temp = state.videos.create_temp().map_err(server_error)?;
    let mut file = tokio::fs::File::from_std(temp.as_file().try_clone().map_err(server_error)?);

    while let Some(chunk) = field.chunk().await.map_err(|_| broken_upload())? {
        file.write_all(&chunk).await.map_err(server_error)?;
    }
    file.flush().await.map_err(server_error)?;
    Ok(temp)
}

/// 방송 정보와 영상을 함께 저장한다. 둘 중 하나만 남는 일이 없게 한다.
///
/// 1. 트랜잭션 안에서 방송 정보를 넣어 번호를 받고 영상 키를 정한다.
/// 2. 임시 파일을 영상 키 위치로 옮긴다. 실패하면 트랜잭션이 되돌려진다.
/// 3. 커밋한다. 실패하면 옮긴 파일을 지운다.
async fn save(state: &AppState, title: &str, upload: NamedTempFile) -> anyhow::Result<i64> {
    let mut tx = state.pool.begin().await?;
    let id = crate::db::insert_broadcast(&mut tx, title).await?;
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
    failure(StatusCode::BAD_REQUEST, "broken_upload", "업로드가 중간에 끊겼어요. 다시 시도해 주세요.")
}

fn server_error(e: std::io::Error) -> Response {
    dioxus::logger::tracing::error!("영상 저장 실패: {e}");
    failure(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "영상을 저장하지 못했어요. 잠시 뒤 다시 시도해 주세요.")
}

/// 실패 응답: 상태 코드와 원인 코드, 한국어 메시지.
fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "code": code, "message": message }))).into_response()
}

/// 이 테스트들은 실제 PostgreSQL이 필요하다(`#[sqlx::test]`가 테스트마다 임시 DB를 만든다).
/// 서버를 띄우지 않고 라우터에 요청을 직접 넣어 응답과 그 뒤의 조회 결과를 본다.
#[cfg(test)]
#[allow(non_snake_case)]
pub(crate) mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tempfile::TempDir;

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
        TestApp { router: api(state), pool, data_dir }
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

    pub fn register_request(body: Vec<u8>) -> Request<Body> {
        Request::post("/api/broadcasts")
            .header("content-type", format!("multipart/form-data; boundary={BOUNDARY}"))
            .body(Body::from(body))
            .unwrap()
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
    async fn 제목과_MP4로_등록하면_201과_번호가_오고_목록과_조회에_나타난다(pool: PgPool) {
        let app = app(pool);

        let id = register_ok(&app, "첫 방송").await;

        let list = crate::db::list_broadcasts(&app.pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].title, "첫 방송");

        let found = crate::db::find_broadcast(&app.pool, id).await.unwrap().unwrap();
        assert_eq!(found.title, "첫 방송");
    }

    #[sqlx::test]
    async fn 영상은_보관_폴더에_저장되고_DB에는_상대_위치가_남는다(pool: PgPool) {
        let app = app(pool);

        let id = register_ok(&app, "영상 확인").await;

        let key = crate::db::find_video_key(&app.pool, id).await.unwrap().unwrap();
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

        let ids: Vec<i64> = crate::db::list_broadcasts(&app.pool).await.unwrap().iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![third, second, first]);
    }

    async fn get_video(app: &TestApp, path: &str, range: Option<&str>) -> Response {
        let mut request = Request::get(path);
        if let Some(range) = range {
            request = request.header("range", range);
        }
        app.router.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap()
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec()
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

        let response = get_video(&app, &format!("/api/broadcasts/{id}/video"), Some("bytes=4-11")).await;

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-range"], format!("bytes 4-11/{total}"));
        assert_eq!(body_bytes(response).await, b"ftypisom");
    }

    #[sqlx::test]
    async fn 없는_번호나_번호가_아닌_값으로_영상을_요청하면_404다(pool: PgPool) {
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

        let found = crate::db::find_broadcast(&reopened, id).await.unwrap().unwrap();
        assert_eq!(found.title, "재시작 뒤에도");
        let request = Request::get(format!("/api/broadcasts/{id}/video")).body(Body::empty()).unwrap();
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
}
