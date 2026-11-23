//! 다운로드 페이지 `GET /download`. 방문자가 앱을 OS별로 내려받는다.
//!
//! 서버가 HTML을 통째로 그려 보낸다. 링크만 있는 정적인 안내라 Dioxus 화면이 필요 없고,
//! TestFlight 링크처럼 서버 설정에서 오는 값을 그대로 넣을 수 있다.

use axum::{extract::State, response::Html, routing::get, Router};

#[derive(Clone)]
pub struct DownloadState {
    /// iOS 앱 TestFlight 공개 링크. 없으면 "준비 중"으로 보여 준다.
    pub testflight_url: Option<String>,
}

/// 다운로드 페이지 라우터.
pub fn api(state: DownloadState) -> Router {
    Router::new()
        .route("/download", get(download_page))
        .with_state(state)
}

/// 08의 GitHub Release 고정 태그 주소. main에 올릴 때마다 같은 이름의 파일이 최신으로 바뀐다.
const RELEASE_BASE: &str = "https://github.com/haroobuzii/chzzk/releases/download/app-latest";

async fn download_page(State(state): State<DownloadState>) -> Html<String> {
    let ios = match &state.testflight_url {
        Some(url) => format!(
            r#"<a class="button" href="{}">TestFlight에서 설치</a>"#,
            escape_html(url)
        ),
        None => r#"<span class="download-pending">준비 중</span>"#.to_string(),
    };
    Html(
        PAGE.replace("{css}", &crate::MAIN_CSS.to_string())
            .replace("{release}", RELEASE_BASE)
            .replace("{ios}", &ios),
    )
}

/// 설정 값을 HTML 속성에 넣기 전에 특수 문자를 바꾼다.
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// 페이지 본문. `{css}`, `{release}`, `{ios}`를 채워 보낸다.
const PAGE: &str = r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>앱 다운로드 - 녹화방송</title>
<link rel="stylesheet" href="{css}">
</head>
<body>
<header class="site-header">
  <a class="brand" href="/">녹화방송</a>
</header>
<main class="download-page">
  <h1>앱 다운로드</h1>
  <p class="download-intro">앱은 방송 목록을 보고 라이브와 녹화를 시청하는 <strong>보기 전용</strong>입니다.
  로그인, 댓글, 후원은 방송 화면의 "웹에서 열기"를 눌러 웹에서 합니다.</p>

  <section class="download-os">
    <h2>Windows</h2>
    <p class="download-arch">Windows x64 · Windows 10 이상</p>
    <a class="button" href="{release}/chzzk-windows-x64.msi">Windows용 내려받기 (.msi)</a>
    <ol class="download-steps">
      <li>내려받은 <code>chzzk-windows-x64.msi</code>를 엽니다.</li>
      <li>"Windows의 PC 보호" 창이 뜨면 <b>추가 정보</b>를 누른 뒤 <b>실행</b>을 누릅니다.</li>
      <li>설치 화면에는 이름이 <b>Chzzk</b>로 나옵니다. 설치가 끝나면 시작 메뉴에서 실행하며, 창 제목은 <b>녹화방송</b>입니다.</li>
    </ol>
  </section>

  <section class="download-os">
    <h2>macOS</h2>
    <p class="download-arch">macOS 11 이상 · 내 맥의 칩에 맞는 파일을 고릅니다(Apple 메뉴 → 이 Mac에 관하여 → 칩).</p>
    <div class="download-buttons">
      <a class="button" href="{release}/chzzk-macos-arm64.dmg">Apple Silicon(M1 이후)용 내려받기 (.dmg)</a>
      <a class="button" href="{release}/chzzk-macos-x64.dmg">Intel용 내려받기 (.dmg)</a>
    </div>
    <ol class="download-steps">
      <li>내려받은 <code>.dmg</code>를 열고, <b>녹화방송</b>을 <b>응용 프로그램</b> 폴더로 끌어 놓습니다.</li>
      <li>응용 프로그램 폴더에서 녹화방송을 엽니다.</li>
      <li>"확인되지 않은 개발자" 경고로 열리지 않으면, 앱을 오른쪽 클릭해 <b>열기</b>를 누릅니다.
        그래도 막히면 <b>시스템 설정 → 개인정보 보호 및 보안</b>에서 <b>그래도 열기</b>를 누릅니다.
        아직 Apple 서명이 없는 빌드일 때 한 번만 필요합니다.</li>
    </ol>
  </section>

  <section class="download-os">
    <h2>iOS (iPhone)</h2>
    <p class="download-arch">TestFlight 공개 베타 · App Store의 TestFlight 앱이 필요합니다.</p>
    {ios}
  </section>
</main>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::{api, DownloadState};

    async fn get_page(testflight_url: Option<&str>) -> (StatusCode, String, String) {
        let state = DownloadState {
            testflight_url: testflight_url.map(String::from),
        };
        let request = Request::get("/download").body(Body::empty()).unwrap();
        let response = api(state).oneshot(request).await.unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            content_type,
            String::from_utf8(body.to_vec()).unwrap(),
        )
    }

    #[tokio::test]
    async fn OS별_고정_Release_링크와_아키텍처_문구가_있다() {
        let (status, content_type, html) = get_page(None).await;

        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("text/html"), "{content_type}");
        for link in [
            "https://github.com/haroobuzii/chzzk/releases/download/app-latest/chzzk-windows-x64.msi",
            "https://github.com/haroobuzii/chzzk/releases/download/app-latest/chzzk-macos-arm64.dmg",
            "https://github.com/haroobuzii/chzzk/releases/download/app-latest/chzzk-macos-x64.dmg",
        ] {
            assert!(html.contains(&format!("href=\"{link}\"")), "링크 없음: {link}");
        }
        for words in ["Windows x64", "Apple Silicon", "Intel"] {
            assert!(html.contains(words), "문구 없음: {words}");
        }
    }

    #[tokio::test]
    async fn 설치_방법과_앱은_보기_전용이라는_안내가_있다() {
        let (_, _, html) = get_page(None).await;

        for words in [
            "추가 정보",
            "실행",
            "응용 프로그램",
            "그래도 열기",
            "Chzzk",
            "웹에서",
        ] {
            assert!(html.contains(words), "문구 없음: {words}");
        }
    }

    #[tokio::test]
    async fn TestFlight_링크가_없으면_준비_중이다() {
        let (_, _, html) = get_page(None).await;

        assert!(html.contains("준비 중"));
        assert!(!html.contains("testflight.apple.com"));
    }

    #[tokio::test]
    async fn TestFlight_링크가_있으면_그_링크를_건다() {
        let (_, _, html) = get_page(Some("https://testflight.apple.com/join/AbCd1234")).await;

        assert!(html.contains("href=\"https://testflight.apple.com/join/AbCd1234\""));
        assert!(!html.contains("준비 중"));
    }

    #[tokio::test]
    async fn TestFlight_링크의_특수_문자는_HTML로_새지_않는다() {
        let (_, _, html) = get_page(Some("https://example.com/\"><script>x</script>")).await;

        assert!(!html.contains("<script>x"));
    }
}
