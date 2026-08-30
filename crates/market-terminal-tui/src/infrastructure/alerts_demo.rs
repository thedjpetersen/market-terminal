use std::sync::Mutex;

use crate::features::alerts::{
    AlertCondition, AlertObservation, AlertRule, AlertRuleId, AlertSnapshot, AlertsQuery,
    DebouncePolicy, InstrumentRef,
};

/// Finite deterministic alert replay. Every query advances one frame and the
/// final frame is held, so replaying it also exercises idempotent evaluation.
pub struct DemoAlertsReplay {
    cursor: Mutex<usize>,
}

impl DemoAlertsReplay {
    pub fn new() -> Self {
        Self {
            cursor: Mutex::new(0),
        }
    }
}

impl Default for DemoAlertsReplay {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertsQuery for DemoAlertsReplay {
    fn load_snapshot(
        &self,
        _instruments: &[InstrumentRef],
    ) -> Result<AlertSnapshot, crate::features::alerts::AlertsError> {
        let mut cursor = self
            .cursor
            .lock()
            .expect("demo alerts replay cursor poisoned");
        let frame = ALERT_FRAMES[(*cursor).min(ALERT_FRAMES.len() - 1)];
        *cursor = (*cursor + 1).min(ALERT_FRAMES.len() - 1);

        Ok(AlertSnapshot::new(
            frame.sequence,
            frame.as_of,
            (frame.sequence == 0).then(demo_rules).unwrap_or_default(),
            frame
                .observations
                .iter()
                .copied()
                .map(DemoObservation::into_domain)
                .collect(),
            "DETERMINISTIC REPLAY · SIMULATED LOCAL",
        ))
    }
}

fn demo_rules() -> Vec<AlertRule> {
    vec![
        AlertRule::new(
            AlertRuleId::new("demo:aapl:above-205.35"),
            InstrumentRef::new("us:xnas:aapl", "AAPL"),
            AlertCondition::price_above(205.35),
            DebouncePolicy::consecutive(2),
        ),
        AlertRule::new(
            AlertRuleId::new("demo:nvda:move-above-2"),
            InstrumentRef::new("us:xnas:nvda", "NVDA"),
            AlertCondition::percent_move_above(2.0),
            DebouncePolicy::consecutive(1),
        ),
        AlertRule::new(
            AlertRuleId::new("demo:spy:below-650"),
            InstrumentRef::new("us:arcx:spy", "SPY"),
            AlertCondition::price_below(650.0),
            DebouncePolicy::consecutive(2),
        ),
    ]
}

#[derive(Debug, Clone, Copy)]
struct DemoFrame {
    sequence: u64,
    as_of: &'static str,
    observations: &'static [DemoObservation],
}

#[derive(Debug, Clone, Copy)]
struct DemoObservation {
    evaluation_id: &'static str,
    instrument_id: &'static str,
    price: f64,
    percent_move: f64,
    observed_at: &'static str,
}

impl DemoObservation {
    fn into_domain(self) -> AlertObservation {
        AlertObservation::new(
            self.evaluation_id,
            self.instrument_id,
            self.price,
            self.percent_move,
            self.observed_at,
        )
    }
}

const FRAME_0: [DemoObservation; 3] = [
    observation(
        "alert-replay-0-aapl",
        "us:xnas:aapl",
        205.30,
        0.84,
        "2026-08-25T20:00:00Z",
    ),
    observation(
        "alert-replay-0-nvda",
        "us:xnas:nvda",
        184.92,
        2.36,
        "2026-08-25T20:00:00Z",
    ),
    observation(
        "alert-replay-0-spy",
        "us:arcx:spy",
        653.28,
        0.64,
        "2026-08-25T20:00:00Z",
    ),
];

const FRAME_1: [DemoObservation; 3] = [
    observation(
        "alert-replay-1-aapl",
        "us:xnas:aapl",
        205.36,
        0.87,
        "2026-08-25T20:00:01Z",
    ),
    observation(
        "alert-replay-1-nvda",
        "us:xnas:nvda",
        185.10,
        2.45,
        "2026-08-25T20:00:01Z",
    ),
    observation(
        "alert-replay-1-spy",
        "us:arcx:spy",
        649.80,
        -0.54,
        "2026-08-25T20:00:01Z",
    ),
];

const FRAME_2: [DemoObservation; 3] = [
    observation(
        "alert-replay-2-aapl",
        "us:xnas:aapl",
        205.42,
        0.90,
        "2026-08-25T20:00:02Z",
    ),
    observation(
        "alert-replay-2-nvda",
        "us:xnas:nvda",
        183.40,
        1.50,
        "2026-08-25T20:00:02Z",
    ),
    observation(
        "alert-replay-2-spy",
        "us:arcx:spy",
        649.50,
        -0.58,
        "2026-08-25T20:00:02Z",
    ),
];

const ALERT_FRAMES: [DemoFrame; 3] = [
    DemoFrame {
        sequence: 0,
        as_of: "2026-08-25T20:00:00Z",
        observations: &FRAME_0,
    },
    DemoFrame {
        sequence: 1,
        as_of: "2026-08-25T20:00:01Z",
        observations: &FRAME_1,
    },
    DemoFrame {
        sequence: 2,
        as_of: "2026-08-25T20:00:02Z",
        observations: &FRAME_2,
    },
];

const fn observation(
    evaluation_id: &'static str,
    instrument_id: &'static str,
    price: f64,
    percent_move: f64,
    observed_at: &'static str,
) -> DemoObservation {
    DemoObservation {
        evaluation_id,
        instrument_id,
        price,
        percent_move,
        observed_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_holds_final_frame_with_stable_evaluation_ids() {
        let replay = DemoAlertsReplay::new();
        let first = replay.load_snapshot(&[]).unwrap();
        let _second = replay.load_snapshot(&[]).unwrap();
        let final_frame = replay.load_snapshot(&[]).unwrap();
        let repeated_final = replay.load_snapshot(&[]).unwrap();

        assert_eq!(first.rules.len(), 3);
        assert!(final_frame.rules.is_empty());
        assert_eq!(final_frame.sequence, repeated_final.sequence);
        assert_eq!(final_frame.observations, repeated_final.observations);
    }
}
