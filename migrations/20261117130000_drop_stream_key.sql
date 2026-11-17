-- 3단계: 송출과 종료는 송출 키 대신 로그인한 소유자로 지킨다(ADR 0005). 송출 키 해시 칸을 없앤다.
-- 종류별 칸 제약이 이 칸을 요구하므로 제약을 먼저 지우고, 송출 키 조건만 뺀 제약으로 다시 만든다.
ALTER TABLE broadcasts DROP CONSTRAINT broadcasts_kind_fields;

ALTER TABLE broadcasts DROP COLUMN stream_key_hash;

ALTER TABLE broadcasts ADD CONSTRAINT broadcasts_kind_fields CHECK (
    CASE kind
        WHEN 'recorded' THEN
            video_key IS NOT NULL
            AND status IS NULL
            AND started_at IS NULL
            AND ended_at IS NULL
        WHEN 'live' THEN
            video_key IS NULL
            AND status IS NOT NULL
    END
);
