use window_state::{
    decode_state, encode_state, restore_from_bytes, MonitorId, MonitorInfo, RestoreConfig,
    RestoreSource, WindowBounds, WindowState,
};

const V1_FIXTURE: &[u8] = include_bytes!("fixtures/window-state-v1.json");

fn id(value: &str) -> MonitorId {
    MonitorId::new(value).expect("valid monitor id")
}

fn area(x: i32, y: i32, width: u32, height: u32) -> WindowBounds {
    WindowBounds::new(x, y, width, height)
}

fn monitor(name: &str, work_area: WindowBounds, scale_factor: f64, primary: bool) -> MonitorInfo {
    MonitorInfo::new(id(name), work_area, scale_factor, primary).expect("valid monitor")
}

#[test]
fn checked_in_fixture_is_the_public_round_trip_contract() {
    let state = decode_state(V1_FIXTURE).expect("fixture should parse");
    assert_eq!(state.schema_version, 1);
    assert_eq!(state.monitor_id, id("DISPLAY1"));
    assert_eq!(state.bounds, area(100, 140, 1_200, 800));
    assert_eq!(state.monitor_work_area, area(0, 0, 1_920, 1_040));
    assert_eq!(state.scale_factor, 1.0);
    assert!(!state.maximized);
    assert_eq!(
        decode_state(&encode_state(&state).expect("encode")).unwrap(),
        state
    );
}

#[test]
fn unknown_fields_and_future_schema_are_rejected_as_corruption() {
    let mut future = serde_json::from_slice::<serde_json::Value>(V1_FIXTURE).unwrap();
    future["schemaVersion"] = serde_json::json!(2);
    assert_eq!(
        decode_state(future.to_string().as_bytes()),
        Err(window_state::WindowStateError::UnsupportedSchema)
    );

    let mut unknown = serde_json::from_slice::<serde_json::Value>(V1_FIXTURE).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(decode_state(unknown.to_string().as_bytes()).is_err());
}

#[test]
fn resolution_shrink_clamps_the_titlebar_and_no_primary_uses_first_valid_monitor() {
    let saved = WindowState::new(
        id("DISPLAY1"),
        area(3_200, 1_800, 1_200, 800),
        area(0, 0, 3_840, 2_160),
        1.0,
        false,
    )
    .expect("valid state");
    let monitors = [
        monitor("DISPLAY2", area(-1_280, 0, 1_280, 720), 1.0, false),
        monitor("DISPLAY3", area(0, 0, 1_920, 1_040), 1.0, false),
    ];

    let result = window_state::restore_window(Some(&saved), &monitors, RestoreConfig::default());

    assert_eq!(result.source, RestoreSource::MonitorFallback);
    assert_eq!(result.state.monitor_id, Some(id("DISPLAY2")));
    assert_eq!(result.state.bounds, area(-64, 696, 1_200, 800));
}

#[test]
fn corrupt_state_falls_back_to_default_and_never_returns_input() {
    let result = restore_from_bytes(
        Some(br#"{"schemaVersion":1,"monitorId":"credential-like","unknown":true}"#),
        &[monitor("DISPLAY1", area(0, 0, 1_920, 1_040), 1.0, true)],
        RestoreConfig::default(),
    );
    assert_eq!(result.source, RestoreSource::CorruptState);
    assert_eq!(result.state.bounds, area(448, 136, 1_024, 768));
}

#[test]
fn removed_monitor_falls_back_to_primary_even_when_list_order_changes() {
    let saved = WindowState::new(
        id("DISPLAY2"),
        area(2_120, 120, 1_000, 700),
        area(1_920, 0, 1_920, 1_040),
        1.0,
        true,
    )
    .expect("valid state");
    let monitors = [
        monitor("DISPLAY3", area(-1_280, 0, 1_280, 1_024), 1.25, false),
        monitor("DISPLAY1", area(0, 0, 1_280, 720), 1.0, true),
    ];
    let result = window_state::restore_window(Some(&saved), &monitors, RestoreConfig::default());
    assert_eq!(result.source, RestoreSource::MonitorFallback);
    assert_eq!(result.state.monitor_id, Some(id("DISPLAY1")));
    assert!(result.state.maximized);
    assert_eq!(result.state.bounds, area(200, 120, 1_000, 700));
}
