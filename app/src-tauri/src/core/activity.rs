//! Activity collection policy (platform independent): consent, pause, privacy
//! rules and the offline buffer. Titles leave this module only after the rules;
//! without compiled rules no title leaves at all (01 §6.2 fail-closed).
use devbox_activity::model::event::{Observation, ObservationKind};
use devbox_activity::model::privacy::{CompiledRules, PrivacyRules};
use devbox_win::activity_buffer::{Batch, OfflineBuffer};
use std::path::Path;

/// What the platform must do with the native hook after a state change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    Start,
    Stop,
    Keep,
}

#[derive(Debug, Default)]
pub struct Collector {
    consent: bool,
    paused: bool,
    /// `None`: not received from the daemon yet, or failed to compile.
    rules: Option<CompiledRules>,
    buffer: OfflineBuffer,
    /// Nameless stop control retained outside the evictable observation queue.
    /// It must reach the daemon before any observation after renewed consent.
    withdrawal: Option<Observation>,
    last_input_ms: i64,
    /// The foreground is an excluded process and its boundary is queued.
    excluded: bool,
}

impl Collector {
    /// Saved settings at app start: no `pause` is queued for a saved pause.
    pub fn restore(consent: bool, paused: bool) -> (Self, Hook) {
        let collector = Self {
            consent,
            paused,
            ..Self::default()
        };
        let hook = if collector.running() {
            Hook::Start
        } else {
            Hook::Keep
        };
        (collector, hook)
    }

    /// The native hook runs only with consent and while not paused.
    pub fn running(&self) -> bool {
        self.consent && !self.paused
    }

    pub fn consent(&self) -> bool {
        self.consent
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Withdraw all unsent names/titles, but close any already-ingested session.
    /// A newer seq invalidates an uncertain uplink batch. Until ACK, keep the
    /// earliest cutoff: repeated off/on cannot turn unconfirmed time into activity.
    pub fn set_consent(&mut self, consent: bool) -> Hook {
        let was = self.running();
        let withdrawing = self.consent && !consent;
        self.consent = consent;
        self.excluded = false;
        if !consent {
            self.buffer.clear();
            if withdrawing {
                let cutoff = self
                    .withdrawal
                    .as_ref()
                    .map_or(self.last_input_ms, |boundary| boundary.last_input_ms);
                self.boundary(ObservationKind::Idle, cutoff, cutoff);
                self.withdrawal = self.buffer.drain(1).events.pop();
                self.buffer.clear();
            }
        }
        self.transition(was)
    }

    /// Pausing queues one `pause` (open sessions end now); later observations
    /// are dropped until resumed.
    pub fn set_paused(&mut self, paused: bool, at_ms: i64) -> Hook {
        let was = self.running();
        if paused && was {
            let last_input_ms = self.last_input_ms;
            self.boundary(ObservationKind::Pause, at_ms, last_input_ms);
        }
        self.paused = paused;
        self.excluded = false;
        self.transition(was)
    }

    fn transition(&self, was: bool) -> Hook {
        match (was, self.running()) {
            (false, true) => Hook::Start,
            (true, false) => Hook::Stop,
            _ => Hook::Keep,
        }
    }

    /// A nameless boundary. `Pause` (explicit pause) ends open sessions at
    /// `at_ms`; `Idle` ends them at `last_input_ms`, so time without input
    /// (an excluded lock screen, a suspend) is never counted.
    fn boundary(&mut self, kind: ObservationKind, at_ms: i64, last_input_ms: i64) -> u64 {
        self.buffer.push(Observation {
            seq: 0,
            at_ms,
            kind,
            app: None,
            title: None,
            last_input_ms: if last_input_ms == 0 {
                at_ms
            } else {
                last_input_ms
            },
        })
    }

    /// The daemon's rules (`activity.rules`); `None` or a compile failure drops titles.
    pub fn configure(&mut self, rules: Option<&PrivacyRules>) {
        self.rules = rules.and_then(|rules| CompiledRules::compile(rules).ok());
    }

    /// Applies consent, pause and the rules, then queues. Returns the `seq`
    /// given to the queued observation.
    pub fn accept(&mut self, mut observation: Observation) -> Option<u64> {
        if !self.running() {
            return None;
        }
        self.last_input_ms = self.last_input_ms.max(observation.last_input_ms);
        let title = observation.title.take();
        if let Some(app) = observation.app.take() {
            match &self.rules {
                None => observation.app = Some(app),
                Some(rules) => match rules.apply(&app, title.as_deref().unwrap_or("")) {
                    // Excluded: one nameless `idle` boundary instead, so the
                    // previous window's session ends at its last input and does
                    // not absorb this time (v0.9.0 dropped the excluded session
                    // after sessionizing). Not `pause`: that would extend it to
                    // `at_ms` and store the idle tail (AU-K2).
                    None => {
                        if observation.kind != ObservationKind::Foreground || self.excluded {
                            return None;
                        }
                        self.excluded = true;
                        return Some(self.boundary(
                            ObservationKind::Idle,
                            observation.at_ms,
                            observation.last_input_ms,
                        ));
                    }
                    Some((app, title)) => {
                        observation.app = Some(app);
                        observation.title = (!title.is_empty()).then_some(title);
                    }
                },
            }
            if observation.kind == ObservationKind::Foreground {
                self.excluded = false;
            }
        }
        if self.excluded && observation.kind == ObservationKind::Input {
            return None;
        }
        Some(self.buffer.push(observation))
    }

    pub fn drain(&self, max: usize) -> Batch {
        let boundary = self.withdrawal.as_ref().filter(|_| max > 0);
        let mut batch = self
            .buffer
            .drain(max.saturating_sub(usize::from(boundary.is_some())));
        if let Some(boundary) = boundary {
            batch.events.insert(0, boundary.clone());
            batch.upto = batch.events.last().map_or(0, |event| event.seq);
        }
        batch
    }

    pub fn ack(&mut self, upto: u64) {
        self.buffer.ack(upto);
        if self
            .withdrawal
            .as_ref()
            .is_some_and(|event| event.seq <= upto)
        {
            self.withdrawal = None;
        }
    }

    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.buffer.len() + usize::from(self.withdrawal.is_some())
    }
}

/// Saving attempts for a consent or pause change.
pub const SAVE_ATTEMPTS: usize = 3;

/// A consent or pause change. Stopping (withdrawal, pause) takes effect before
/// saving and stays in effect if saving fails; starting waits for a successful
/// save. The save is retried, so a withdrawal is not resumed at the next start
/// by a stale `consent = true` unless every attempt failed (then it reports).
/// `run` gets each hook change without the collector locked.
pub fn change(
    collector: &std::sync::Mutex<Collector>,
    consent: Option<bool>,
    paused: Option<bool>,
    at_ms: i64,
    mut save: impl FnMut() -> std::io::Result<()>,
    mut run: impl FnMut(Hook),
) -> std::io::Result<()> {
    let mut apply = |consent: Option<bool>, paused: Option<bool>| {
        if let Some(paused) = paused {
            let hook = lock(collector).set_paused(paused, at_ms);
            run(hook);
        }
        if let Some(consent) = consent {
            let hook = lock(collector).set_consent(consent);
            run(hook);
        }
    };
    apply(consent.filter(|c| !c), paused.filter(|p| *p));
    let mut saved = save();
    for _ in 1..SAVE_ATTEMPTS {
        if saved.is_ok() {
            break;
        }
        saved = save();
    }
    if saved.is_ok() {
        apply(consent.filter(|c| *c), paused.filter(|p| !p));
    }
    saved
}

fn lock(collector: &std::sync::Mutex<Collector>) -> std::sync::MutexGuard<'_, Collector> {
    collector.lock().unwrap_or_else(|error| error.into_inner())
}

/// `runId` for this app start: the daemon accepts 1..=64 of `[A-Za-z0-9_-]`.
pub fn run_id(epoch_nanos: u128, pid: u32, random: u64) -> String {
    format!("{epoch_nanos:x}-{pid:x}-{random:x}")
}

/// v0.9.0 Knowledge keeps one `%LOCALAPPDATA%\com.devbox.v08.knowledge.i<suffix>`
/// per install; `active-stores.json` there means it may be collecting too.
/// Read-only existence check; links and junctions are not followed.
pub fn legacy_knowledge(local_app_data: &Path) -> bool {
    const PREFIX: &str = "com.devbox.v08.knowledge.i";
    let Ok(entries) = std::fs::read_dir(local_app_data) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let suffixed = name
            .to_str()
            .is_some_and(|name| name.len() > PREFIX.len() && name.starts_with(PREFIX));
        // `file_type` and `symlink_metadata` do not follow links (junctions on Windows).
        suffixed
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && std::fs::symlink_metadata(entry.path().join("active-stores.json"))
                .is_ok_and(|meta| meta.file_type().is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(kind: ObservationKind, app: Option<&str>, title: Option<&str>, at: i64) -> Observation {
        Observation {
            seq: 0,
            at_ms: at,
            kind,
            app: app.map(Into::into),
            title: title.map(Into::into),
            last_input_ms: at,
        }
    }
    fn fg(app: &str, title: &str, at: i64) -> Observation {
        obs(ObservationKind::Foreground, Some(app), Some(title), at)
    }
    fn rules(processes: &[&str], redact: &[&str]) -> PrivacyRules {
        PrivacyRules {
            excluded_processes: processes.iter().map(|s| s.to_string()).collect(),
            redact_title_patterns: redact.iter().map(|s| s.to_string()).collect(),
            ..PrivacyRules::default()
        }
    }

    #[test]
    fn missing_or_broken_rules_drop_titles() {
        let mut c = Collector::default(); // consent=false, rules=None
        assert!(
            c.accept(fg("code.exe", "secret", 1_000)).is_none(),
            "no consent: nothing queued"
        );
        assert_eq!(c.pending(), 0);
        assert_eq!(c.set_consent(true), Hook::Start);
        assert_eq!(c.accept(fg("code.exe", "secret", 2_000)), Some(1));
        let e = &c.drain(10).events[0];
        assert_eq!(e.title, None, "rules missing: fail-closed");
        assert_eq!(
            (e.app.as_deref(), e.at_ms, e.last_input_ms),
            (Some("code.exe"), 2_000, 2_000)
        );

        c.configure(Some(&rules(&[], &["sec"])));
        c.accept(fg("code.exe", "secret", 3_000));
        assert_eq!(
            c.drain(10).events[1].title.as_deref(),
            Some("[redacted]ret"),
            "rules apply"
        );

        c.configure(Some(&rules(&[], &["(unclosed"])));
        c.accept(fg("code.exe", "secret", 4_000));
        let e = &c.drain(10).events[2];
        assert_eq!(
            (e.title.as_deref(), e.app.as_deref()),
            (None, Some("code.exe")),
            "broken = missing"
        );

        c.configure(Some(&rules(&[], &[])));
        c.configure(None);
        c.accept(fg("code.exe", "secret", 5_000));
        assert_eq!(c.drain(10).events[3].title, None, "rules withdrawn");

        let masked = PrivacyRules {
            mask_all_titles: true,
            ..PrivacyRules::default()
        };
        c.configure(Some(&masked));
        c.accept(fg("code.exe", "secret", 6_000));
        assert_eq!(
            c.drain(10).events[4].title,
            None,
            "an emptied title is no title"
        );
        c.ack(c.drain(10).upto);
        assert_eq!(c.pending(), 0);
    }

    #[test]
    fn excluded_process_never_reaches_the_buffer() {
        let mut c = Collector::default();
        c.set_consent(true);
        c.configure(Some(&rules(&["KeePass.exe"], &[])));
        c.accept(fg("code.exe", "notes", 1_000));
        // Its foreground, title changes and heartbeats leave no name or title,
        // only one boundary so the previous window's session ends here.
        c.accept(fg("keepass.EXE", "bank vault", 2_000));
        c.accept(fg("keepass.exe", "bank vault 2", 2_500));
        c.accept(obs(
            ObservationKind::Input,
            Some("keepass.exe"),
            Some("bank vault 2"),
            3_000,
        ));
        c.accept(fg("code.exe", "notes", 4_000));
        let batch = c.drain(10);
        let text = format!("{:?}", batch.events).to_lowercase();
        assert!(
            !text.contains("keepass") && !text.contains("bank"),
            "{text}"
        );
        let kinds: Vec<_> = batch
            .events
            .iter()
            .map(|e| (e.kind, e.at_ms, e.app.as_deref()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (ObservationKind::Foreground, 1_000, Some("code.exe")),
                (ObservationKind::Idle, 2_000, None),
                (ObservationKind::Foreground, 4_000, Some("code.exe")),
            ]
        );
        assert_eq!(batch.events[1].title, None);
        // A heartbeat without a window name while excluded stays out too, and
        // an observation without an app never carries a title.
        c.accept(fg("keepass.exe", "vault", 5_000));
        c.accept(obs(ObservationKind::Input, None, None, 6_000));
        c.accept(obs(ObservationKind::Idle, None, Some("stray title"), 7_000));
        let tail: Vec<_> = c.drain(10).events[3..]
            .iter()
            .map(|e| (e.kind, e.title.clone()))
            .collect();
        assert_eq!(
            tail,
            vec![(ObservationKind::Idle, None), (ObservationKind::Idle, None)]
        );
    }

    /// The daemon's mapping (`crates/activity` `service.rs` `apply`) on the
    /// ported sessionizer: what would be stored from these observations.
    fn daemon_sessions(events: &[Observation]) -> Vec<(String, i64, i64)> {
        use devbox_activity::model::sessionizer::Sessionizer;
        let mut s = Sessionizer::new();
        let mut open: Option<(String, String)> = None;
        let mut out = Vec::new();
        for e in events {
            let (at, last) = (e.at_ms, e.last_input_ms);
            match e.kind {
                ObservationKind::Foreground => {
                    let named = (
                        e.app.clone().unwrap_or_else(|| "unknown".into()),
                        e.title.clone().unwrap_or_default(),
                    );
                    out.extend(s.observe_with_input(named.0.clone(), named.1.clone(), at, last));
                    open = Some(named);
                }
                ObservationKind::Input => {
                    let named = e
                        .app
                        .clone()
                        .map(|a| (a, e.title.clone().unwrap_or_default()));
                    if let Some((a, t)) = open.clone().or(named) {
                        out.extend(s.observe_with_input(a, t, at, last));
                    }
                }
                ObservationKind::Idle => {
                    out.extend(s.close_at_last_input(last));
                    open = None;
                }
                ObservationKind::Pause => {
                    out.extend(s.finish_all(at));
                    open = None;
                }
            }
        }
        out.into_iter()
            .filter(|c| c.end_ts > c.start_ts)
            .map(|c| (c.app, c.start_ts, c.end_ts))
            .collect()
    }

    #[test]
    fn excluded_foreground_ends_the_previous_session_at_its_last_input() {
        let mut c = Collector::default();
        c.set_consent(true);
        c.configure(Some(&rules(&["LockApp.exe"], &[])));
        c.accept(fg("code.exe", "main.rs", 0));
        c.accept(obs(
            ObservationKind::Input,
            Some("code.exe"),
            Some("main.rs"),
            60_000,
        ));
        // The lock screen takes the foreground 120 s after the last input and
        // stays 180 s; then the user unlocks and returns.
        let mut lock = fg("LockApp.exe", "", 180_000);
        lock.last_input_ms = 60_000;
        c.accept(lock);
        let mut back = fg("code.exe", "main.rs", 360_000);
        back.last_input_ms = 359_000;
        c.accept(back);
        c.accept(obs(
            ObservationKind::Input,
            Some("code.exe"),
            Some("main.rs"),
            400_000,
        ));
        c.set_paused(true, 400_000);
        let events = c.drain(100).events;
        assert_eq!(events[2].last_input_ms, 60_000, "{events:?}");
        assert_eq!(
            daemon_sessions(&events),
            vec![
                ("code.exe".to_string(), 0, 60_000),
                ("code.exe".to_string(), 360_000, 400_000),
            ],
            "no idle tail, no locked time"
        );
    }

    #[test]
    fn consent_withdrawal_clears_the_queue_and_pause_emits_one_pause() {
        let mut c = Collector::default();
        assert_eq!(
            c.set_paused(true, 500),
            Hook::Keep,
            "no consent: no hook, no pause"
        );
        assert_eq!(c.pending(), 0);
        assert_eq!(c.set_paused(false, 600), Hook::Keep);
        assert_eq!(c.set_consent(true), Hook::Start);
        c.accept(fg("code.exe", "a", 1_000));
        assert_eq!(c.set_paused(true, 2_000), Hook::Stop);
        assert_eq!(c.set_paused(true, 2_500), Hook::Keep);
        assert!(c.consent() && c.paused() && !c.running());
        assert!(
            c.accept(fg("code.exe", "b", 3_000)).is_none(),
            "paused: dropped"
        );
        let batch = c.drain(10);
        let kinds: Vec<_> = batch.events.iter().map(|e| (e.kind, e.at_ms)).collect();
        assert_eq!(
            kinds,
            vec![
                (ObservationKind::Foreground, 1_000),
                (ObservationKind::Pause, 2_000)
            ]
        );
        assert_eq!(
            (batch.events[1].app.clone(), batch.events[1].last_input_ms),
            (None, 1_000)
        );

        assert_eq!(c.set_paused(false, 4_000), Hook::Start);
        c.accept(fg("code.exe", "c", 4_000));
        assert_eq!(c.set_consent(false), Hook::Stop);
        assert_eq!(c.pending(), 1, "withdrawal keeps only a nameless boundary");
        assert!(c.accept(fg("code.exe", "d", 5_000)).is_none());
        assert_eq!(c.set_consent(true), Hook::Start);
        // Numbering continues within the run: the daemon skips `seq <= upto`.
        assert_eq!(c.accept(fg("code.exe", "e", 6_000)), Some(5));

        let (restored, hook) = Collector::restore(true, true);
        assert_eq!(
            (hook, restored.pending(), restored.running()),
            (Hook::Keep, 0, false)
        );
        let (restored, hook) = Collector::restore(true, false);
        assert_eq!((hook, restored.running()), (Hook::Start, true));
        assert_eq!(Collector::restore(false, false).1, Hook::Keep);
    }

    #[test]
    fn failed_saves_keep_a_stop_and_never_start() {
        use std::sync::Mutex;
        let collector = Mutex::new(Collector::restore(true, false).0);
        lock(&collector).accept(fg("code.exe", "a", 1_000));
        let (mut tries, mut hooks) = (0, Vec::new());
        let result = change(
            &collector,
            Some(false),
            None,
            2_000,
            || {
                tries += 1;
                Err(std::io::Error::other("disk full"))
            },
            |h| hooks.push(h),
        );
        assert!(result.is_err());
        assert_eq!((tries, hooks.clone()), (SAVE_ATTEMPTS, vec![Hook::Stop]));
        let c = lock(&collector);
        assert!(
            !c.consent() && !c.running() && c.pending() == 1,
            "stopped with only a nameless boundary"
        );
        drop(c);

        // A later attempt succeeds: the withdrawal is what gets saved.
        let (mut tries, mut hooks) = (0, Vec::new());
        let ok = change(
            &collector,
            Some(false),
            None,
            3_000,
            || {
                tries += 1;
                if tries < 2 {
                    Err(std::io::Error::other("busy"))
                } else {
                    Ok(())
                }
            },
            |h| hooks.push(h),
        );
        assert!(ok.is_ok() && tries == 2 && hooks == vec![Hook::Keep]);

        // Consent or resume is applied only after a successful save.
        let (mut tries, mut hooks) = (0, Vec::new());
        let result = change(
            &collector,
            Some(true),
            Some(false),
            4_000,
            || {
                tries += 1;
                Err(std::io::Error::other("disk full"))
            },
            |h| hooks.push(h),
        );
        assert!(result.is_err() && tries == SAVE_ATTEMPTS && hooks.is_empty());
        assert!(!lock(&collector).running());
        let mut hooks = Vec::new();
        change(
            &collector,
            Some(true),
            None,
            5_000,
            || Ok(()),
            |h| hooks.push(h),
        )
        .unwrap();
        assert_eq!(hooks, vec![Hook::Start]);

        // Pausing stops before saving and stays paused on failure.
        let mut hooks = Vec::new();
        let result = change(
            &collector,
            None,
            Some(true),
            6_000,
            || Err(std::io::Error::other("x")),
            |h| hooks.push(h),
        );
        assert!(result.is_err() && hooks == vec![Hook::Stop] && lock(&collector).paused());
    }

    #[test]
    fn run_id_fits_the_daemon_charset() {
        let id = run_id(u128::MAX, u32::MAX, u64::MAX);
        assert!(!id.is_empty() && id.len() <= 64, "{id}");
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{id}"
        );
        assert_ne!(run_id(1, 2, 3), run_id(1, 2, 4));
    }

    #[test]
    fn legacy_knowledge_detects_any_suffixed_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mk = |name: &str| std::fs::create_dir_all(root.join(name)).unwrap();
        assert!(!legacy_knowledge(root), "empty");
        assert!(!legacy_knowledge(&root.join("missing")));
        mk("com.devbox.v08.knowledge.iabc");
        mk("com.devbox.v08.knowledge");
        std::fs::write(
            root.join("com.devbox.v08.knowledge/active-stores.json"),
            "{}",
        )
        .unwrap();
        mk("com.devbox.v08.knowledge.ixyz/active-stores.json");
        std::fs::write(root.join("com.devbox.v08.knowledge.i"), "").unwrap();
        assert!(
            !legacy_knowledge(root),
            "no file, no suffix, or a folder named like the file"
        );
        #[cfg(unix)]
        {
            let real = tempfile::tempdir().unwrap();
            std::fs::write(real.path().join("active-stores.json"), "{}").unwrap();
            std::os::unix::fs::symlink(real.path(), root.join("com.devbox.v08.knowledge.ilink"))
                .unwrap();
            mk("com.devbox.v08.knowledge.ifile");
            std::os::unix::fs::symlink(
                real.path().join("active-stores.json"),
                root.join("com.devbox.v08.knowledge.ifile/active-stores.json"),
            )
            .unwrap();
            assert!(!legacy_knowledge(root), "links are not followed");
        }
        std::fs::write(
            root.join("com.devbox.v08.knowledge.iabc/active-stores.json"),
            "{}",
        )
        .unwrap();
        assert!(legacy_knowledge(root), "one suffixed store is enough");
    }
}

#[cfg(test)]
#[path = "activity_consent_tests.rs"]
mod consent_tests;
