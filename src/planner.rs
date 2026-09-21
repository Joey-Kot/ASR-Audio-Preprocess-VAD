use crate::{Config, Interval, Mode, Result, error::invalid, mapping::scale};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub intervals: Vec<Interval>,
    pub hard_cut: bool,
}
impl Plan {
    pub fn frames(&self) -> u64 {
        self.intervals.iter().map(|i| i.len()).sum()
    }
}

pub fn plan(config: &Config, intervals: &[Interval], rate: u32, total: u64) -> Result<Vec<Plan>> {
    if config.mode == Mode::Preconvert {
        return Ok(if total == 0 {
            vec![]
        } else {
            vec![Plan {
                intervals: vec![Interval::new(0, total)],
                hard_cut: false,
            }]
        });
    }
    if intervals.is_empty() {
        return Ok(vec![]);
    }
    if config.mode == Mode::Trim {
        return Ok(vec![Plan {
            intervals: intervals.to_vec(),
            hard_cut: false,
        }]);
    }
    let budget = scale(config.max_duration_ms, rate, 1000, false)?;
    if budget == 0 {
        return Err(invalid("duration is shorter than one source frame"));
    }
    let mut result = Vec::new();
    let mut current = Plan {
        intervals: vec![],
        hard_cut: false,
    };
    let mut used = 0;
    for original in intervals {
        let mut i = *original;
        if i.is_empty() {
            return Err(invalid("empty planning interval"));
        }
        let gap = if config.keep_pauses() {
            current
                .intervals
                .last()
                .map_or(0, |last| i.start_frame.saturating_sub(last.end_frame))
        } else {
            0
        };
        // Prefer a speech gap over splitting the next speech interval.
        if used > 0
            && (u128::from(used) + u128::from(gap) + u128::from(i.len()) > u128::from(budget))
        {
            result.push(current);
            current = Plan {
                intervals: vec![],
                hard_cut: false,
            };
            used = 0;
        } else if gap > 0 {
            current.intervals.last_mut().unwrap().end_frame = i.start_frame;
            used += gap;
        }
        while i.len() > budget - used {
            let cut = i.start_frame + budget - used;
            current.intervals.push(Interval::new(i.start_frame, cut));
            current.hard_cut = true;
            result.push(current);
            current = Plan {
                intervals: vec![],
                hard_cut: true,
            };
            used = 0;
            i.start_frame = cut;
        }
        used += i.len();
        current.intervals.push(i);
    }
    if !current.intervals.is_empty() {
        result.push(current);
    }
    Ok(result)
}

pub fn bisect(plan: &Plan) -> Result<(Plan, Plan)> {
    let target = plan.frames() / 2;
    if target == 0 {
        return Err(crate::Error::Constraint(
            "one source frame still exceeds the duration or byte limit".into(),
        ));
    }
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut remaining = target;
    let mut hard = false;
    for i in &plan.intervals {
        if remaining == 0 {
            right.push(*i);
        } else if i.len() <= remaining {
            left.push(*i);
            remaining -= i.len();
        } else {
            let cut = i.start_frame + remaining;
            left.push(Interval::new(i.start_frame, cut));
            right.push(Interval::new(cut, i.end_frame));
            hard = true;
            remaining = 0;
        }
    }
    Ok((
        Plan {
            intervals: left,
            hard_cut: plan.hard_cut || hard,
        },
        Plan {
            intervals: right,
            hard_cut: plan.hard_cut || hard,
        },
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hard_cuts_and_tail() {
        let c = Config {
            max_duration_ms: 1000,
            ..Config::default()
        };
        let p = plan(&c, &[Interval::new(0, 2500)], 1000, 2500).unwrap();
        assert_eq!(
            p.iter().map(Plan::frames).collect::<Vec<_>>(),
            [1000, 1000, 500]
        );
        assert!(p.iter().all(|p| p.hard_cut));
    }
    #[test]
    fn gaps_budget_and_conservation() {
        let raw = [
            Interval::new(0, 300),
            Interval::new(600, 1000),
            Interval::new(3000, 3300),
        ];
        for mode in [Mode::Process, Mode::Split] {
            let c = Config {
                mode,
                max_duration_ms: 1000,
                ..Config::default()
            };
            let p = plan(&c, &raw, 1000, 3300).unwrap();
            assert_eq!(p.len(), if mode == Mode::Split { 2 } else { 1 });
            assert_eq!(p[0].frames(), 1000);
            if mode == Mode::Split {
                assert_eq!(p[1].frames(), 300);
            }
            let (a, b) = bisect(&p[0]).unwrap();
            assert_eq!(a.frames() + b.frames(), p[0].frames());
        }
    }
}
