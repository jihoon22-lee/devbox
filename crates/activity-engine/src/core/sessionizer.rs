use crate::core::models::ClosedSession;

/// 연속 관찰을 세션으로 병합하는 순수 로직.
/// 폴러가 2초마다 (app, title)을 관찰하면 `observe`에 넘긴다.
/// 같은 app+title이 이어지면 열린 세션의 end_ts만 갱신하고,
/// 다른 값이 들어오면 이전 세션을 닫아 반환한다.
#[derive(Debug, Default)]
pub struct Sessionizer {
    current: Option<OpenSession>,
    pending: Vec<ClosedSession>,
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
        Self {
            current: None,
            pending: Vec::new(),
        }
    }

    /// 새 관찰을 반영하고, 닫힌 세션이 있으면 반환한다.
    pub fn observe(&mut self, app: String, title: String, ts: i64) -> Option<ClosedSession> {
        match &mut self.current {
            Some(cur) if cur.app == app && cur.title == title => {
                cur.end_ts = cur.end_ts.max(ts);
                None
            }
            Some(cur) => {
                cur.end_ts = ts.max(cur.end_ts);
                let closed = cur.close();
                self.current = Some(OpenSession {
                    app,
                    title,
                    start_ts: ts.max(cur.end_ts),
                    end_ts: ts.max(cur.end_ts),
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

    /// Closed observations after the latest input stay unsettled until input
    /// advances or an explicit pause ends collection. An idle cutoff can then
    /// trim every foreground/title interval in the idle tail.
    pub fn observe_with_input(
        &mut self,
        app: String,
        title: String,
        ts: i64,
        last_input: i64,
    ) -> Vec<ClosedSession> {
        if let Some(closed) = self.observe(app, title, ts) {
            self.pending.push(closed);
        }
        let (confirmed, pending): (Vec<_>, Vec<_>) = self
            .pending
            .drain(..)
            .partition(|session| session.end_ts <= last_input);
        self.pending = pending;
        confirmed
    }
    pub fn close_at_last_input(&mut self, last_input: i64) -> Vec<ClosedSession> {
        if let Some(mut current) = self.current.take() {
            current.end_ts = last_input.max(current.start_ts);
            self.pending.push(current.close());
        }
        self.pending
            .drain(..)
            .filter_map(|mut session| {
                session.end_ts = session.end_ts.min(last_input).max(session.start_ts);
                (session.end_ts > session.start_ts).then_some(session)
            })
            .collect()
    }
    pub fn finish_all(&mut self, ts: i64) -> Vec<ClosedSession> {
        if let Some(closed) = self.finish(ts) {
            self.pending.push(closed);
        }
        self.pending.drain(..).collect()
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
    fn idle_retroactively_excludes_all_last_input_tail_intervals() {
        let mut s = Sessionizer::new();
        assert!(s
            .observe_with_input("owned".into(), "first".into(), 900_000, 900_000)
            .is_empty());
        assert!(s
            .observe_with_input("owned".into(), "first".into(), 1_000_000, 1_000_000)
            .is_empty());
        assert!(s
            .observe_with_input("owned".into(), "tail1".into(), 1_100_000, 1_000_000)
            .is_empty());
        assert!(s
            .observe_with_input("owned".into(), "tail2".into(), 1_200_000, 1_000_000)
            .is_empty());
        s.observe_with_input("owned".into(), "tail2".into(), 1_298_000, 1_000_000);
        let settled = s.close_at_last_input(1_000_000);
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].end_ts, 1_000_000);
        assert_eq!(settled[0].start_ts, 900_000);
        assert!(s
            .observe_with_input("owned".into(), "resume".into(), 1_400_000, 1_400_000)
            .is_empty());
        assert_eq!(s.finish_all(1_410_000)[0].end_ts, 1_410_000);
    }
    #[test]
    fn explicit_pause_keeps_actual_pause_time_and_wall_clock_reversal_never_makes_negative_spans() {
        let mut s = Sessionizer::new();
        s.observe_with_input("a".into(), "a".into(), 100, 100);
        s.observe_with_input("a".into(), "a".into(), 90, 90);
        s.observe_with_input("b".into(), "b".into(), 95, 90);
        let sessions = s.finish_all(120);
        assert!(sessions
            .iter()
            .all(|session| session.end_ts >= session.start_ts));
        assert_eq!(sessions.last().unwrap().end_ts, 120);
    }

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
