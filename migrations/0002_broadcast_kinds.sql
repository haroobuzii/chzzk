-- 2단계: 방송을 녹화 방송과 라이브 방송 두 종류로 나눈다. 기존 행은 모두 녹화 방송이 된다.
CREATE TYPE broadcast_kind AS ENUM ('recorded', 'live');

-- 라이브 방송의 방송 상태. 준비·방송 중·종료에 더해, 준비에서 끝내 방송 중이 되지 못한 "버려짐"이 있다.
-- 버려짐은 화면에 보이지 않는다.
CREATE TYPE live_status AS ENUM ('ready', 'live', 'ended', 'abandoned');

ALTER TABLE broadcasts
    ADD COLUMN kind            broadcast_kind NOT NULL DEFAULT 'recorded',
    -- 라이브 방송만 가진다.
    ADD COLUMN status          live_status,
    -- 송출 키의 SHA-256 해시(32바이트). 라이브 방송만 가진다. 원문은 저장하지 않는다.
    ADD COLUMN stream_key_hash BYTEA CHECK (octet_length(stream_key_hash) = 32),
    ADD COLUMN started_at      TIMESTAMPTZ,
    ADD COLUMN ended_at        TIMESTAMPTZ,
    -- 영상 키는 녹화 방송만 가지므로 비어 있을 수 있게 한다.
    ALTER COLUMN video_key DROP NOT NULL;

-- 종류에 맞는 칸만 채워지게 한다.
ALTER TABLE broadcasts ADD CONSTRAINT broadcasts_kind_fields CHECK (
    CASE kind
        WHEN 'recorded' THEN
            video_key IS NOT NULL
            AND status IS NULL
            AND stream_key_hash IS NULL
            AND started_at IS NULL
            AND ended_at IS NULL
        WHEN 'live' THEN
            video_key IS NULL
            AND status IS NOT NULL
            AND stream_key_hash IS NOT NULL
    END
);

-- 방송 상태에 맞는 시각만 채워지게 한다. 녹화 방송은 상태가 비어 있어 이 제약에 걸리지 않는다.
--   준비: 시작·종료 시각 없음
--   방송 중: 시작 시각만 있음
--   종료: 시작·종료 시각 모두 있음
--   버려짐: 시작 시각 없음(버려진 시각은 종료 시각에 남길 수 있다)
ALTER TABLE broadcasts ADD CONSTRAINT broadcasts_status_times CHECK (
    CASE status
        WHEN 'ready' THEN started_at IS NULL AND ended_at IS NULL
        WHEN 'live' THEN started_at IS NOT NULL AND ended_at IS NULL
        WHEN 'ended' THEN started_at IS NOT NULL AND ended_at IS NOT NULL
        WHEN 'abandoned' THEN started_at IS NULL
        ELSE TRUE
    END
);
