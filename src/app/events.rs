use std::{
    any::Any,
    collections::{HashMap, HashSet, VecDeque},
    fmt::Debug,
    sync::Arc,
};

pub trait EventTopic {
    fn topic(&self) -> &'static str;
}

trait ErasedEvent: Debug + Send + Sync {
    fn topic(&self) -> &'static str;
    fn as_any(&self) -> &dyn Any;
}

impl<T> ErasedEvent for T
where
    T: EventTopic + Debug + Send + Sync + 'static,
{
    fn topic(&self) -> &'static str { EventTopic::topic(self) }
    fn as_any(&self) -> &dyn Any { self }
}

#[derive(Clone, Debug)]
pub struct EventEnvelope {
    sequence: u64,
    event: Arc<dyn ErasedEvent>,
}

impl EventEnvelope {
    pub fn sequence(&self) -> u64 { self.sequence }
    pub fn topic(&self) -> &'static str { self.event.topic() }

    pub fn downcast_ref<T: EventTopic + Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        self.event.as_any().downcast_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionMetrics {
    pub pending: usize,
    pub capacity: usize,
    pub dropped: u64,
    pub last_sequence: Option<u64>,
}

struct Subscription {
    topics: HashSet<&'static str>,
    capacity: usize,
    queue: VecDeque<EventEnvelope>,
    dropped: u64,
    last_sequence: Option<u64>,
}

/// A bounded, in-process event bus for cross-feature notifications.
///
/// Event schemas remain owned by their publishing features. The kernel only
/// provides ordered envelopes, bounded fan-out, cancellation, and lag metrics.
#[derive(Default)]
pub struct EventBus {
    next_sequence: u64,
    next_subscription: u64,
    subscriptions: HashMap<SubscriptionId, Subscription>,
}

impl EventBus {
    pub fn subscribe(
        &mut self,
        topics: impl IntoIterator<Item = &'static str>,
        capacity: usize,
    ) -> SubscriptionId {
        self.next_subscription = self.next_subscription.wrapping_add(1);
        let id = SubscriptionId(self.next_subscription);
        self.subscriptions.insert(
            id,
            Subscription {
                topics: topics.into_iter().collect(),
                capacity: capacity.max(1),
                queue: VecDeque::with_capacity(capacity.max(1)),
                dropped: 0,
                last_sequence: None,
            },
        );
        id
    }

    pub fn unsubscribe(&mut self, id: SubscriptionId) -> bool {
        self.subscriptions.remove(&id).is_some()
    }

    pub fn publish<T>(&mut self, event: T) -> u64
    where
        T: EventTopic + Debug + Send + Sync + 'static,
    {
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let sequence = self.next_sequence;
        let envelope = EventEnvelope { sequence, event: Arc::new(event) };
        for subscription in self.subscriptions.values_mut() {
            if !subscription.topics.is_empty() && !subscription.topics.contains(envelope.topic()) {
                continue;
            }
            if subscription.queue.len() == subscription.capacity {
                subscription.queue.pop_front();
                subscription.dropped = subscription.dropped.saturating_add(1);
            }
            subscription.last_sequence = Some(sequence);
            subscription.queue.push_back(envelope.clone());
        }
        sequence
    }

    pub fn drain(&mut self, id: SubscriptionId) -> Vec<EventEnvelope> {
        self.subscriptions
            .get_mut(&id)
            .map(|subscription| subscription.queue.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn metrics(&self, id: SubscriptionId) -> Option<SubscriptionMetrics> {
        self.subscriptions.get(&id).map(|subscription| SubscriptionMetrics {
            pending: subscription.queue.len(),
            capacity: subscription.capacity,
            dropped: subscription.dropped,
            last_sequence: subscription.last_sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct QuoteUpdated {
        symbol: &'static str,
    }

    impl EventTopic for QuoteUpdated {
        fn topic(&self) -> &'static str { "market.quote.updated" }
    }

    #[derive(Debug)]
    struct StoryArrived;

    impl EventTopic for StoryArrived {
        fn topic(&self) -> &'static str { "news.story.arrived" }
    }

    #[test]
    fn subscriptions_filter_and_preserve_typed_events() {
        let mut bus = EventBus::default();
        let quotes = bus.subscribe(["market.quote.updated"], 4);

        bus.publish(StoryArrived);
        bus.publish(QuoteUpdated { symbol: "AAPL" });

        let events = bus.drain(quotes);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence(), 2);
        assert_eq!(
            events[0].downcast_ref::<QuoteUpdated>(),
            Some(&QuoteUpdated { symbol: "AAPL" })
        );
    }

    #[test]
    fn bounded_queues_drop_oldest_and_report_lag() {
        let mut bus = EventBus::default();
        let quotes = bus.subscribe([], 2);

        bus.publish(QuoteUpdated { symbol: "AAPL" });
        bus.publish(QuoteUpdated { symbol: "MSFT" });
        bus.publish(QuoteUpdated { symbol: "NVDA" });

        assert_eq!(
            bus.metrics(quotes),
            Some(SubscriptionMetrics {
                pending: 2,
                capacity: 2,
                dropped: 1,
                last_sequence: Some(3),
            })
        );
        let symbols = bus
            .drain(quotes)
            .into_iter()
            .filter_map(|event| event.downcast_ref::<QuoteUpdated>().map(|quote| quote.symbol))
            .collect::<Vec<_>>();
        assert_eq!(symbols, vec!["MSFT", "NVDA"]);
    }

    #[test]
    fn unsubscribe_cancels_delivery() {
        let mut bus = EventBus::default();
        let subscription = bus.subscribe([], 4);
        assert!(bus.unsubscribe(subscription));
        bus.publish(QuoteUpdated { symbol: "AAPL" });
        assert!(bus.drain(subscription).is_empty());
    }
}
