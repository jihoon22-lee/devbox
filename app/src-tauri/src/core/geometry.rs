/// Physical client sizes from monitor work area, DPI and physical non-client extents.
pub fn initial_size(width: u32, height: u32, scale: f64, frame: (u32, u32)) -> (u32, u32) {
    let dimension = |pixels: u32, decoration: u32, min: f64, max: f64| {
        (((f64::from(pixels) / scale * 0.8).clamp(min, max) * scale).round() as u32)
            .min(pixels.saturating_sub(decoration))
    };
    (
        dimension(width, frame.0, 1180.0, 1600.0),
        dimension(height, frame.1, 760.0, 1000.0),
    )
}

pub fn minimum_size(width: u32, height: u32, scale: f64, frame: (u32, u32)) -> (u32, u32) {
    // A small work area takes precedence over the normal 960 × 600 minimum.
    (
        width
            .saturating_sub(frame.0)
            .min((960.0 * scale).round() as u32),
        height
            .saturating_sub(frame.1)
            .min((600.0 * scale).round() as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorated_client_fits_work_area_at_fractional_dpi() {
        let area = (900, 550);
        let frame = (24, 58);
        let client = initial_size(area.0, area.1, 1.5, frame);
        assert_eq!(client, (876, 492));
        assert_eq!(minimum_size(area.0, area.1, 1.5, frame), client);
        assert_eq!((client.0 + frame.0, client.1 + frame.1), area);
        // Decorations only constrain available space; ordinary desired size is unchanged.
        assert_eq!(initial_size(2400, 1500, 1.5, frame), (1920, 1200));
        assert_eq!(minimum_size(2400, 1500, 1.5, frame), (1440, 900));
    }

    #[test]
    fn uses_eighty_percent_and_logical_limits_at_each_dpi() {
        assert_eq!(initial_size(1920, 1080, 1.0, (0, 0)), (1536, 864));
        assert_eq!(initial_size(3840, 2160, 2.0, (0, 0)), (3072, 1728));
        assert_eq!(initial_size(4000, 3000, 1.0, (0, 0)), (1600, 1000));
        assert_eq!(initial_size(1280, 800, 1.0, (0, 0)), (1180, 760));
    }

    #[test]
    fn never_exceeds_small_work_area_even_at_fractional_dpi() {
        assert_eq!(initial_size(900, 550, 1.0, (0, 0)), (900, 550));
        assert_eq!(initial_size(1500, 900, 1.5, (0, 0)), (1500, 900));
        assert_eq!(minimum_size(900, 550, 1.0, (0, 0)), (900, 550));
        assert_eq!(minimum_size(1920, 1080, 1.5, (0, 0)), (1440, 900));
    }
}
