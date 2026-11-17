//! 방송을 화면에 어떻게 보여 줄지 정하는 규칙: 카드 표시, 카드 시각, 방송한 시간.

use chrono::{DateTime, Utc};

use crate::{Broadcast, BroadcastKind, LiveStatus};

/// 카드 왼쪽 위에 붙는 표시.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// 녹화 방송.
    Recorded,
    /// 방송 중(빨간 LIVE).
    Live,
    /// 종료된 라이브 방송.
    Ended,
}

impl Mark {
    /// 표시에 쓰는 글자.
    pub fn label(self) -> &'static str {
        match self {
            Mark::Recorded => "녹화",
            Mark::Live => "LIVE",
            Mark::Ended => "종료",
        }
    }

    /// 표시 색을 정하는 CSS 클래스. 모양(`badge`, `inline-badge`)은 쓰는 쪽이 붙인다.
    pub fn class(self) -> &'static str {
        match self {
            Mark::Recorded => "badge-recorded",
            Mark::Live => "badge-live",
            Mark::Ended => "badge-ended",
        }
    }
}

impl Broadcast {
    /// 카드에 붙일 표시.
    pub fn mark(&self) -> Mark {
        match (self.kind, self.status) {
            (BroadcastKind::Recorded, _) => Mark::Recorded,
            (BroadcastKind::Live, Some(LiveStatus::Live)) => Mark::Live,
            // 목록과 상세에는 방송 중·종료만 온다. 그 밖의 상태는 끝난 방송으로 본다.
            (BroadcastKind::Live, _) => Mark::Ended,
        }
    }

    /// 카드에 보여 줄 시각. 녹화는 등록 시각, 라이브는 시작 시각이다.
    pub fn shown_time(&self) -> DateTime<Utc> {
        match self.kind {
            BroadcastKind::Recorded => self.created_at,
            BroadcastKind::Live => self.started_at.unwrap_or(self.created_at),
        }
    }

    /// 카드와 상세 화면에 보여 줄 소유자. 소유자가 없는 1·2단계 방송은 "익명"이다.
    pub fn owner_label(&self) -> &str {
        self.owner.as_deref().unwrap_or("익명")
    }
}

/// 방송한 시간을 "12분 34초"처럼 쓴다. 한 시간이 넘으면 "1시간 2분 3초"다.
pub fn broadcast_duration(started_at: DateTime<Utc>, ended_at: DateTime<Utc>) -> String {
    let total = (ended_at - started_at).num_seconds().max(0);
    let (hours, minutes, seconds) = (total / 3600, total % 3600 / 60, total % 60);
    if hours > 0 {
        format!("{hours}시간 {minutes}분 {seconds}초")
    } else if minutes > 0 {
        format!("{minutes}분 {seconds}초")
    } else {
        format!("{seconds}초")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::at;
    use chrono::TimeZone;

    fn live(status: LiveStatus, started: Option<u32>, ended: Option<u32>) -> Broadcast {
        Broadcast {
            id: 1,
            title: "라이브".into(),
            created_at: at(0),
            kind: BroadcastKind::Live,
            status: Some(status),
            started_at: started.map(at),
            ended_at: ended.map(at),
            owner: None,
        }
    }

    #[test]
    fn 녹화_방송은_녹화_표시와_등록_시각이다() {
        let recorded = Broadcast {
            id: 1,
            title: "녹화".into(),
            created_at: at(3),
            kind: BroadcastKind::Recorded,
            status: None,
            started_at: None,
            ended_at: None,
            owner: None,
        };
        assert_eq!(recorded.mark(), Mark::Recorded);
        assert_eq!(recorded.shown_time(), at(3));
    }

    #[test]
    fn 소유자는_아이디로_보이고_소유자가_없으면_익명이다() {
        let mut broadcast = live(LiveStatus::Live, Some(5), None);
        assert_eq!(broadcast.owner_label(), "익명");
        broadcast.owner = Some("하루".into());
        assert_eq!(broadcast.owner_label(), "하루");
    }

    #[test]
    fn 방송_중은_LIVE_표시와_시작_시각이다() {
        let broadcast = live(LiveStatus::Live, Some(5), None);
        assert_eq!(broadcast.mark(), Mark::Live);
        assert_eq!(broadcast.shown_time(), at(5));
    }

    #[test]
    fn 종료는_종료_표시와_시작_시각이다() {
        let broadcast = live(LiveStatus::Ended, Some(5), Some(9));
        assert_eq!(broadcast.mark(), Mark::Ended);
        assert_eq!(broadcast.shown_time(), at(5));
    }

    #[test]
    fn 방송한_시간은_시작부터_종료까지를_분과_초로_쓴다() {
        let start = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
        let cases = [
            (0, "0초"),
            (59, "59초"),
            (12 * 60 + 34, "12분 34초"),
            (5 * 60, "5분 0초"),
            (3600 + 2 * 60 + 3, "1시간 2분 3초"),
        ];
        for (seconds, expected) in cases {
            let end = start + chrono::Duration::seconds(seconds);
            assert_eq!(broadcast_duration(start, end), expected, "{seconds}초");
        }
        // 시각이 거꾸로면 0초로 본다.
        assert_eq!(broadcast_duration(at(5), at(1)), "0초");
    }
}
