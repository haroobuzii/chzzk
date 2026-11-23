//! 방송을 그리는 조각: 목록 칸("지금 라이브"와 전체 방송)과 카드, 상세 화면의 방송 정보와 종료 안내.
//! 웹 화면과 앱이 함께 쓴다.
//!
//! 카드를 누르면 갈 곳만 쓰는 쪽이 정한다. 웹은 웹 화면의 상세 주소, 앱은 앱 화면의 상세 주소다.

use dioxus::prelude::*;

use crate::display::{broadcast_duration, Mark};
use crate::refresh::{sleep_ms, REFRESH_MS};
use crate::{format_kst, Broadcast, BroadcastKind};

/// 상세 화면의 방송 정보: 제목(방송 중이면 앞에 LIVE), 소유자, 시각.
/// 녹화는 등록 시각, 라이브는 시작 시각을 쓰고, 종료된 라이브는 방송한 시간도 쓴다.
#[component]
pub fn BroadcastInfo(broadcast: Broadcast) -> Element {
    let shown_time = broadcast.shown_time();
    rsx! {
        h1 { class: "detail-title",
            if broadcast.mark() == Mark::Live {
                span { class: "inline-badge {Mark::Live.class()}", "{Mark::Live.label()}" }
            }
            "{broadcast.title}"
        }
        p { class: "detail-owner", "{broadcast.owner_label()}" }
        if broadcast.kind == BroadcastKind::Recorded {
            p { class: "card-time", "{format_kst(&shown_time)}" }
        } else {
            p { class: "card-time", "시작 {format_kst(&shown_time)}" }
            if let Some(ended_at) = broadcast.ended_at {
                p { class: "card-time", "방송한 시간 {broadcast_duration(shown_time, ended_at)}" }
            }
        }
    }
}

/// 방송 중인 라이브의 상태를 한 번 확인한 뒤 화면에 할 일.
#[derive(Debug, PartialEq)]
pub enum LiveCheck {
    /// 지금 화면을 그대로 둔다.
    Keep,
    /// 바뀐 방송(제목 변경, 종료)으로 바꿔 보여 준다.
    Show(Broadcast),
    /// 방송이 지워졌다.
    Deleted,
}

/// 확인 결과(`latest`)를 지금 보이는 방송(`current`)과 견준다.
/// 확인에 실패하면 지금 화면을 그대로 두고 다음에 다시 묻는다.
pub fn live_check<E>(current: &Broadcast, latest: Result<Option<Broadcast>, E>) -> LiveCheck {
    match latest {
        Ok(Some(latest)) if latest != *current => LiveCheck::Show(latest),
        Ok(None) => LiveCheck::Deleted,
        Ok(Some(_)) | Err(_) => LiveCheck::Keep,
    }
}

/// 방송 중인 라이브의 상태(제목 포함)를 5초마다 `fetch`로 확인해 반영한다. 웹과 앱이 함께 쓴다.
/// 보이는 방송과, 방송이 지워졌는지를 돌려준다. 종료되거나 지워지면 더 묻지 않는다.
pub fn use_live_status<F, Fut, E>(
    broadcast: Broadcast,
    fetch: F,
) -> (Signal<Broadcast>, Signal<bool>)
where
    F: Fn(i64) -> Fut + Clone + 'static,
    Fut: std::future::Future<Output = Result<Option<Broadcast>, E>> + 'static,
{
    let mut current = use_signal(|| broadcast);
    let mut deleted = use_signal(|| false);
    use_effect(move || {
        let id = current.peek().id;
        let fetch = fetch.clone();
        spawn(async move {
            while current.peek().mark() == Mark::Live {
                sleep_ms(REFRESH_MS).await;
                let latest = fetch(id).await;
                let check = live_check(&current.peek(), latest);
                match check {
                    LiveCheck::Keep => {}
                    LiveCheck::Show(latest) => current.set(latest),
                    LiveCheck::Deleted => {
                        deleted.set(true);
                        break;
                    }
                }
            }
        });
    });
    (current, deleted)
}

/// 종료된 라이브 방송의 재생기 자리에 보이는 안내.
#[component]
pub fn LiveEndedNotice() -> Element {
    rsx! {
        div { class: "live-ended", role: "status",
            p { class: "live-ended-title", "방송이 종료되었습니다" }
            p { "종료된 라이브 방송은 다시 볼 수 없습니다." }
        }
    }
}

/// 방송 중인 방송을 모은 "지금 라이브" 칸과 전체 방송 목록. `list`는 비어 있지 않다.
#[component]
pub fn BroadcastSections(
    list: Vec<Broadcast>,
    /// 방송 번호로 그 방송의 상세 화면 주소를 만든다.
    detail_of: Callback<i64, NavigationTarget>,
) -> Element {
    let live_now: Vec<Broadcast> = list
        .iter()
        .filter(|b| b.mark() == Mark::Live)
        .cloned()
        .collect();
    rsx! {
        // 방송 중인 방송이 없으면 이 칸은 보이지 않는다.
        if !live_now.is_empty() {
            section { class: "live-now",
                h2 { class: "section-title", "지금 라이브" }
                ul { class: "broadcast-grid",
                    for broadcast in live_now {
                        li { key: "{broadcast.id}",
                            BroadcastCard { to: detail_of(broadcast.id), broadcast }
                        }
                    }
                }
            }
        }
        section {
            if list.iter().any(|b| b.mark() == Mark::Live) {
                h2 { class: "section-title", "전체 방송" }
            }
            ul { class: "broadcast-grid",
                for broadcast in list {
                    li { key: "{broadcast.id}",
                        BroadcastCard { to: detail_of(broadcast.id), broadcast }
                    }
                }
            }
        }
    }
}

/// 목록의 카드 하나: 썸네일 자리와 표시(녹화·LIVE·종료), 제목, 소유자, 시각.
/// 시각은 녹화는 등록 시각, 라이브는 시작 시각이다.
#[component]
fn BroadcastCard(broadcast: Broadcast, to: NavigationTarget) -> Element {
    let mark = broadcast.mark();
    rsx! {
        article { class: "card",
            Link { class: "card-link", to,
                // 썸네일은 만들지 않으므로 회색 빈 칸을 두고, 왼쪽 위에 표시를 붙인다.
                div { class: "thumbnail",
                    span { class: "badge {mark.class()}", "{mark.label()}" }
                }
                h2 { class: "card-title", title: "{broadcast.title}", "{broadcast.title}" }
                p { class: "card-owner", title: "{broadcast.owner_label()}", "{broadcast.owner_label()}" }
                p { class: "card-time", "{format_kst(&broadcast.shown_time())}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::at;
    use crate::LiveStatus;

    fn live(status: LiveStatus, ended: Option<u32>) -> Broadcast {
        Broadcast {
            id: 1,
            title: "라이브".into(),
            created_at: at(0),
            kind: BroadcastKind::Live,
            status: Some(status),
            started_at: Some(at(1)),
            ended_at: ended.map(at),
            owner: None,
        }
    }

    #[test]
    fn 방송이_끝났으면_끝난_방송으로_바꿔_보여_준다() {
        let ended = live(LiveStatus::Ended, Some(9));
        assert_eq!(
            live_check(
                &live(LiveStatus::Live, None),
                Ok::<_, ()>(Some(ended.clone()))
            ),
            LiveCheck::Show(ended)
        );
    }

    #[test]
    fn 방송이_지워졌으면_지워졌다고_알린다() {
        assert_eq!(
            live_check(&live(LiveStatus::Live, None), Ok::<_, ()>(None)),
            LiveCheck::Deleted
        );
    }

    #[test]
    fn 확인에_실패하거나_그대로면_지금_화면을_둔다() {
        let current = live(LiveStatus::Live, None);
        assert_eq!(live_check(&current, Err(())), LiveCheck::Keep);
        assert_eq!(
            live_check(&current, Ok::<_, ()>(Some(current.clone()))),
            LiveCheck::Keep
        );
    }
}
