use crate::core::models::ClosedSession;

/// 연속 관찰을 세션으로 병합하는 순수 로직.
/// 폴러가 2초마다 (app, title)을 관찰하면 `observe`에 넘긴다.
/// 같은 app+title이 이어지면 열린 세션의 end_ts만 갱신하고,
/// 다른 값이 들어오면 이전 세션을 닫아 반환한다.
#[derive(Debug, Default)]
pub struct Sessionizer {
    current: Option<OpenSession>,
}

#[derive(Debug, Clone)]
struct OpenSession {
    app: String,
    title: String,
    start_ts: i64,
    end_ts: i64,
}

impl Sessionizer {
    pub fn new() -> Self {
        Self { current: None }
    }

    /// 새 관찰을 반영하고, 닫힌 세션이 있으면 반환한다.
    pub fn observe(&mut self, app: String, title: String, ts: i64) -> Option<ClosedSession> {
        match &mut self.current {
            Some(cur) if cur.app == app && cur.title == title => {
                cur.end_ts = ts;
                None
            }
            Some(cur) => {
                cur.end_ts = ts.max(cur.end_ts);
                let closed = cur.close();
                self.current = Some(OpenSession {
                    app,
                    title,
                    start_ts: ts,
                    end_ts: ts,
                });
                Some(closed)
            }
            None => {
                self.current = Some(OpenSession {
                    app,
                    title,
                    start_ts: ts,
                    end_ts: ts,
                });
                None
            }
        }
    }

    /// 현재 열린 세션을 마감한다 (앱 종료/추적 중지 시).
    pub fn finish(&mut self, ts: i64) -> Option<ClosedSession> {
        let current = self.current.take();
        current.map(|mut c| {
            c.end_ts = c.end_ts.max(ts);
            c.close()
        })
    }
}

impl OpenSession {
    fn close(&self) -> ClosedSession {
        ClosedSession {
            app: self.app.clone(),
            title: self.title.clone(),
            start_ts: self.start_ts,
            end_ts: self.end_ts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_app_title_extends_session() {
        let mut s = Sessionizer::new();
        assert!(s.observe("vs".into(), "FamilyCard".into(), 100).is_none());
        assert!(s.observe("vs".into(), "FamilyCard".into(), 102).is_none());
        let closed = s.finish(104).expect("finish should close");
        assert_eq!(closed.start_ts, 100);
        assert_eq!(closed.end_ts, 104);
    }

    #[test]
    fn app_change_closes_previous() {
        let mut s = Sessionizer::new();
        s.observe("vs".into(), "FamilyCard".into(), 100);
        let closed = s
            .observe("chrome".into(), "GitHub".into(), 103)
            .expect("should close");
        assert_eq!(closed.app, "vs");
        assert_eq!(closed.end_ts, 103);
        let final_closed = s.finish(105).unwrap();
        assert_eq!(final_closed.app, "chrome");
    }

    #[test]
    fn title_change_within_same_app_also_closes() {
        let mut s = Sessionizer::new();
        s.observe("chrome".into(), "A".into(), 100);
        let closed = s.observe("chrome".into(), "B".into(), 102).unwrap();
        assert_eq!(closed.title, "A");
        assert_eq!(closed.end_ts, 102);
    }

    #[test]
    fn finish_with_no_session_returns_none() {
        let mut s = Sessionizer::new();
        assert!(s.finish(0).is_none());
    }
}
