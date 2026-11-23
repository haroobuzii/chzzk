//! 앱의 서버 연결 모듈 테스트. 배포 서버 대신 임의 포트에 띄운 작은 가짜 HTTP 서버를 부른다.
//! 가짜는 스펙에 적힌 공개 API의 모양(`/api/broadcasts/list`, `/api/broadcasts/info?id=`) 그대로 JSON을 돌려준다.

use axum::{
    extract::RawQuery,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use chrono::DateTime;

use super::deploy_server::DeployServer;
use crate::{BroadcastKind, LiveStatus};

/// 목록 API가 돌려주는 본문. 최신순으로 라이브 방송(방송 중) 하나와 익명 녹화 방송 하나다.
const LIST_JSON: &str = r#"[
  {"id": 7, "title": "저녁 라이브", "created_at": "2026-11-23T10:00:00Z", "kind": "Live",
   "status": "Live", "started_at": "2026-11-23T10:00:05Z", "ended_at": null, "owner": "alice"},
  {"id": 3, "title": "첫 녹화", "created_at": "2026-11-20T01:02:03Z", "kind": "Recorded",
   "status": null, "started_at": null, "ended_at": null, "owner": null}
]"#;

/// 단건 API가 3번에 돌려주는 본문.
const INFO_3_JSON: &str = r#"{"id": 3, "title": "첫 녹화", "created_at": "2026-11-20T01:02:03Z",
  "kind": "Recorded", "status": null, "started_at": null, "ended_at": null, "owner": null}"#;

fn json(body: &'static str) -> Response {
    ([("content-type", "application/json")], body).into_response()
}

/// 정상 응답하는 가짜 배포 서버를 띄우고 그 주소를 돌려준다.
async fn start_fake() -> String {
    let router = Router::new()
        .route("/api/broadcasts/list", get(|| async { json(LIST_JSON) }))
        .route(
            "/api/broadcasts/info",
            get(|RawQuery(query): RawQuery| async move {
                match query.as_deref() {
                    Some("id=3") => json(INFO_3_JSON),
                    _ => json("null"),
                }
            }),
        );
    serve(router).await
}

/// 앱이 사용자에게 보여 줄 연결 실패 문구(스펙: "서버에 연결할 수 없어요").
const CONNECTION_FAILED: &str =
    "서버에 연결할 수 없어요. 인터넷 연결을 확인한 뒤 다시 시도해 주세요.";

/// 모든 요청에 500을 돌려주는 가짜 배포 서버(DB가 죽은 서버처럼).
/// 본문은 일부러 읽을 수 있는 JSON(`null`)으로 둔다. 상태 코드를 보지 않고 본문만 읽으면 실패를 놓친다.
async fn start_broken_fake() -> String {
    let router = Router::new()
        .fallback(|| async { (StatusCode::INTERNAL_SERVER_ERROR, json("null")).into_response() });
    serve(router).await
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    url
}

#[tokio::test]
async fn 목록을_읽는다() {
    let server = DeployServer::new(&start_fake().await);

    let list = server.broadcasts().await.unwrap();

    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, 7);
    assert_eq!(list[0].title, "저녁 라이브");
    assert_eq!(list[0].kind, BroadcastKind::Live);
    assert_eq!(list[0].status, Some(LiveStatus::Live));
    assert_eq!(list[0].owner.as_deref(), Some("alice"));
    assert_eq!(
        list[0].started_at,
        Some(
            DateTime::parse_from_rfc3339("2026-11-23T10:00:05Z")
                .unwrap()
                .into()
        )
    );
    assert_eq!(list[1].id, 3);
    assert_eq!(list[1].kind, BroadcastKind::Recorded);
    assert_eq!(list[1].status, None);
    assert_eq!(list[1].owner, None);
}

#[tokio::test]
async fn 방송_하나를_읽고_없으면_None이다() {
    let server = DeployServer::new(&start_fake().await);

    let found = server.broadcast(3).await.unwrap().unwrap();
    assert_eq!(found.id, 3);
    assert_eq!(found.title, "첫 녹화");

    assert_eq!(server.broadcast(99).await.unwrap(), None);
}

#[tokio::test]
async fn 서버가_500이면_연결_실패_문구다() {
    let server = DeployServer::new(&start_broken_fake().await);

    let error = server.broadcasts().await.unwrap_err();
    assert_eq!(error.to_string(), CONNECTION_FAILED);
    let error = server.broadcast(3).await.unwrap_err();
    assert_eq!(error.to_string(), CONNECTION_FAILED);
}

#[tokio::test]
async fn 서버가_꺼져_있으면_연결_실패_문구다() {
    // 포트를 잡았다가 바로 놓아, 아무도 듣지 않는 주소를 만든다.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let server = DeployServer::new(&url);

    let error = server.broadcasts().await.unwrap_err();
    assert_eq!(error.to_string(), CONNECTION_FAILED);
}

#[test]
fn 영상_시청_웹_주소는_서버_주소_기준의_절대_주소다() {
    // 끝에 `/`를 붙여 설정해도 주소가 겹치지 않는다.
    let server = DeployServer::new("https://chzzk.haroobuzii.com/");

    assert_eq!(
        server.video_url(3),
        "https://chzzk.haroobuzii.com/api/broadcasts/3/video"
    );
    assert_eq!(
        server.whep_url(7),
        "https://chzzk.haroobuzii.com/api/live/7/whep"
    );
    assert_eq!(
        server.web_page_url(7),
        "https://chzzk.haroobuzii.com/broadcasts/7"
    );
}
