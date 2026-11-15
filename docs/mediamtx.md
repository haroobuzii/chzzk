<!--
이 문서는 2단계를 만들 때 MediaMTX 1.21.3 소스·문서와 MDN에서 확인한 사실 메모다.
- 실제로 쓰는 설정은 docker-compose.yml의 configs.mediamtx에 있다. 아래 2절은 그 설정을 고른 근거다.
- "inference"/"unverified" 표시가 붙은 항목은 추론이다.
- 2026-11-15에 실제 서버로 확인: /v3/webrtc/sessions/... 와 /v3/webrtcsessions/... 두 경로 모두 응답한다.
  kick은 없는 id에 404를 돌려준다. paths/list의 online 필드로 방송 중을 판단한다.
MediaMTX 버전을 올릴 때는 이 문서의 사실부터 다시 확인한다.
-->

# MediaMTX + WebRTC (WHIP/WHEP) notes for chzzk live broadcasts

All MediaMTX facts checked against tag **v1.21.3** (source code + docs in repo at that tag), 2026-11-15.
`SRC` = `https://github.com/bluenviron/mediamtx/blob/v1.21.3`

## 1. Version / image

- Latest stable: **v1.21.3** (published 2026-10-25; not prerelease). Source: https://api.github.com/repos/bluenviron/mediamtx/releases
- Docker image: `bluenviron/mediamtx`. Tags `1.21.3`, `1.21.3-ffmpeg`, `1.21.3-rpi`, `1.21.3-ffmpeg-rpi` exist on Docker Hub. `:1` floats to latest 1.x.
  **Pin `bluenviron/mediamtx:1.21.3`** (base image has no shell utils, so no curl healthcheck inside).
  Source: `SRC/docs/1-kickoff/2-install.md`, https://hub.docker.com/r/bluenviron/mediamtx/tags
- Config in Docker (docs pattern): mount folder at `/etc/mediamtx`, `working_dir: /etc/mediamtx`, command `./mediamtx.yml`.
  Source: `SRC/docs/2-features/04-configuration.md`

## 2. Minimal mediamtx.yml (WebRTC + API only)

Defaults in v1.21.3 `mediamtx.yml`: rtsp/rtmp/hls/webrtc/srt/**moq** = true; api/metrics/pprof/playback = false.
**New in 1.2x: `moq: true` by default (ports 8892 tcp+udp, 8893 udp) — must be turned off too.**
Unspecified keys keep defaults, so a short file is fine. Source: `SRC/mediamtx.yml`

```yaml
logLevel: info
api: true
apiAddress: :9997
metrics: false
pprof: false
playback: false
rtsp: false
rtmp: false
hls: false
srt: false
moq: false
webrtc: true
webrtcAddress: :8889
webrtcEncryption: false          # TLS is terminated by cloudflared; media is DTLS-SRTP anyway
webrtcAllowOrigins: ["*"]        # default; browser never calls MediaMTX directly (axum proxies)
webrtcLocalUDPAddress: :8189     # default; single UDP ICE mux port
webrtcLocalTCPAddress: ""        # default (disabled)
webrtcIPsFromInterfaces: false   # container IPs (172.x) are useless to the phone
webrtcAdditionalHosts: []        # set via MTX_WEBRTCADDITIONALHOSTS=${LAN_IP}
authMethod: internal
authInternalUsers:
  - user: any
    pass:
    ips: []                      # any IP: see Docker gotcha below
    permissions:
      - action: publish
      - action: read
      - action: api
paths:
  all_others:
```

- Env override: `MTX_<PARAMNAME uppercased>`, arrays = comma-separated, list items by index:
  `MTX_WEBRTCADDITIONALHOSTS=192.168.0.10`, `MTX_AUTHINTERNALUSERS_0_USER=...`. Source: `SRC/docs/2-features/04-configuration.md`
- Install doc literally uses `-e MTX_WEBRTCADDITIONALHOSTS=192.168.x.x` for Docker: "Fill ... with the IP that will be used to connect to the server." Source: `SRC/docs/1-kickoff/2-install.md`
- Default auth (yml comments): user `any` gets publish/read/playback from any IP; a second `any` user with `ips: ["127.0.0.1","::1"]` gets api/metrics/pprof. Source: `SRC/mediamtx.yml` lines 62-92.
- **Docker gotcha (inference):** docs say Docker's network stack "sometimes replaces" real source IPs (install doc, RTSP UDP note). On Docker Desktop for Mac, requests arriving through a published port are not seen as 127.0.0.1 inside the container, so the default localhost-only `api` user would give **401 on /v3/...**. Granting `api` with `ips: []` is safe only because the host ports are bound to 127.0.0.1 (section 3). Verify once with `curl localhost:9997/v3/paths/list`.
- `webrtcIPsFromInterfaces` default true would advertise container interface IPs; harmless but wasted candidates. With false + `webrtcAdditionalHosts=[LAN_IP]`, the answer's host candidate is `LAN_IP:8189` udp. Source: `SRC/mediamtx.yml` 405-415, `SRC/docs/2-features/24-webrtc-specific-features.md` ("If clients are on the same LAN as the server, add the LAN address of the server").

## 3. docker compose ports (Docker Desktop for Mac)

```yaml
mediamtx:
  image: bluenviron/mediamtx:1.21.3
  working_dir: /etc/mediamtx
  command: ["./mediamtx.yml"]
  volumes: ["./mediamtx:/etc/mediamtx:ro"]
  environment:
    MTX_WEBRTCADDITIONALHOSTS: ${LAN_IP:?set LAN_IP to this Mac's Wi-Fi IP}
  ports:
    - "127.0.0.1:8889:8889"     # WHIP/WHEP HTTP: localhost only
    - "127.0.0.1:9997:9997"     # control API: localhost only
    - "8189:8189/udp"           # ICE/media: must be reachable from LAN (phone + viewers)
```
- Docs: "If there's a NAT / container between server and clients, it must be configured to route all incoming UDP packets on port 8189 to the server ... `-p 8189:8189/udp`". `--network=host` "is not compatible with Windows, macOS". Source: `SRC/docs/2-features/24-webrtc-specific-features.md`, `SRC/docs/1-kickoff/2-install.md`
- Host and container port must be the same (8189) because the advertised candidate is `LAN_IP:8189`.
- Fallback if UDP is blocked: `webrtcLocalTCPAddress: :8189` + `8189:8189/tcp`. Source: same webrtc doc.
- macOS application firewall (if enabled) may need to allow Docker incoming connections — unverified, check if phone can't connect.

## 4. WHIP/WHEP HTTP in MediaMTX (`internal/servers/webrtc/http_server.go`, `session.go`)

- Routes: `^/(.+?)/(whip|whep)$` (no session) and `^/(.+?)/(whip|whep)/(.+?)$` (session). Path name may contain `/`, so `live/42` → `POST /live/42/whip`, `POST /live/42/whep`.
- **POST** (create): request `Content-Type: application/sdp` (else 400 `invalid Content-Type`), body = SDP offer.
  Success **201**, `Content-Type: application/sdp`, body = SDP answer. Headers:
  `Location`, `ETag: *`, `ID: <session uuid>`, `Accept-Patch: application/trickle-ice-sdpfrag`, `Link` (ICE servers; empty when none configured),
  `Access-Control-Expose-Headers: ETag, ID, Accept-Patch, Link, Location`.
- **Location is a relative path**: `/{path}/whip/{secret-uuid}` (or `/whep/`), plus `?{original query}` if the POST had one. The `secret` is a different UUID from the `ID` header (ID = session id used by the control API). MediaMTX's own JS resolves it with `new URL(location, postUrl)`.
- Errors: JSON body `{"status":"error","error":"..."}`. Codes seen: 400 (bad SDP / content type / generic), 401 (auth, may add `WWW-Authenticate: Basic`), **404 on WHEP when path has no publisher** (`PathNoStreamAvailableError`), 406 (unsupported track count/codecs on WHIP), 500. GET/HEAD/PUT on the endpoint → 405.
- Answer is built with `CreateFullAnswer` → contains all server candidates; **non-trickle works**: send offer after `iceGatheringState === "complete"`, the offer's candidates are used and no PATCH is needed.
- **PATCH** `/{path}/whip/{secret}`: `Content-Type: application/trickle-ice-sdpfrag` (else 400). `If-Match` is not checked by the server (MediaMTX JS sends `If-Match: *`). Returns **204** (candidates added) or **200** with `application/trickle-ice-sdpfrag` + `ETag: "<ufrag>"` on ICE restart; 404 if session unknown.
- **DELETE** `/{path}/whip/{secret}`: **200** `{"status":"ok"}`; 404 if not found; 400 if secret is not a UUID.
- **OPTIONS** on `/{path}/whip|whep`: 204, `Access-Control-Allow-Methods: OPTIONS, GET, POST, PATCH, DELETE`, `Access-Control-Allow-Headers: Authorization, Content-Type, If-Match`, `Link` with ICE servers. Any OPTIONS carrying `Access-Control-Request-Method` is answered 204 by a preflight middleware. CORS origin governed by `webrtcAllowOrigins`.
- `overridePublisher: true` (path default): a new WHIP publish to the same path kicks the old publisher. Source: `SRC/mediamtx.yml` line 602.
- Specs cited by MediaMTX: WHIP = RFC 9725; WHEP = draft-ietf-wish-whep. Source: `SRC/docs/7-misc/4-specifications.md`
- **Proxy gotcha:** axum must rewrite `Location` from `/live/42/whip/<secret>` to its own route (e.g. `/api/live/42/whip/<secret>`) or not expose it; otherwise the browser resolves it against the tunnel origin and PATCH/DELETE hit the app's 404.

## 5. Control API (`internal/defs/api_path.go`, `api_webrtc.go`, `internal/apidocsgen/openapi.template.yaml`)

- `GET /v3/paths/list?page=0&itemsPerPage=100` → `{"itemCount","pageCount","items":[APIPath]}`. `GET /v3/paths/get/{name}` (name like `live/42`, URL-encode the `/`? path param — test it; list is simpler).
- `APIPath` fields: `name`, `confName`, `ready`(**deprecated**), `readyTime`(deprecated), `available`, `availableTime`, `online`, `onlineTime`, `source` (`{type,id}` or null), `tracks`(deprecated), `tracks2`, `readers` (`[{type,id}]`), `inboundBytes`, `outboundBytes`, `inboundFramesInError`, ...
- "Has an active publisher": `online == true` (and `source.type == "webRTCSession"` for WHIP). `available` also counts always-available offline segments; with alwaysAvailable off they match. `ready` still emitted but deprecated.
  Sample (shape from structs, values illustrative):
  ```json
  {"itemCount":1,"pageCount":1,"items":[{"name":"live/42","confName":"all_others",
    "ready":true,"readyTime":"2026-11-15T10:00:00Z","available":true,"availableTime":"2026-11-15T10:00:00Z",
    "online":true,"onlineTime":"2026-11-15T10:00:00Z",
    "source":{"type":"webRTCSession","id":"6c1f...-uuid"},
    "tracks2":[...],"readers":[{"type":"webRTCSession","id":"..."}],
    "inboundBytes":123456,"outboundBytes":0,"inboundFramesInError":0}]}
  ```
- Kick: `POST /v3/webrtc/sessions/kick/{id}` → 200 `{"status":"ok"}`, 404 if not found. `id` = `source.id` from the path, = `ID` header from WHIP POST. Also `GET /v3/webrtc/sessions/list`, `/get/{id}`; session has `state` = `"publish"|"read"`, `path`, `remoteAddr`, `peerConnectionEstablished`.
- No "kick path publisher" endpoint; kick the session by id (or DELETE the WHIP session URL).
- Source: `SRC/docs/2-features/20-control-api.md`, `SRC/internal/apidocsgen/openapi.template.yaml` (lines 861, 1816-1894).

## 6. Browser side (MDN + MediaMTX publisher.js/reader.js)

Publish (non-trickle WHIP):
```js
const stream = await navigator.mediaDevices.getUserMedia({
  video: { facingMode: { ideal: "environment" }, width: { ideal: 1280 }, height: { ideal: 720 } },
  audio: true,
});
const pc = new RTCPeerConnection();               // no ICE servers needed on LAN
const vt = pc.addTransceiver(stream.getVideoTracks()[0], { direction: "sendonly", streams: [stream] });
pc.addTransceiver(stream.getAudioTracks()[0], { direction: "sendonly", streams: [stream] });
const caps = RTCRtpSender.getCapabilities("video").codecs;
vt.setCodecPreferences([...caps.filter(c => c.mimeType === "video/H264"), ...caps.filter(c => c.mimeType !== "video/H264")]);
await pc.setLocalDescription(await pc.createOffer());
await new Promise(r => pc.iceGatheringState === "complete" ? r() :
  pc.addEventListener("icegatheringstatechange", () => pc.iceGatheringState === "complete" && r()));
const res = await fetch(`/api/live/${id}/whip?key=...`, { method: "POST",
  headers: { "Content-Type": "application/sdp" }, body: pc.localDescription.sdp });
if (res.status !== 201) throw new Error(await res.text());
await pc.setRemoteDescription({ type: "answer", sdp: await res.text() });
```
Camera switch: `await vt.sender.replaceTrack(newVideoTrack)` (no renegotiation; stop old track first on iOS, which often can't open two cameras at once).
Read (WHEP): `pc.addTransceiver("video",{direction:"recvonly"}); pc.addTransceiver("audio",{direction:"recvonly"}); pc.ontrack = e => video.srcObject = e.streams[0];` then same POST/answer flow; 404 = not live yet.
Wake lock: `let lock = await navigator.wakeLock.request("screen");` re-request on `visibilitychange` when visible (released automatically when page is hidden); `lock.release()` on stop. Requires secure context.

- MediaMTX does **not transcode**: viewers must decode the publisher's codec. Safari/iOS → prefer **H264** on publish (works everywhere). Docs: H264 with B-frames unsupported by browsers; browser encoders don't emit B-frames. Source: `SRC/docs/2-features/24-webrtc-specific-features.md`. MediaMTX WebRTC codecs: video AV1/VP9/VP8/H265/H264, audio Opus/G722/G711. Source: `SRC/docs/3-publish/03-webrtc.md`
- MediaMTX's own pages: publisher uses `addTrack` + SDP munging for codec choice; reader uses `addTransceiver(kind,{direction:"recvonly"})`; `<video muted autoplay playsinline>`. Source: `SRC/internal/servers/webrtc/{publisher.js,reader.js,publish_index.html,read_index.html}`
- Browser support (MDN BCD, api/*.json @ main): `RTCRtpTransceiver.setCodecPreferences` Safari 13.1, Firefox 128; `RTCRtpSender.getCapabilities` Safari 12.1; `replaceTrack` Safari 11; `WakeLock.request` Safari 16.4 desktop, **iOS Safari 18.4** (BCD value), Firefox 126.
  Docs: https://developer.mozilla.org/en-US/docs/Web/API/RTCRtpTransceiver/setCodecPreferences , .../RTCRtpSender/replaceTrack , .../MediaDevices/getUserMedia , .../Screen_Wake_Lock_API
- iOS quirks: autoplay only if `muted` (unmute needs a user gesture → "소리 켜기" button); `playsinline` required or iOS goes fullscreen; getUserMedia only in secure contexts (HTTPS / localhost) — hence the tunnel.

## 7. cloudflared quick tunnel

- Only HTTP signaling (page, POST/PATCH/DELETE to axum) goes through `https://*.trycloudflare.com`; ICE candidate in the answer is `LAN_IP:8189/udp`, so the phone sends media directly over Wi-Fi. Phone must be on the same LAN; client-isolated/guest Wi-Fi breaks it.
- Quick Tunnel limits (Cloudflare docs): no uptime guarantee, **200 in-flight requests max (429 beyond)**, **no Server-Sent Events**, hostname changes every run. Source: https://developers.cloudflare.com/tunnel/get-started/quick-tunnels/
  → don't use SSE for live status; poll instead.
- Keep `Location` rewriting and relative URLs; never put `localhost:8889` in anything the browser sees.
- iOS Safari with mDNS host candidates: phone's offer may contain `*.local` candidates; MediaMTX still learns the phone's address from incoming STUN checks (peer-reflexive), so non-trickle LAN works as long as the phone can reach `LAN_IP:8189` (inference; verify on device).
