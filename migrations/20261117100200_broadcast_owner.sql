-- 3단계: 방송의 소유자(방송을 만든 사용자).
-- 1·2단계에서 만든 방송은 소유자가 없다(화면에는 "익명"). 사용자는 지우지 않으므로 지울 때의 동작은 정하지 않는다.
ALTER TABLE broadcasts ADD COLUMN owner_id BIGINT REFERENCES users (id);
