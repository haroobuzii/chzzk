# 배포·데스크톱 조사 노트 (2026-11-22 기준)

과제 ⑥(OCI 배포, 도메인·HTTPS, Windows/macOS 클라이언트) 설계 전에 1차 자료에서 확인한 사실이다.
**[추론]** 표시는 문서에 직접 적혀 있지 않고 내가 판단한 내용이다. 가격은 바뀌므로 구매 직전에 다시 확인한다.

## 1. OCI Always Free

출처: [Always Free Resources](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)

- **A1(Arm)**: "first **1,500 OCPU hours and 9,000 GB hours per month** ... VM.Standard.A1.Flex ... equivalent to **2 OCPUs and 12 GB** of memory."
  - 예전 값(3,000 OCPU시간·18,000 GB시간 = 4 OCPU·24 GB)이 절반으로 줄었다. 블로그와 커뮤니티에 4/24로 적힌 글은 옛 정보다. 과제 문서의 "2 OCPU·12GB"와 같다.
  - 인스턴스는 A1을 "one or two ... 2 OCPUs total"까지 만들 수 있다. 부트 볼륨은 인스턴스마다 최소 47 GB다.
- **E2.1.Micro(AMD)**: 최대 2대. 1/8 OCPU(필요하면 CPU를 더 씀), 메모리 1 GB, 인터넷 대역폭 최대 50 Mbps.
  - [추론] Rust 빌드와 Postgres, MediaMTX를 함께 돌리기에는 1 GB가 부족하다. A1을 얻지 못했을 때만 쓰는 대안이다.
- **블록 스토리지**: 부트와 블록 볼륨을 합쳐 **200 GB**(홈 리전). 볼륨 백업 5개.
- **나가는 트래픽**: 월 **10 TB** 무료.
- **홈 리전에서만**: Always Free 리소스는 "free of charge in the home region". 가입할 때 "choose the home region carefully. You can provision Always Free ... compute instances only in the home region." ([Free Tier](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier.htm))
- **홈 리전은 바꿀 수 없다**: "You can't make changes after your tenancy is provisioned." ([Managing Regions](https://docs.oracle.com/en-us/iaas/Content/Identity/regions/managingregions.htm))
- **한국 리전** ([Regions](https://docs.oracle.com/en-us/iaas/Content/General/Concepts/regions.htm)): South Korea Central (Seoul) `ap-seoul-1`(ICN), South Korea North (Chuncheon) `ap-chuncheon-1`(YNY). 둘 다 가용 도메인(AD)이 1개다.
- **재고 부족**: "If you receive an **'out of host capacity'** error ... temporary lack of Always Free shapes in your home region. Try ... a different availability domain, or wait a while ... You can also choose to upgrade your account to Pay as You Go ... Oracle doesn't charge for Always Free resources after you upgrade."
  - [추론] 한국 리전은 AD가 하나라서 다른 AD로 옮길 수 없다. 남은 길은 기다렸다가 다시 시도하거나 PAYG로 업그레이드하는 것이다. 업그레이드하면 한도 초과분은 과금되므로 compartment quota로 막는다.
- **유휴 인스턴스 회수**: 7일 동안 다음이 모두 참이면 Oracle이 회수할 수 있다.
  - CPU 95번째 백분위 사용률 < 20%
  - 네트워크 사용률 < 20%
  - 메모리 사용률 < 20%(A1에만 적용)
  - [추론] 트래픽이 적은 과제 서버는 이 조건에 걸릴 수 있다. 다만 Postgres와 앱이 메모리를 20% 넘게 쓰면 A1은 걸리지 않는다. 12 GB의 20%는 약 2.4 GB다.
- **가입**: "most users need a mobile phone number and a **credit card** ... will not be charged unless you upgrade." ([Free Tier](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier.htm))
- **Ubuntu ARM64 이미지**: OCI 콘솔에서 Canonical이 만든 `Canonical-Ubuntu-<버전>-aarch64-<날짜>` 이미지를 고른다. ([Canonical: Find Ubuntu images on OCI](https://documentation.ubuntu.com/oracle/oracle-how-to/find-ubuntu-images), [Canonical offerings](https://documentation.ubuntu.com/oracle/oracle-explanation/canonical-offerings)) 24.04 LTS도 지원 목록에 있다.
- **우리가 쓰는 Docker 이미지는 모두 arm64를 제공한다**(Docker Hub 태그 API로 확인).
  - `bluenviron/mediamtx:1.21.3`(arm64)
  - `postgres:17`, `caddy:2`, `rust:1-trixie`, `debian:trixie-slim`(arm64/v8)

## 2. OCI 네트워크: 포트 열기

방화벽이 두 겹이라 **둘 다** 열어야 한다.

1. **VCN 보안 목록(또는 NSG)** ([Security Lists](https://docs.oracle.com/en-us/iaas/Content/Network/Concepts/securitylists.htm))
   - 기본 보안 목록은 SSH(TCP 22)만 연다.
   - 상태 저장(stateful) ingress 규칙을 추가한다. 출처는 `0.0.0.0/0`이다.
     - TCP 80: ACME HTTP 챌린지와 리다이렉트
     - TCP 443: HTTPS
     - UDP 443: 선택, Caddy HTTP/3
     - **UDP 8189**: MediaMTX WebRTC
2. **Ubuntu 이미지 안의 iptables**
   - Oracle 개발자 블로그: OCI의 Ubuntu 이미지는 UFW를 쓰지 않는다. `/etc/iptables/rules.v4`를 직접 고치고 `iptables-restore`로 다시 읽는다. ([Oracle blog](https://blogs.oracle.com/developers/post/enabling-network-traffic-to-ubuntu-images-in-oracle-cloud-infrastructure), 자동 fetch가 403이라 검색 요약으로 확인했다.)
   - Oracle devs Medium 글: "Don't use ufw on OCI". ([Firewalls with Ubuntu on OCI](https://medium.com/oracledevs/firewalls-with-ubuntu-on-oci-274c6a1ea670))
   - [추론, 흔히 쓰는 방법] `rules.v4`의 맨 끝 `-A INPUT -j REJECT --reject-with icmp-host-prohibited` **앞에** 규칙을 넣는다.
     - `-A INPUT -p tcp -m state --state NEW --dport 80 -j ACCEPT`(443도 같은 방식)
     - `-A INPUT -p udp --dport 8189 -j ACCEPT`
     - 넣은 뒤 `sudo iptables-restore < /etc/iptables/rules.v4`, 또는 `sudo netfilter-persistent save`로 재부팅 뒤에도 유지되게 한다.
   - [추론] Docker가 publish한 포트는 DOCKER 체인(FORWARD)을 거치므로 INPUT의 REJECT에 막히지 않을 수 있다. 그래도 직접 실행하는 서비스와 문서화를 위해 열어 두는 편이 안전하다. 배포한 뒤 바깥에서 `curl`과 WebRTC로 실제 확인한다.
3. **MediaMTX의 공인 IP**
   - [추론] OCI 공인 IP는 NAT로 붙어서 VM 안에서는 사설 IP만 보인다. 그래서 `MTX_WEBRTCADDITIONALHOSTS`에 VM의 **공인 IP**(또는 도메인)를 넣는다. 지금 `LAN_IP` 자리에 들어가는 값이다.

## 3. Caddy(자동 HTTPS 리버스 프록시)

- **자동 HTTPS 조건** ([Automatic HTTPS](https://caddyserver.com/docs/automatic-https))
  - 도메인의 A/AAAA 레코드가 서버를 가리킨다.
  - 외부에서 80/443에 닿는다.
  - 데이터 디렉터리가 **쓰기 가능하고 영구적**이다.
  - 설정에 도메인 이름이 있다.
  - HTTP 챌린지는 80, TLS-ALPN 챌린지는 443을 쓴다.
- **최소 Caddyfile**([reverse_proxy](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy)):
  ```
  example.com {
      request_body { max_size 2GiB }   # v2.10+ (Experimental)
      reverse_proxy app:8080
  }
  ```
- **본문 크기**: `request_body max_size`는 Content-Length가 크면 바로 413으로 거절한다. 설정하지 않으면 제한이 없다. [추론] 문서에 기본값이 적혀 있지 않고, 예시에서만 제한을 건다. ([request_body](https://caddyserver.com/docs/caddyfile/directives/request_body))
- **타임아웃**([global options](https://caddyserver.com/docs/caddyfile/options)): 2 GiB 업로드에서 중요한 값이다.
  - `read_body`: 기본 **없음**
  - `read_body_idle`: 기본 **1m**. 읽을 때마다 초기화되므로 계속 보내는 업로드는 괜찮다.
  - `read_header` 1m, `write` 없음, `write_idle` 1m, `idle` 5m
  - reverse_proxy `transport http`의 `response_header_timeout`과 `read_timeout`은 기본 **없음**이다.
  - [추론] 업로드가 끝난 뒤 앱이 응답하기 전에 1분 넘게 처리(검사·이동)하면 `write_idle`과는 무관하다. 그래도 클라이언트 쪽 대기 시간을 따져 봐야 한다.
- 최신 릴리스는 v2.11.9다(GitHub API).
- **compose 구성**: `caddy:2` 이미지를 쓰고 `80:80`, `443:443`, `443:443/udp`를 publish한다. `/data`(인증서)와 `/config`는 named volume으로 둔다. [추론] 같은 compose 네트워크 안에서 앱 서비스 이름으로 proxy하고, 앱은 밖으로 publish하지 않는다.

## 4. ARM64 리눅스용 서버 빌드

- 지금 쓰는 패턴은 short-url `Dockerfile`이다.
  - `rust:1-trixie` → cargo-chef → `cargo binstall dioxus-cli@0.7.10` → `dx bundle --web --release`
  - 산출물: `target/dx/<name>/release/web/`(실행 파일 `server`와 `public/`)
  - 런타임 이미지: `debian:trixie-slim`, `IP=0.0.0.0`, `PORT=8080`
  - 근거: [Dioxus 0.7 deploy](https://dioxuslabs.com/learn/0.7/tutorial/deploy)
- **dx 미리 빌드된 바이너리**: v0.7.10 릴리스에 `dx-aarch64-unknown-linux-gnu.tar.gz`가 있다. x86_64 Linux와 macOS 두 아키텍처, Windows x64·arm64 바이너리도 있다. ([release v0.7.10](https://github.com/DioxusLabs/dioxus/releases/tag/v0.7.10)) 그래서 arm64 컨테이너에서도 `cargo binstall`이 컴파일 없이 받는다. [추론] binstall이 릴리스 자산을 찾는다.
- **빌드 위치 선택지**
  1. **VM에서 `docker compose build`**: 가장 단순하다. 2 OCPU라 첫 빌드는 오래 걸린다. [추론] 수십 분이고, 메모리 12 GB면 충분하다.
  2. **GitHub Actions `ubuntu-24.04-arm`**: 공개 저장소의 표준 러너는 "free and unlimited"이다. 공개 저장소는 4 vCPU·16 GB arm64, 비공개 저장소는 2 vCPU·8 GB다. ([GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)) 이미지를 GHCR에 올리고 VM은 pull만 한다.
  3. **교차 컴파일**(x86 → aarch64): wasm, 서버, sqlx 매크로, 링커까지 맞춰야 해서 복잡하다. [추론] 이득이 적다.

## 5. Dioxus 0.7 데스크톱

- **패키징** ([Bundle 튜토리얼](https://dioxuslabs.com/learn/0.7/tutorial/bundle))
  - `dx bundle --desktop --package-types <...>`로 만든다.
    - macOS: `.app`, `.dmg`
    - Windows: `.msi`, `.exe`(NSIS)
  - "`dx bundle` only builds desktop apps for the **native platform and architecture**." 그래서 GitHub Actions 매트릭스를 권장한다.
  - 출력 예: `target/dx/<app>/bundle/macos/bundle/dmg/<App>_0.1.0_aarch64.dmg`
  - `--json-output`의 마지막 줄에서 `BundleOutput.bundles`를 뽑을 수 있다.
  - 설정은 `Dioxus.toml`의 `[bundle]` 표에 둔다. `identifier`, `publisher`, `icon` 등이다.
- **필요한 도구** (dx v0.7.10 소스 `packages/cli/src/bundler/`)
  - Windows: **WiX 3.14**(msi)와 **NSIS 3.11**(exe)을 dx가 **자동으로 내려받는다**. `tools.rs`에 해시 검증이 있다.
  - macOS: `hdiutil`(dmg)과 `codesign`을 쓴다. 서명은 `APPLE_CERTIFICATE` 환경변수나 `signing_identity`가 있을 때만 한다.
  - [추론] Windows 러너에는 MSVC가, macOS 러너에는 Xcode CLT가 이미 깔려 있다.
- **같은 크레이트로 빌드할 때**
  - `desktop = ["dioxus/desktop"]` feature를 추가해야 한다. [추론]
  - dx `BuildArgs` 주석: `fullstack` feature가 켜져 있으면 "the server will be built as well". 우리 `dioxus` 의존성은 `fullstack`을 항상 켜므로, 데스크톱 번들에서도 서버가 함께 빌드될 수 있다. [추론] `@client`나 feature 분리로 피할 수 있는지 시도해 봐야 한다.
- **원격 서버 주소**
  - 문서는 `server_fn::client::set_server_url("https://...")`를 `#[cfg(not(feature="server"))]`에서 `launch` 전에 부르라고 한다. ([deploy: Fullstack Desktop](https://dioxuslabs.com/learn/0.7/tutorial/deploy))
  - 0.7.10 실제 API는 `dioxus::fullstack::set_server_url(&'static str)`이다. 내부가 `OnceLock`이고 `.unwrap()`이라 **두 번 부르면 panic**한다.
  - `launch`는 desktop 빌드에서 주소가 비어 있으면 `http://127.0.0.1:8080`(DIOXUS_DEVSERVER_IP/PORT)으로 채운다. 그래서 **반드시 launch 전에** 설정한다. (`dioxus-0.7.10/src/launch.rs`, `dioxus-fullstack-0.7.10/src/client.rs`)
  - 해시 대신 고정 경로를 쓰려면 `#[server(endpoint = "...")]`를 권장한다.
  - 네이티브 클라이언트는 reqwest를 쓰고 cookie jar를 켠다(`client.rs`).
- **우리 코드의 함정** [추론]
  - 화면의 `call_api`는 `document::eval`로 **웹뷰 안 JS `fetch`**를 부르고 상대 경로 `/api/...`를 쓴다.
  - 데스크톱 웹뷰의 origin은 Windows가 `http://dioxus.index.html/`, macOS가 `dioxus://index.html/`이다(`packages/desktop/src/protocol.rs`).
  - 그래서 상대 경로 fetch는 우리 서버에 닿지 않는다. 절대 URL로 바꾸면 **CORS**(위 origin 허용)가 필요하고, 교차 사이트 쿠키도 문제다.
  - 데스크톱 클라이언트는 "목록 조회 + 재생"만 하면 된다.
    - 목록: Rust 쪽 HTTP(reqwest나 서버 함수)로 받으면 CORS가 필요 없다.
    - 녹화 재생: `<video src="https://도메인/...">`는 CORS 없이 재생된다.
    - 라이브: WHEP `fetch`(SDP POST)는 웹뷰 JS에서 부르므로 서버의 WHEP 프록시 경로에 CORS를 허용해야 한다.
- **asset! CSS**: `asset!("/assets/x.css")`는 데스크톱을 포함한 모든 플랫폼에 번들된다. Windows와 macOS에서는 실행 파일 옆 파일시스템에 놓인다. ([Assets](https://dioxuslabs.com/learn/0.7/essentials/ui/assets))
- **WebView2(Windows)**
  - "Most features work the same in WebView2 and Microsoft Edge." 쓸 수 없는 기능 목록은 Push Notifications, Web Payment, Periodic Background Sync이고 **WebRTC는 목록에 없다**. ([WebView2 browser features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features))
  - Edge는 H.264를 지원한다. ([Edge video playback](https://learn.microsoft.com/en-us/troubleshoot/microsoft-edge/development/video-playback-issues))
  - [추론] WebView2에서 MP4(H.264/AAC) Range 재생과 RTCPeerConnection은 된다. 다만 실제 기기에서 확인해야 한다.
- **WKWebView(macOS)**: **불확실, 가장 큰 위험**
  - WebKit(2017): "RTCPeerConnection and RTCDataChannel are available in any web view"이고, 카메라와 마이크만 Safari로 제한된다. ([WebKit blog](https://webkit.org/blog/7763/a-closer-look-into-webrtc))
  - 반대로 Apple 포럼에는 "macOS WKWebView에서 RTCPeerConnection undefined"라는 보고가 있다. ([thread 695871](https://developer.apple.com/forums/thread/695871), [767818](https://developer.apple.com/forums/thread/767818))
  - 시청에는 카메라가 필요 없다. 그래도 `typeof RTCPeerConnection`을 실제 .app에서 먼저 확인한다.
  - [추론] 안 되면 라이브는 "브라우저로 열기"로 대체한다. H.264 MP4는 macOS WebKit이 기본 지원한다.

## 6. GitHub Actions로 패키지 만들기

- **러너 레이블**(공개 저장소, [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners))

  | 레이블 | 아키텍처 | 사양 |
  | --- | --- | --- |
  | `windows-latest`(=2025) | x64 | 4 CPU·16 GB |
  | `macos-latest`/`macos-15` | arm64(M1) | 3 CPU·7 GB |
  | `macos-15-intel` | x64 | 4 CPU·14 GB |
  | `ubuntu-24.04-arm` | arm64 | 4 CPU·16 GB |

  - 공개 저장소에서 표준 러너는 무료이고 사용량 제한이 없다.
- [추론] 매트릭스는 `windows-latest`(msi 또는 nsis), `macos-latest`(arm64 dmg), 필요하면 `macos-15-intel`(x64 dmg)로 짠다. `on: push: tags: ['v*']`에서 `dx bundle --desktop --release`를 돌리고 산출물을 Release에 올린다(`softprops/action-gh-release` 또는 `gh release upload`).
  - dx의 universal macOS 지원은 `Arch::Universal`처럼 코드에 흔적만 있고 문서는 확인하지 못했다. 그래서 arm64와 x64를 따로 빌드하는 편이 안전하다.
- **다운로드 URL**: `https://github.com/<owner>/<repo>/releases/latest/download/<asset-name>`. ([Linking to releases](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases)) [추론] 파일 이름에 버전이 들어가면 latest 링크가 깨지므로, 업로드할 때 고정 이름으로 바꾼다.
- **크기**: 릴리스 하나에 자산 1000개까지, 파일마다 **2 GiB 미만**, 전체 크기와 대역폭은 무제한이다. ([About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)) [추론] Dioxus 데스크톱 번들은 수 MB에서 수십 MB라 문제없다.

## 7. 서명하지 않은 앱 실행

- **macOS** ([Apple 102445](https://support.apple.com/en-us/102445))
  - 공증하지 않은 앱은 한 번 열어 보면 막힌다. 그다음 **System Settings > Privacy & Security > Open Anyway**를 누르고, 다시 뜨는 경고에서 Open을 누른다.
  - **macOS 15 Sequoia부터 Control-click(우클릭) > Open으로 우회할 수 없다.** ([Apple Developer News, Updates to runtime protection in macOS Sequoia](https://developer.apple.com/news/?id=saqachfa), 자동 fetch가 401이라 검색 요약으로 확인했다.)
  - **arm64 코드는 최소한 ad-hoc 서명이 있어야 실행된다.** 링커가 자동으로 ad-hoc 서명하고, 직접 하려면 `codesign -s -`를 쓴다. ([Big Sur Universal Apps release notes](https://developer.apple.com/documentation/macos-release-notes/macos-big-sur-11_0_1-universal-apps-release-notes))
  - [추론] 번들을 만든 뒤 Info.plist 등이 바뀌어 서명이 깨지면 "손상됨(damaged)" 경고가 난다. 그때는 CI에서 `codesign --force --deep -s - App.app`으로 다시 ad-hoc 서명한다. 사용자 쪽 우회는 `xattr -dr com.apple.quarantine /Applications/App.app`(격리 속성 제거)이다.
  - 정식 서명과 공증에는 Apple Developer Program(유료)이 필요하다. dx는 `APPLE_CERTIFICATE`로 서명을 지원한다.
- **Windows SmartScreen**: "Windows protected your PC" 창에서 **More info > Run anyway**를 누른다. Microsoft 공식 문서는 찾지 못했고 여러 보조 자료가 같은 방법을 적는다. ([UW KB](https://kb.wisc.edu/hd-cps/144696), [PCWorld](https://www.pcworld.com/article/3197443/how-to-get-past-windows-defender-smartscreen-in-windows-10.html))
- **무료 서명**: [SignPath Foundation](https://signpath.org/)이 OSI 라이선스 공개 프로젝트에 Windows 인증서를 무료로 준다.
  - 조건: 자동화된 빌드, MFA, 독점 코드 없음
  - 게시자 이름은 SignPath Foundation으로 나온다.
  - [추론] 과제 기한 안에 심사를 받기는 어렵다. 서명하지 않고 README에 우회 방법을 안내하는 편이 현실적이다.

## 8. 도메인

- **Cloudflare Registrar**
  - 원가 판매: "you pay the registry and ICANN list price with **no markup**"이고 환불은 안 된다. ([Registrar FAQ](https://developers.cloudflare.com/registrar/faq/), [cloudflare.com/domains](https://www.cloudflare.com/domains/))
  - 가격표는 대시보드에서만 볼 수 있다. 보조 자료 기준 `.com`은 등록·갱신 모두 약 **$10.46/년**이다. ([whtop](https://www.whtop.com/plans/cloudflare.com/135912)) 구매 화면에서 다시 확인한다.
  - **Cloudflare DNS를 반드시 써야 한다**: "all domains on Cloudflare Registrar use Cloudflare nameservers"이고, 다른 네임서버를 쓰려면 도메인을 옮겨 나가야 한다. (FAQ)
- **가비아**: domain.gabia.com 메인 이벤트 영역에 `COM 19,800원`, `XYZ 3,300원`, `SHOP 2,200원`이 표시된다(2026-11-22 HTML에서 확인).
  - 이벤트·첫해 가격일 가능성이 높다.
  - **갱신 가격과 VAT 포함 여부는 확인하지 못했다.** 가격표 페이지가 JS로 그려진다. 구매 화면에서 직접 확인해 README에 적는다.
  - [추론] .xyz 같은 싼 TLD는 첫해만 싸고 갱신은 훨씬 비싼 경우가 많다.
- **Cloudflare 프록시(주황 구름)는 끄고 "DNS only"로 둔다**
  - 무료 플랜의 업로드 한도는 **100 MB**이고, 넘으면 413을 돌려준다(Pro 100 MB, Business 200 MB, Enterprise 500 MB 이상). 공식 해결책 중 하나가 "upload ... through an unproxied (grey-clouded) DNS record"이다. ([Error 413](https://developers.cloudflare.com/support/troubleshooting/http-status-codes/4xx-client-error/error-413))
  - [추론] 프록시는 HTTP(S)만 중계하므로 UDP 8189(WebRTC 미디어)는 어차피 지나가지 못한다.
  - [추론] 주황 구름을 켜면 Caddy의 HTTP-01과 TLS-ALPN 챌린지가 Cloudflare에서 끝나 인증서 발급이 꼬일 수 있다. DNS only면 A 레코드가 VM 공인 IP를 그대로 가리킨다.

## 설계에 바로 영향을 주는 함정 요약

1. A1 무료 한도는 이제 **2 OCPU·12 GB**다. 4/24로 적힌 글은 옛 정보다. 홈 리전은 바꿀 수 없고, 한국 리전은 AD가 하나라 capacity 오류가 나면 기다리거나 PAYG로 업그레이드해야 한다.
2. 포트는 VCN 보안 목록과 Ubuntu iptables(`/etc/iptables/rules.v4`) **두 곳**에서 연다. UDP 8189를 잊지 않는다.
3. MediaMTX의 추가 ICE 호스트는 VM **공인 IP**다(VM 안에서는 보이지 않는다).
4. Cloudflare DNS는 **DNS only**로 둔다. 프록시를 켜면 100 MB 업로드 한도에 걸리고 UDP가 지나가지 못한다.
5. Caddy의 `read_body_idle` 기본값은 1m이다. 2 GiB 업로드에는 `request_body max_size`를 지정한다.
6. dx 데스크톱 번들은 **실행하는 OS·아키텍처용만** 만든다. 그래서 CI 매트릭스가 필요하다. WiX와 NSIS는 dx가 자동으로 받는다.
7. 데스크톱에서는 `set_server_url`을 **launch 전에 한 번만** 부른다. 웹뷰 JS fetch의 상대 경로는 동작하지 않고, 절대 경로로 바꾸면 CORS가 필요하다.
8. macOS WKWebView의 WebRTC가 불확실하다. 실제 .app에서 먼저 확인한다.
9. macOS 15 이상은 우클릭으로 열 수 없고 "Open Anyway"를 거쳐야 한다. arm64는 ad-hoc 서명이 필요하다.
