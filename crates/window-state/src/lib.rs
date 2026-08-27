//! Portable persistent state for ordinary application windows.
//!
//! The crate owns the versioned JSON contract and the pure geometry needed to
//! restore a window after a monitor or DPI change.  It deliberately does not
//! read or write files, inspect operating-system monitors, or depend on Tauri;
//! an app supplies the current monitor snapshot and owns its atomic storage
//! path.  This keeps the contract testable on Linux and leaves platform
//! handles in the app wiring that consumes it.
//!
//! Persisted bounds and monitor work areas are physical pixels.  The saved
//! `scale_factor` is used to scale the saved bounds into the current monitor's
//! physical-pixel coordinate space.  Keeping the saved work area in the
//! document makes the transformation well-defined even when a monitor's
//! resolution or virtual-desktop origin changes.

use serde::de::Error as DeError;
use serde::ser::Error as SerError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Version of the persisted window-state document.
pub const SCHEMA_VERSION: u32 = 1;
/// Descriptive alias for consumers that name the value by its domain.
pub const WINDOW_STATE_SCHEMA_VERSION: u32 = SCHEMA_VERSION;

/// Maximum encoded document size accepted by [`decode_state`].
pub const MAX_STATE_BYTES: usize = 16 * 1024;
/// Descriptive alias for the persistence boundary.
pub const MAX_WINDOW_STATE_BYTES: usize = MAX_STATE_BYTES;

/// Maximum UTF-8 length of an opaque monitor identity.
pub const MAX_MONITOR_ID_BYTES: usize = 256;

/// Safe scale-factor range for a monitor snapshot.
pub const MIN_SCALE_FACTOR: f64 = 0.5;
pub const MAX_SCALE_FACTOR: f64 = 8.0;

/// Coordinate and dimension bounds protect arithmetic and reject implausible
/// or corrupted desktop geometry before it reaches a native window API.
pub const MAX_COORDINATE: i32 = 1_000_000_000;
pub const MAX_DIMENSION: u32 = 131_072;

/// Defaults shared by apps unless an app has a more suitable product size.
pub const DEFAULT_WINDOW_WIDTH: u32 = 1_024;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 768;
pub const DEFAULT_VISIBLE_TITLEBAR_WIDTH: u32 = 64;
pub const DEFAULT_VISIBLE_TITLEBAR_HEIGHT: u32 = 24;
pub const DEFAULT_TITLEBAR_HEIGHT: u32 = 32;
pub const DEFAULT_SCALE_FACTOR: f64 = 1.0;

const MIN_WINDOW_WIDTH: u32 = 1;
const MIN_WINDOW_HEIGHT: u32 = 1;

/// Errors intentionally contain no input strings or paths.  Callers can show
/// or log them without echoing persisted data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowStateError {
    InputTooLarge,
    InvalidJson,
    UnsupportedSchema,
    InvalidMonitorId,
    InvalidBounds,
    InvalidScaleFactor,
    Serialization,
}

impl fmt::Display for WindowStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InputTooLarge => "window state exceeds the size limit",
            Self::InvalidJson => "window state JSON is invalid",
            Self::UnsupportedSchema => "window state schema is unsupported",
            Self::InvalidMonitorId => "window state monitor identity is invalid",
            Self::InvalidBounds => "window state bounds are invalid",
            Self::InvalidScaleFactor => "window state scale factor is invalid",
            Self::Serialization => "window state could not be serialized",
        })
    }
}

impl std::error::Error for WindowStateError {}

/// Opaque, bounded identity supplied by the platform monitor adapter.
///
/// The value is not interpreted as a path or command.  Printable Unicode is
/// retained verbatim so Windows display names such as `\\\\.\\DISPLAY1` can
/// be used without putting Windows-only code in this crate.  Control
/// characters, including NUL, are rejected.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonitorId(String);

impl MonitorId {
    /// Creates a monitor identity after applying the portable safety bounds.
    pub fn new(value: impl Into<String>) -> Result<Self, WindowStateError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_MONITOR_ID_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(WindowStateError::InvalidMonitorId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl TryFrom<String> for MonitorId {
    type Error = WindowStateError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for MonitorId {
    type Error = WindowStateError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for MonitorId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for MonitorId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(|_| D::Error::custom("invalid monitor identity"))
    }
}

/// A physical-pixel rectangle.  Window bounds and monitor work areas share
/// this wire shape, but are validated with different size rules.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowBounds {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn validate_coordinate(&self) -> Result<(), WindowStateError> {
        if self.x.abs_diff(0) > MAX_COORDINATE.unsigned_abs()
            || self.y.abs_diff(0) > MAX_COORDINATE.unsigned_abs()
        {
            return Err(WindowStateError::InvalidBounds);
        }
        Ok(())
    }

    fn validate_work_area(&self) -> Result<(), WindowStateError> {
        self.validate_coordinate()?;
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
        {
            return Err(WindowStateError::InvalidBounds);
        }
        Ok(())
    }

    fn validate_window(&self) -> Result<(), WindowStateError> {
        self.validate_work_area()?;
        if self.width < MIN_WINDOW_WIDTH || self.height < MIN_WINDOW_HEIGHT {
            return Err(WindowStateError::InvalidBounds);
        }
        Ok(())
    }
}

/// Dimensions used by [`RestoreConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// Current monitor facts supplied by an app's native adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub work_area: WindowBounds,
    pub scale_factor: f64,
    pub primary: bool,
}

impl MonitorInfo {
    pub fn new(
        id: MonitorId,
        work_area: WindowBounds,
        scale_factor: f64,
        primary: bool,
    ) -> Result<Self, WindowStateError> {
        let monitor = Self {
            id,
            work_area,
            scale_factor,
            primary,
        };
        monitor.validate()?;
        Ok(monitor)
    }

    fn validate(&self) -> Result<(), WindowStateError> {
        self.work_area.validate_work_area()?;
        validate_scale_factor(self.scale_factor)
    }
}

/// Versioned data written by an app's own atomic persistence layer.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowState {
    pub schema_version: u32,
    pub bounds: WindowBounds,
    pub monitor_id: MonitorId,
    pub monitor_work_area: WindowBounds,
    pub scale_factor: f64,
    pub maximized: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowStateWire<'a> {
    schema_version: u32,
    bounds: WindowBounds,
    monitor_id: &'a MonitorId,
    monitor_work_area: WindowBounds,
    scale_factor: f64,
    maximized: bool,
}

impl Serialize for WindowState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.validate()
            .map_err(|_| S::Error::custom("invalid window state"))?;
        WindowStateWire {
            schema_version: self.schema_version,
            bounds: self.bounds,
            monitor_id: &self.monitor_id,
            monitor_work_area: self.monitor_work_area,
            scale_factor: self.scale_factor,
            maximized: self.maximized,
        }
        .serialize(serializer)
    }
}

impl WindowState {
    pub fn new(
        monitor_id: MonitorId,
        bounds: WindowBounds,
        monitor_work_area: WindowBounds,
        scale_factor: f64,
        maximized: bool,
    ) -> Result<Self, WindowStateError> {
        let state = Self {
            schema_version: SCHEMA_VERSION,
            bounds,
            monitor_id,
            monitor_work_area,
            scale_factor,
            maximized,
        };
        state.validate()?;
        Ok(state)
    }

    /// Captures the platform-independent portion of a current window.
    pub fn capture(
        bounds: WindowBounds,
        monitor: &MonitorInfo,
        maximized: bool,
    ) -> Result<Self, WindowStateError> {
        monitor.validate()?;
        Self::new(
            monitor.id.clone(),
            bounds,
            monitor.work_area,
            monitor.scale_factor,
            maximized,
        )
    }

    pub fn validate(&self) -> Result<(), WindowStateError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(WindowStateError::UnsupportedSchema);
        }
        // MonitorId's constructor and deserializer enforce this already, but
        // keeping the check here protects public struct literals as well.
        MonitorId::new(self.monitor_id.as_str())?;
        self.bounds.validate_window()?;
        self.monitor_work_area.validate_work_area()?;
        validate_scale_factor(self.scale_factor)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, WindowStateError> {
        encode_state(self)
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self, WindowStateError> {
        decode_state(input)
    }
}

/// Product-level defaults and the minimum part of the title bar that must
/// remain reachable after restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreConfig {
    pub default_size: WindowSize,
    pub min_visible_titlebar: WindowSize,
    pub titlebar_height: u32,
}

impl Default for RestoreConfig {
    fn default() -> Self {
        Self {
            default_size: WindowSize::new(DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT),
            min_visible_titlebar: WindowSize::new(
                DEFAULT_VISIBLE_TITLEBAR_WIDTH,
                DEFAULT_VISIBLE_TITLEBAR_HEIGHT,
            ),
            titlebar_height: DEFAULT_TITLEBAR_HEIGHT,
        }
    }
}

/// The safe native-window input produced by [`restore_window`].  A missing
/// monitor list is represented by `monitor_id == None`; the app can defer
/// applying the position until its platform adapter has a monitor snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoredWindowState {
    pub bounds: WindowBounds,
    pub monitor_id: Option<MonitorId>,
    pub scale_factor: f64,
    pub maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreSource {
    Default,
    Persisted,
    CorruptState,
    MonitorFallback,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestoreResult {
    pub state: RestoredWindowState,
    pub source: RestoreSource,
}

/// Encodes a validated document without exposing a file-system side effect.
pub fn encode_state(state: &WindowState) -> Result<Vec<u8>, WindowStateError> {
    state.validate()?;
    let bytes = serde_json::to_vec(state).map_err(|_| WindowStateError::Serialization)?;
    if bytes.len() > MAX_STATE_BYTES {
        return Err(WindowStateError::InputTooLarge);
    }
    Ok(bytes)
}

/// Decodes a bounded, strict-schema document.
pub fn decode_state(input: &[u8]) -> Result<WindowState, WindowStateError> {
    if input.len() > MAX_STATE_BYTES {
        return Err(WindowStateError::InputTooLarge);
    }
    // `serde_json` does not expose enough structured detail to safely
    // distinguish every shape failure.  All malformed/unknown-field input has
    // the same non-reflective public error.
    let state: WindowState =
        serde_json::from_slice(input).map_err(|_| WindowStateError::InvalidJson)?;
    state.validate()?;
    Ok(state)
}

/// Restores from optional persisted bytes.  Missing bytes use ordinary
/// defaults; malformed, oversized, or unsupported bytes use the same safe
/// defaults and report [`RestoreSource::CorruptState`] without returning raw
/// parser details.  A valid state whose monitor disappeared uses the primary
/// monitor, then the first valid monitor, and reports
/// [`RestoreSource::MonitorFallback`].
pub fn restore_from_bytes(
    input: Option<&[u8]>,
    monitors: &[MonitorInfo],
    config: RestoreConfig,
) -> RestoreResult {
    match input {
        None => restore_window(None, monitors, config),
        Some(input) => match decode_state(input) {
            Ok(state) => restore_window(Some(&state), monitors, config),
            Err(_) => {
                let mut result = restore_window(None, monitors, config);
                result.source = RestoreSource::CorruptState;
                result
            }
        },
    }
}

/// Restores a validated (or programmatically supplied) state onto the current
/// monitor list.  Invalid public struct literals are treated as corruption so
/// native callers never receive unchecked geometry.
pub fn restore_window(
    saved: Option<&WindowState>,
    monitors: &[MonitorInfo],
    config: RestoreConfig,
) -> RestoreResult {
    let valid_monitors = monitors
        .iter()
        .filter(|monitor| monitor.validate().is_ok())
        .collect::<Vec<_>>();
    let valid_saved = saved.filter(|state| state.validate().is_ok());
    let state_was_corrupt = saved.is_some() && valid_saved.is_none();

    let Some(monitor) = choose_monitor(valid_saved, &valid_monitors) else {
        let fallback = default_bounds_without_monitor(config);
        return RestoreResult {
            state: RestoredWindowState {
                bounds: fallback,
                monitor_id: None,
                scale_factor: valid_saved
                    .map(|state| state.scale_factor)
                    .unwrap_or(DEFAULT_SCALE_FACTOR),
                maximized: valid_saved.map(|state| state.maximized).unwrap_or(false),
            },
            source: if state_was_corrupt {
                RestoreSource::CorruptState
            } else if valid_saved.is_some() {
                RestoreSource::MonitorFallback
            } else {
                RestoreSource::Default
            },
        };
    };

    let monitor_matches_saved = valid_saved
        .map(|state| state.monitor_id == monitor.id)
        .unwrap_or(false);
    let source = if state_was_corrupt {
        RestoreSource::CorruptState
    } else if valid_saved.is_none() {
        RestoreSource::Default
    } else if monitor_matches_saved {
        RestoreSource::Persisted
    } else {
        RestoreSource::MonitorFallback
    };

    let (bounds, maximized) = match valid_saved {
        Some(state) => (
            clamp_visible_titlebar(
                transform_bounds(
                    state.bounds,
                    state.monitor_work_area,
                    monitor.work_area,
                    monitor.scale_factor / state.scale_factor,
                ),
                monitor.work_area,
                config,
            ),
            state.maximized,
        ),
        None => (default_bounds(monitor.work_area, config), false),
    };

    RestoreResult {
        state: RestoredWindowState {
            bounds,
            monitor_id: Some(monitor.id.clone()),
            scale_factor: monitor.scale_factor,
            maximized,
        },
        source,
    }
}

fn validate_scale_factor(value: f64) -> Result<(), WindowStateError> {
    if !value.is_finite() || !(MIN_SCALE_FACTOR..=MAX_SCALE_FACTOR).contains(&value) {
        return Err(WindowStateError::InvalidScaleFactor);
    }
    Ok(())
}

fn choose_monitor<'a>(
    saved: Option<&WindowState>,
    monitors: &[&'a MonitorInfo],
) -> Option<&'a MonitorInfo> {
    if let Some(saved) = saved {
        if let Some(monitor) = monitors
            .iter()
            .find(|monitor| monitor.id == saved.monitor_id)
        {
            return Some(*monitor);
        }
    }
    monitors
        .iter()
        .find(|monitor| monitor.primary)
        .copied()
        .or_else(|| monitors.first().copied())
}

fn default_bounds_without_monitor(config: RestoreConfig) -> WindowBounds {
    let size = normalized_default_size(config.default_size);
    WindowBounds::new(0, 0, size.width, size.height)
}

fn default_bounds(area: WindowBounds, config: RestoreConfig) -> WindowBounds {
    let configured = normalized_default_size(config.default_size);
    let width = configured.width.min(area.width);
    let height = configured.height.min(area.height);
    let x = area.x as i64 + (area.width as i64 - width as i64) / 2;
    let y = area.y as i64 + (area.height as i64 - height as i64) / 2;
    WindowBounds::new(
        saturating_coordinate(x),
        saturating_coordinate(y),
        width,
        height,
    )
}

fn normalized_default_size(size: WindowSize) -> WindowSize {
    WindowSize::new(
        normalize_dimension(size.width, DEFAULT_WINDOW_WIDTH),
        normalize_dimension(size.height, DEFAULT_WINDOW_HEIGHT),
    )
}

fn normalize_dimension(value: u32, fallback: u32) -> u32 {
    if value == 0 {
        fallback
    } else {
        value.min(MAX_DIMENSION)
    }
}

fn transform_bounds(
    bounds: WindowBounds,
    saved_area: WindowBounds,
    current_area: WindowBounds,
    scale_ratio: f64,
) -> WindowBounds {
    let relative_x = bounds.x as i64 - saved_area.x as i64;
    let relative_y = bounds.y as i64 - saved_area.y as i64;
    let x = current_area.x as i64 + scale_round(relative_x, scale_ratio);
    let y = current_area.y as i64 + scale_round(relative_y, scale_ratio);
    let width = scaled_dimension(bounds.width, scale_ratio, MIN_WINDOW_WIDTH);
    let height = scaled_dimension(bounds.height, scale_ratio, MIN_WINDOW_HEIGHT);
    WindowBounds::new(
        saturating_coordinate(x),
        saturating_coordinate(y),
        width,
        height,
    )
}

fn scale_round(value: i64, ratio: f64) -> i64 {
    let scaled = value as f64 * ratio;
    if scaled.is_finite() {
        scaled.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0
    }
}

fn scaled_dimension(value: u32, ratio: f64, minimum: u32) -> u32 {
    let scaled = scale_round(value as i64, ratio);
    scaled.clamp(minimum.max(1) as i64, MAX_DIMENSION as i64) as u32
}

fn clamp_visible_titlebar(
    bounds: WindowBounds,
    area: WindowBounds,
    config: RestoreConfig,
) -> WindowBounds {
    let width = bounds.width.clamp(MIN_WINDOW_WIDTH, MAX_DIMENSION);
    let height = bounds.height.clamp(MIN_WINDOW_HEIGHT, MAX_DIMENSION);
    let visible_width = config
        .min_visible_titlebar
        .width
        .max(1)
        .min(width)
        .min(area.width);
    let titlebar_height = config.titlebar_height.max(1).min(height);
    let visible_height = config
        .min_visible_titlebar
        .height
        .max(1)
        .min(titlebar_height)
        .min(area.height);

    let min_x = area.x as i64 - width as i64 + visible_width as i64;
    let max_x = area.x as i64 + area.width as i64 - visible_width as i64;
    let min_y = area.y as i64 - titlebar_height as i64 + visible_height as i64;
    let max_y = area.y as i64 + area.height as i64 - visible_height as i64;

    WindowBounds::new(
        saturating_coordinate(clamp_i64(bounds.x as i64, min_x, max_x)),
        saturating_coordinate(clamp_i64(bounds.y as i64, min_y, max_y)),
        width,
        height,
    )
}

fn clamp_i64(value: i64, minimum: i64, maximum: i64) -> i64 {
    value.clamp(minimum.min(maximum), minimum.max(maximum))
}

fn saturating_coordinate(value: i64) -> i32 {
    value.clamp(-MAX_COORDINATE as i64, MAX_COORDINATE as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> MonitorId {
        MonitorId::new(value).expect("valid monitor id")
    }

    fn area(x: i32, y: i32, width: u32, height: u32) -> WindowBounds {
        WindowBounds::new(x, y, width, height)
    }

    fn monitor(
        name: &str,
        work_area: WindowBounds,
        scale_factor: f64,
        primary: bool,
    ) -> MonitorInfo {
        MonitorInfo::new(id(name), work_area, scale_factor, primary).expect("valid monitor")
    }

    fn saved(
        monitor_id: &str,
        bounds: WindowBounds,
        monitor_work_area: WindowBounds,
        scale_factor: f64,
        maximized: bool,
    ) -> WindowState {
        WindowState::new(
            id(monitor_id),
            bounds,
            monitor_work_area,
            scale_factor,
            maximized,
        )
        .expect("valid state")
    }

    #[test]
    fn round_trip_is_deterministic_and_strict() {
        let state = saved(
            "DISPLAY1",
            area(100, 140, 1_200, 800),
            area(0, 0, 1_920, 1_040),
            1.0,
            true,
        );
        let first = encode_state(&state).expect("encode");
        let second = encode_state(&state).expect("encode");
        assert_eq!(first, second);
        assert_eq!(decode_state(&first).expect("decode"), state);
    }

    #[test]
    fn rejects_oversized_corrupt_unknown_and_unsupported_documents() {
        assert_eq!(
            decode_state(&vec![b'x'; MAX_STATE_BYTES + 1]),
            Err(WindowStateError::InputTooLarge)
        );
        assert_eq!(
            decode_state(br#"{"schemaVersion":1,"unexpected":true}"#),
            Err(WindowStateError::InvalidJson)
        );
        assert_eq!(
            decode_state(br#"{"schemaVersion":2}"#),
            Err(WindowStateError::InvalidJson)
        );
    }

    #[test]
    fn rejects_invalid_scale_monitor_identity_and_geometry() {
        assert_eq!(
            MonitorId::new("bad\nmonitor"),
            Err(WindowStateError::InvalidMonitorId)
        );
        assert_eq!(
            MonitorInfo::new(id("DISPLAY1"), area(0, 0, 1_920, 1_040), f64::NAN, true),
            Err(WindowStateError::InvalidScaleFactor)
        );
        assert_eq!(
            WindowState::new(
                id("DISPLAY1"),
                area(0, 0, 0, 80),
                area(0, 0, 1_920, 1_040),
                1.0,
                false,
            ),
            Err(WindowStateError::InvalidBounds)
        );
    }

    #[test]
    fn direct_serialization_cannot_bypass_state_validation() {
        let invalid = WindowState {
            schema_version: SCHEMA_VERSION,
            bounds: area(0, 0, 0, 400),
            monitor_id: id("DISPLAY1"),
            monitor_work_area: area(0, 0, 1_920, 1_040),
            scale_factor: 1.0,
            maximized: false,
        };
        assert!(serde_json::to_vec(&invalid).is_err());
    }

    #[test]
    fn dpi_change_scales_from_saved_work_area_and_preserves_maximized() {
        let state = saved(
            "DISPLAY1",
            area(100, 100, 1_000, 700),
            area(0, 0, 1_920, 1_040),
            1.0,
            true,
        );
        let current = monitor("DISPLAY1", area(0, 0, 1_920, 1_040), 2.0, true);
        let result = restore_window(Some(&state), &[current], RestoreConfig::default());
        assert_eq!(result.source, RestoreSource::Persisted);
        assert_eq!(result.state.monitor_id, Some(id("DISPLAY1")));
        assert_eq!(result.state.scale_factor, 2.0);
        assert_eq!(result.state.bounds, area(200, 200, 2_000, 1_400));
        assert!(result.state.maximized);
    }

    #[test]
    fn removed_monitor_uses_primary_and_keeps_relative_position() {
        let state = saved(
            "DISPLAY2",
            area(2_120, 120, 1_000, 700),
            area(1_920, 0, 1_920, 1_040),
            1.0,
            false,
        );
        let current = monitor("DISPLAY1", area(0, 0, 1_280, 720), 1.0, true);
        let result = restore_window(Some(&state), &[current], RestoreConfig::default());
        assert_eq!(result.source, RestoreSource::MonitorFallback);
        assert_eq!(result.state.monitor_id, Some(id("DISPLAY1")));
        assert_eq!(result.state.bounds, area(200, 120, 1_000, 700));
    }

    #[test]
    fn titlebar_clamp_leaves_a_reachable_top_strip() {
        let state = saved(
            "DISPLAY1",
            area(-5_000, -5_000, 1_000, 800),
            area(0, 0, 1_920, 1_040),
            1.0,
            false,
        );
        let current = monitor("DISPLAY1", area(0, 0, 1_920, 1_040), 1.0, true);
        let result = restore_window(Some(&state), &[current], RestoreConfig::default());
        assert_eq!(result.state.bounds, area(-936, -8, 1_000, 800));

        let state = saved(
            "DISPLAY1",
            area(5_000, 5_000, 1_000, 800),
            area(0, 0, 1_920, 1_040),
            1.0,
            false,
        );
        let result = restore_window(
            Some(&state),
            &[monitor("DISPLAY1", area(0, 0, 1_920, 1_040), 1.0, true)],
            RestoreConfig::default(),
        );
        assert_eq!(result.state.bounds, area(1_856, 1_016, 1_000, 800));
    }

    #[test]
    fn corrupt_bytes_fall_back_without_reflecting_input() {
        let result = restore_from_bytes(
            Some(br#"{"schemaVersion":1,"monitorId":"secret","bad":true}"#),
            &[monitor("DISPLAY1", area(0, 0, 1_920, 1_040), 1.0, true)],
            RestoreConfig::default(),
        );
        assert_eq!(result.source, RestoreSource::CorruptState);
        assert_eq!(result.state.bounds, area(448, 136, 1_024, 768));
        assert_eq!(
            WindowStateError::InvalidJson.to_string(),
            "window state JSON is invalid"
        );
    }

    #[test]
    fn default_size_is_centered_and_empty_monitor_list_is_safe() {
        let current = monitor("DISPLAY1", area(-1_280, 0, 1_280, 1_024), 1.25, true);
        let result = restore_window(None, &[current], RestoreConfig::default());
        assert_eq!(result.source, RestoreSource::Default);
        assert_eq!(result.state.bounds, area(-1_152, 128, 1_024, 768));
        assert_eq!(result.state.scale_factor, 1.25);

        let result = restore_window(None, &[], RestoreConfig::default());
        assert_eq!(result.state.monitor_id, None);
        assert_eq!(result.state.bounds, area(0, 0, 1_024, 768));
    }
}
