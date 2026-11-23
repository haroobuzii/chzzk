# 배포 서버(OCI A1, linux/arm64)에 올리는 앱 이미지. 웹 화면(WASM)과 서버를 함께 담는다(ADR 0008).
# short-url과 같은 cargo-chef·dx bundle 다단계 구성이다(docs/deploy-notes.md 4장).
#
# 만들기(맥에서):
#   docker buildx build --platform linux/arm64 --build-arg CHZZK_VERSION=$(git rev-parse --short HEAD) \
#     -t ghcr.io/haroobuzii/chzzk:local --load .

# 1) 빌드 도구가 들어 있는 기본 이미지. rust-toolchain.toml의 Rust 버전과 wasm 대상을 여기서 미리 설치한다.
FROM rust:1-trixie AS chef
WORKDIR /app
COPY rust-toolchain.toml .
RUN rustup toolchain install && cargo install cargo-chef --locked

# 2) 의존성 목록만 뽑아 둔다. 내 코드만 바뀌었을 때 의존성을 다시 빌드하지 않게 하려는 것이다.
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# 3) 의존성을 먼저 빌드하고, 그다음 dx로 화면(WASM)과 서버를 함께 묶는다.
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# dx와 같은 프로필·대상·RUSTFLAGS로 빌드해야 dx bundle이 이 결과를 그대로 쓴다(Cargo.toml의 프로필 설명).
# WASM 쪽 RUSTFLAGS는 dx 0.7.10이 웹 빌드에 붙이는 값과 같다. dx를 올리면 다시 확인한다.
RUN cargo chef cook --profile server-release --target "$(rustc -vV | sed -n 's/^host: //p')" \
        --no-default-features --features server --recipe-path recipe.json \
    && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
        cargo chef cook --profile wasm-release --target wasm32-unknown-unknown --recipe-path recipe.json
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall dioxus-cli@0.7.10 --root /.cargo -y --force
ENV PATH="/.cargo/bin:$PATH"
COPY . .
# /healthz의 version으로 나갈 커밋 해시. 컴파일할 때 읽는다(src/health.rs). 넣지 않으면 "dev"다.
ARG CHZZK_VERSION=dev
ENV CHZZK_VERSION=$CHZZK_VERSION
RUN dx bundle --web --release

# 4) 실행에 필요한 것만 담은 작은 이미지. curl은 compose의 건강 검사가 쓴다.
FROM debian:trixie-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/dx/chzzk/release/web/ /usr/local/app

# 모든 주소에서 8080으로 받는다. 밖에는 열지 않고 Caddy가 같은 compose 네트워크에서 넘겨준다.
# 영상 보관 폴더는 /data이고, compose가 볼륨을 붙인다.
ENV IP=0.0.0.0
ENV PORT=8080
ENV DATA_DIR=/data
EXPOSE 8080

WORKDIR /usr/local/app
ENTRYPOINT ["/usr/local/app/server"]
