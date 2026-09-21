use crate::{Result, error::invalid, mapping::scale};
use serde::{Deserialize, Serialize};

/// Half-open frames per channel (not interleaved sample offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interval {
    pub start_frame: u64,
    pub end_frame: u64,
}
impl Interval {
    pub fn new(start_frame: u64, end_frame: u64) -> Self {
        Self {
            start_frame,
            end_frame,
        }
    }
    pub fn len(self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }
    pub fn is_empty(self) -> bool {
        self.start_frame >= self.end_frame
    }
}
pub fn normalize(mut intervals: Vec<Interval>, total: u64) -> Vec<Interval> {
    intervals.retain(|i| !i.is_empty());
    intervals.sort_unstable_by_key(|i| i.start_frame);
    let mut result: Vec<Interval> = Vec::new();
    for mut interval in intervals {
        interval.start_frame = interval.start_frame.min(total);
        interval.end_frame = interval.end_frame.min(total);
        if interval.is_empty() {
            continue;
        }
        if let Some(last) = result.last_mut()
            && interval.start_frame <= last.end_frame
        {
            last.end_frame = last.end_frame.max(interval.end_frame);
        } else {
            result.push(interval);
        }
    }
    result
}
pub fn from_analysis(intervals: &[Interval], rate: u32, total: u64) -> Result<Vec<Interval>> {
    let raw = normalize(intervals.to_vec(), u64::MAX);
    let mapped: Result<Vec<_>> = raw
        .into_iter()
        .map(|i| {
            Ok(Interval::new(
                scale(i.start_frame, rate, 16000, false)?,
                scale(i.end_frame, rate, 16000, true)?,
            ))
        })
        .collect();
    Ok(normalize(mapped?, total))
}
pub fn pad(
    intervals: &[Interval],
    padding_ms: u32,
    rate: u32,
    total: u64,
) -> Result<Vec<Interval>> {
    if padding_ms > 1000 {
        return Err(invalid("padding exceeds 1000 ms"));
    }
    let mut out = normalize(intervals.to_vec(), total);
    let padding = scale(u64::from(padding_ms), rate, 1000, false)?;
    let count = out.len();
    // Divide in source samples, assigning an odd remaining sample to the next segment.
    let after = padding / 2;
    let before = padding - after;
    for (n, i) in out.iter_mut().enumerate() {
        i.start_frame = i
            .start_frame
            .saturating_sub(if n == 0 { padding } else { before });
        i.end_frame = i
            .end_frame
            .saturating_add(if n + 1 == count { padding } else { after })
            .min(total);
    }
    Ok(normalize(out, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn padding_and_normalization() {
        let raw = [Interval::new(100, 200), Interval::new(400, 500)];
        assert_eq!(
            pad(&raw, 3, 1000, 1000).unwrap(),
            [Interval::new(97, 201), Interval::new(398, 503)]
        );
        assert_eq!(pad(&raw, 0, 1000, 1000).unwrap(), raw);
        assert_eq!(pad(&raw, 200, 1000, 1000).unwrap(), [Interval::new(0, 700)]);
        assert_eq!(
            normalize(
                vec![
                    Interval::new(50, 90),
                    Interval::new(1, 60),
                    Interval::new(500, 600)
                ],
                80
            ),
            [Interval::new(1, 80)]
        );
        assert_eq!(
            normalize(
                (0..10000)
                    .map(|n| Interval::new(n * 4, n * 4 + 1))
                    .collect(),
                u64::MAX
            )
            .len(),
            10000
        );
    }
    #[test]
    fn mapping_rounding_and_overflow() {
        assert_eq!(
            from_analysis(&[Interval::new(1, 3)], 44100, 100).unwrap(),
            [Interval::new(2, 9)]
        );
        assert!(from_analysis(&[Interval::new(u64::MAX - 1, u64::MAX)], 48000, u64::MAX).is_err());
    }
}
