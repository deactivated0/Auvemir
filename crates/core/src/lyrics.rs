#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub ms: u64,
    pub text: String,
}
pub fn parse_lrc(text: &str) -> Vec<Line> {
    let mut result = Vec::new();
    for raw in text.lines() {
        let mut line = raw;
        let mut times = Vec::new();
        while let Some(rest) = line.strip_prefix('[') {
            let Some((stamp, remaining)) = rest.split_once(']') else {
                break;
            };
            if let Some((min, sec)) = stamp.split_once(':')
                && let (Ok(m), Ok(s)) = (min.parse::<u64>(), sec.parse::<f64>())
                && s.is_finite()
                && (0.0..60.0).contains(&s)
            {
                times.push(
                    m.saturating_mul(60_000)
                        .saturating_add((s * 1000.0).round() as u64),
                );
            }
            line = remaining;
        }
        for ms in times {
            result.push(Line {
                ms,
                text: line.to_owned(),
            });
        }
    }
    result.sort_by_key(|l| l.ms);
    result
}
pub fn active_line(lines: &[Line], ms: u64) -> Option<usize> {
    lines.partition_point(|l| l.ms <= ms).checked_sub(1)
}

/// Undo the display offset when seeking to a timed line, without signed overflow.
pub fn seek_position(line_ms: u64, offset_ms: i64, duration_ms: u64) -> u64 {
    let position =
        (i128::from(line_ms) - i128::from(offset_ms)).clamp(0, i128::from(u64::MAX)) as u64;
    if duration_ms > 0 {
        position.min(duration_ms)
    } else {
        position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeking_undoes_offset_and_clamps_at_track_boundaries() {
        assert_eq!(seek_position(12_500, 500, 60_000), 12_000);
        assert_eq!(seek_position(12_500, -500, 60_000), 13_000);
        assert_eq!(seek_position(500, 1_000, 60_000), 0);
        assert_eq!(seek_position(59_500, -1_000, 60_000), 60_000);
        assert_eq!(seek_position(u64::MAX, i64::MIN, 0), u64::MAX);
        assert_eq!(seek_position(0, i64::MAX, 0), 0);
    }
}
