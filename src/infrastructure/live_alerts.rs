use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use chrono::Utc;

use crate::features::{
    alerts::{AlertObservation, AlertSnapshot, AlertsError, AlertsQuery, InstrumentRef},
    market_data::{DataQuality, MarketDataErrorKind, MarketDataQuery},
};

pub struct LiveAlertsQuery {
    market_data: Arc<dyn MarketDataQuery>,
    sequence: AtomicU64,
}

impl LiveAlertsQuery {
    pub fn new(market_data: Arc<dyn MarketDataQuery>) -> Self {
        Self {
            market_data,
            sequence: AtomicU64::new(0),
        }
    }
}

impl AlertsQuery for LiveAlertsQuery {
    fn load_snapshot(&self, instruments: &[InstrumentRef]) -> Result<AlertSnapshot, AlertsError> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        if instruments.is_empty() {
            return Ok(AlertSnapshot::new(
                sequence,
                Utc::now().to_rfc3339(),
                Vec::new(),
                Vec::new(),
                "LIVE MARKET DATA · NO RULES TO EVALUATE",
            ));
        }

        let mut observations = Vec::new();
        let mut failures = Vec::new();
        let mut sources = Vec::<String>::new();
        let mut permission_denied = false;
        for instrument in instruments {
            match self
                .market_data
                .quote_snapshots(std::slice::from_ref(&instrument.canonical_id))
            {
                Ok(snapshots) => {
                    let Some(snapshot) = snapshots.into_iter().next() else {
                        failures.push(format!("{} returned no quote", instrument.symbol));
                        continue;
                    };
                    if !snapshot.quality.is_usable() {
                        permission_denied |= snapshot.quality == DataQuality::PermissionDenied;
                        failures.push(format!(
                            "{} quote quality is {}",
                            instrument.symbol,
                            snapshot.quality.label()
                        ));
                        continue;
                    }
                    let Some(price) = snapshot.last else {
                        failures.push(format!("{} has no usable last price", instrument.symbol));
                        continue;
                    };
                    let Some(change) = snapshot.change else {
                        failures.push(format!("{} has no percent-move field", instrument.symbol));
                        continue;
                    };
                    let provider = snapshot.provenance.provider.as_str();
                    let observed_at = snapshot.as_of.as_str();
                    let evaluation_id = format!(
                        "{provider}:{}:{observed_at}:{:.8}:{:.8}",
                        instrument.canonical_id.as_str(),
                        price.value(),
                        change.percent.value()
                    );
                    observations.push(AlertObservation::new(
                        evaluation_id,
                        instrument.canonical_id.as_str(),
                        price.value(),
                        change.percent.value(),
                        observed_at,
                    ));
                    let source = format!(
                        "{} · {}",
                        provider.to_ascii_uppercase(),
                        snapshot.quality.label()
                    );
                    if !sources.contains(&source) {
                        sources.push(source);
                    }
                }
                Err(error) => {
                    permission_denied |= error.kind() == MarketDataErrorKind::PermissionDenied;
                    failures.push(format!("{}: {error}", instrument.symbol));
                }
            }
        }

        if observations.is_empty() {
            let message = bounded_failures(&failures);
            if permission_denied {
                return Err(AlertsError::PermissionDenied(message));
            }
            return Err(AlertsError::Unavailable(message));
        }
        let as_of = observations
            .iter()
            .map(|observation| observation.observed_at.as_str())
            .max()
            .unwrap_or_default()
            .to_owned();
        let mut source = sources.join(" + ");
        if !failures.is_empty() {
            source.push_str(&format!(
                " · PARTIAL {}/{}",
                observations.len(),
                instruments.len()
            ));
        }
        Ok(AlertSnapshot::new(
            sequence,
            as_of,
            Vec::new(),
            observations,
            source,
        ))
    }
}

fn bounded_failures(failures: &[String]) -> String {
    if failures.is_empty() {
        "provider returned no usable observations".to_owned()
    } else {
        failures.join("; ").chars().take(320).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{CommandInvocation, Workspace};
    use crate::features::alerts::{AlertEvaluation, AlertStateStore, AlertStatus, AlertsWorkspace};
    use crate::features::market_data::MarketDataError;
    use crate::infrastructure::{AlphaVantageMarketData, LocalPersistence};

    #[test]
    #[ignore = "live Alpha Vantage alert-observation contract test"]
    fn live_ibm_quote_becomes_an_idempotent_alert_observation() {
        let market_data = Arc::new(AlphaVantageMarketData::from_env());
        let query = LiveAlertsQuery::new(market_data);
        let instrument = InstrumentRef::new("us:listed:ibm", "IBM");

        let first = query
            .load_snapshot(std::slice::from_ref(&instrument))
            .unwrap();
        let second = query.load_snapshot(&[instrument]).unwrap();

        assert_eq!(first.observations.len(), 1);
        assert!(first.observations[0].price > 0.0);
        assert_eq!(
            first.observations[0].evaluation_id,
            second.observations[0].evaluation_id
        );
        assert!(first.source.contains("ALPHA-VANTAGE"));
        assert!(first.source.contains("DELAYED"));
    }

    #[test]
    #[ignore = "live Alpha Vantage + durable alert-state restart contract test"]
    fn live_ibm_evaluation_remains_idempotent_after_durable_restart() {
        let _ = dotenvy::dotenv();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("market-terminal-live-alerts-{unique}"));
        let store = Arc::new(LocalPersistence::new(&root));
        let query: Arc<dyn AlertsQuery> = Arc::new(LiveAlertsQuery::new(Arc::new(
            AlphaVantageMarketData::from_env(),
        )));
        let mut workspace = AlertsWorkspace::persistent(query, store.clone());
        workspace.handle_command(&CommandInvocation {
            function: "ALERT".to_owned(),
            args: vec!["IBM".to_owned(), ">".to_owned(), "0".to_owned()],
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while workspace
            .rules()
            .first()
            .and_then(|rule| rule.last_observation.as_ref())
            .is_none()
            && std::time::Instant::now() < deadline
        {
            workspace.poll_intents();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let first_observation = workspace.rules()[0]
            .last_observation
            .clone()
            .expect("live IBM observation should arrive");
        assert!(first_observation.price > 0.0);
        assert!(matches!(
            workspace.rules()[0].status,
            AlertStatus::Pending {
                matched: 1,
                required: 2
            }
        ));
        drop(workspace);

        let restored = store
            .load_alert_rules()
            .unwrap()
            .expect("workspace drop should flush durable alert state");
        assert_eq!(restored.rules.len(), 1);
        assert!(restored.revision > 0);
        assert!(restored.rules[0]
            .runtime_state()
            .processed_evaluation_ids
            .contains(&first_observation.evaluation_id));

        let live_after_restart = LiveAlertsQuery::new(Arc::new(AlphaVantageMarketData::from_env()))
            .load_snapshot(&[InstrumentRef::new("us:listed:ibm", "IBM")])
            .unwrap();
        assert_eq!(
            live_after_restart.observations[0].evaluation_id,
            first_observation.evaluation_id
        );
        assert_eq!(
            restored.rules[0]
                .clone()
                .evaluate(&live_after_restart.observations[0]),
            AlertEvaluation::Duplicate
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn market_data_errors_remain_typed_at_the_alert_boundary() {
        struct Unavailable;

        impl MarketDataQuery for Unavailable {
            fn quote_snapshots(
                &self,
                _instruments: &[crate::features::market_data::CanonicalInstrumentId],
            ) -> Result<Vec<crate::features::market_data::QuoteSnapshot>, MarketDataError>
            {
                Err(MarketDataError::PermissionDenied(
                    "test entitlement".to_owned(),
                ))
            }

            fn price_history(
                &self,
                _request: &crate::features::market_data::HistoryRequest,
            ) -> Result<Vec<crate::features::market_data::PriceBar>, MarketDataError> {
                Err(MarketDataError::Unsupported("test".to_owned()))
            }
        }

        let query = LiveAlertsQuery::new(Arc::new(Unavailable));
        let error = query
            .load_snapshot(&[InstrumentRef::new("us:listed:ibm", "IBM")])
            .unwrap_err();
        assert!(matches!(error, AlertsError::PermissionDenied(_)));
    }
}
