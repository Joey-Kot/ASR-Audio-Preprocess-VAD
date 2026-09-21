//! Earshot 1.2.2, continuous 256-sample frames at 16 kHz.
use crate::{Error, Interval, Result};
const FRAME: usize = 256;
// A confident onset prevents sustained ambiguous background noise from opening
// a segment. The lower continuation threshold preserves soft syllables afterward.
const CONTINUE_THRESHOLD: f32 = 0.5;
const START_FRAMES: u64 = 3;
pub(crate) struct Vad {
    start_threshold: f32,
    detector: earshot::Detector,
    pending: [i16; FRAME],
    used: usize,
    position: u64,
    candidate: Option<u64>,
    voiced: u64,
    consecutive: u64,
    active: bool,
    last_end: u64,
    intervals: Vec<Interval>,
    score_bins: [u64; 10],
    high_runs: [u64; 4],
    max_high_runs: [u64; 4],
}
impl Default for Vad {
    fn default() -> Self {
        Self {
            start_threshold: crate::VadConfig::default().start_threshold,
            detector: earshot::Detector::default(),
            pending: [0; FRAME],
            used: 0,
            position: 0,
            candidate: None,
            voiced: 0,
            consecutive: 0,
            active: false,
            last_end: 0,
            intervals: vec![],
            score_bins: [0; 10],
            high_runs: [0; 4],
            max_high_runs: [0; 4],
        }
    }
}
impl Vad {
    pub fn new(start_threshold: f32) -> Self {
        Self {
            start_threshold,
            ..Self::default()
        }
    }
    pub fn push(&mut self, mut input: &[i16]) -> Result<()> {
        while !input.is_empty() {
            let n = (FRAME - self.used).min(input.len());
            self.pending[self.used..self.used + n].copy_from_slice(&input[..n]);
            self.used += n;
            input = &input[n..];
            if self.used == FRAME {
                let score = self.detector.predict_i16(&self.pending);
                self.score_bins[((score * 10.0) as usize).min(9)] += 1;
                for (n, threshold) in [0.9, 0.95, 0.99, 0.999].into_iter().enumerate() {
                    self.high_runs[n] = if score >= threshold {
                        self.high_runs[n] + 1
                    } else {
                        0
                    };
                    self.max_high_runs[n] = self.max_high_runs[n].max(self.high_runs[n]);
                }
                self.accept_score(score, FRAME as u64)?;
                self.used = 0;
            }
        }
        Ok(())
    }
    pub fn diagnostic(&self) -> String {
        format!(
            "Earshot score deciles: {:?}; longest runs at 0.9/0.95/0.99/0.999: {:?}",
            self.score_bins, self.max_high_runs
        )
    }
    fn accept_score(&mut self, score: f32, n: u64) -> Result<()> {
        let start = self.position;
        self.position = self
            .position
            .checked_add(n)
            .ok_or_else(|| Error::Detection("VAD coordinate overflow".into()))?;
        if score >= CONTINUE_THRESHOLD {
            self.candidate.get_or_insert(start);
            self.voiced += 1;
            self.consecutive = if score >= self.start_threshold {
                self.consecutive + 1
            } else {
                0
            };
            self.last_end = self.position;
            if !self.active {
                // Keep a bounded onset history while waiting for a confident start.
                self.candidate = self
                    .candidate
                    .map(|p| p.max(self.position.saturating_sub(6 * FRAME as u64)));
            }
            self.active |= self.consecutive >= START_FRAMES;
        } else {
            self.consecutive = 0;
            if !self.active {
                self.candidate = None;
                self.voiced = 0;
            } else if self.position - self.last_end >= 10 * FRAME as u64 {
                self.close();
            }
        }
        Ok(())
    }
    fn close(&mut self) {
        if self.active && self.voiced >= 4 {
            self.intervals
                .push(Interval::new(self.candidate.unwrap(), self.last_end));
        }
        self.candidate = None;
        self.voiced = 0;
        self.consecutive = 0;
        self.active = false;
    }
    pub fn finish(mut self) -> Result<Vec<Interval>> {
        if self.used > 0 {
            self.pending[self.used..].fill(0);
            let score = self.detector.predict_i16(&self.pending);
            self.accept_score(score, self.used as u64)?;
        }
        self.close();
        Ok(self.intervals)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_chunks_tail_and_independent_inputs() {
        for _ in 0..3 {
            let mut v = Vad::default();
            for _ in 0..100 {
                v.push(&[0; 317]).unwrap();
            }
            assert!(v.finish().unwrap().is_empty());
        }
    }
    #[test]
    fn close_at_eof_and_short_noise() {
        let mut v = Vad::default();
        v.accept_score(1.0, 256).unwrap();
        v.accept_score(0.0, 256).unwrap();
        for _ in 0..4 {
            v.accept_score(1.0, 256).unwrap();
        }
        assert_eq!(v.finish().unwrap(), [Interval::new(512, 1536)]);
    }
    #[test]
    fn softer_onset_and_continuation() {
        let mut below = Vad::default();
        for _ in 0..10 {
            below.accept_score(0.599, 256).unwrap();
        }
        assert!(below.finish().unwrap().is_empty());

        let mut speech = Vad::default();
        for _ in 0..3 {
            speech.accept_score(0.6, 256).unwrap();
        }
        speech.accept_score(0.5, 256).unwrap();
        assert_eq!(speech.finish().unwrap(), [Interval::new(0, 1024)]);
    }
    #[test]
    fn custom_onset_threshold_controls_detection() {
        for (threshold, expected) in [(0.6, 1), (0.9, 0)] {
            let mut vad = Vad::new(threshold);
            for _ in 0..4 {
                vad.accept_score(0.7, 256).unwrap();
            }
            assert_eq!(vad.finish().unwrap().len(), expected);
        }
    }
}
