# 라이브 송출은 WebRTC로 MediaMTX에 보내고, 내 컴퓨터를 터널로 연다

휴대폰 브라우저가 카메라와 마이크 영상을 WebRTC(WHIP)로 미디어 서버 MediaMTX에 보내고, 시청자 브라우저는 MediaMTX에서 WebRTC(WHEP)로 받는다. 우리 Rust 서버는 라이브 방송의 정보와 방송 상태, 송출 키만 관리하고 영상은 다루지 않는다. 지연이 1초 안팎이고, 시청자가 늘어도 휴대폰은 영상을 한 번만 보낸다. MediaMTX는 DB처럼 Docker Compose로 띄운다.

휴대폰 브라우저는 HTTPS에서만 카메라를 쓰게 해 준다. 그래서 내 컴퓨터에서 실행하고, Cloudflare 무료 터널(`cloudflared`)로 HTTPS 주소를 만든다. 터널은 화면과 송출 연결 요청만 나르고, 영상은 같은 와이파이 안에서 휴대폰과 MediaMTX가 직접 주고받는다. 그래서 휴대폰과 컴퓨터가 같은 와이파이에 있어야 하며, LTE·5G 송출은 범위 밖이다.

## Considered Options

- **휴대폰과 시청자를 직접 연결(P2P)**: 미디어 서버가 없지만, 시청자마다 휴대폰이 영상을 따로 보내야 해서 시청자가 늘면 버티지 못한다.
- **MediaRecorder로 몇 초씩 녹화해 서버에 올리기**: 우리 서버만으로 되지만 지연이 크고, iOS Safari에서 이어 붙여 재생하기 어렵다.
- **Railway 배포**: 1단계 단축 URL처럼 공개 주소를 얻을 수 있지만, Railway는 WebRTC가 쓰는 UDP를 받지 않아 MediaMTX를 올릴 수 없다.
