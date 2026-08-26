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
    use crate::features::market_data::MarketDataError;
    use crate::infrastructure::AlphaVantageMarketData;

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
