pub fn normalize_output_volume_percent(volume: f64) -> f64 {
    if !volume.is_finite() {
        return 1.0;
    }
    (volume / 100.0).clamp(0.0, 1.0)
}

pub fn effective_output_volume(source_volume: f64, output_volume: f64) -> f64 {
    source_volume.clamp(0.0, 1.0) * output_volume.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{effective_output_volume, normalize_output_volume_percent};

    #[test]
    fn output_volume_percent_normalizes_and_scales_source_volume() {
        assert_eq!(normalize_output_volume_percent(100.0), 1.0);
        assert_eq!(normalize_output_volume_percent(35.0), 0.35);
        assert_eq!(normalize_output_volume_percent(-20.0), 0.0);
        assert_eq!(normalize_output_volume_percent(250.0), 1.0);
        assert_eq!(normalize_output_volume_percent(f64::NAN), 1.0);

        assert!((effective_output_volume(0.8, 0.5) - 0.4).abs() < f64::EPSILON);
        assert_eq!(effective_output_volume(1.2, 0.25), 0.25);
        assert_eq!(effective_output_volume(0.75, -1.0), 0.0);
    }
}
