//! The wallet's outstanding obligations.

use std::collections::HashMap;

use crate::intent::{IntentId, IntentWithPolicy};

/// All the information the wallet has about what the user wants to do.
/// Not sorted in any particular order. Definetly not by priority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Queue(HashMap<IntentId, IntentWithPolicy>);

impl Queue {
    /// Take intents into a queue, giving each one an id.
    pub fn new(intents: impl IntoIterator<Item = IntentWithPolicy>) -> Self {
        Queue(
            intents
                .into_iter()
                .enumerate()
                .map(|(index, intent)| (IntentId(index), intent))
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The intent `id` names, or `None` if the queue does not hold it.
    pub fn get(&self, id: IntentId) -> Option<&IntentWithPolicy> {
        self.0.get(&id)
    }

    /// Every intent, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &IntentWithPolicy> {
        self.0.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{FixedPaymentInstructions, Intent, PayoffCurve};
    use bitcoin::{Amount, ScriptBuf};
    use std::time::{Duration, Instant};

    /// Built from an explicit `start` so two calls with the same arguments compare equal.
    fn intent(start: Instant, sats: u64) -> IntentWithPolicy {
        IntentWithPolicy::new(
            Intent::PaymentRequest(FixedPaymentInstructions {
                script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
                amount: Amount::from_sat(sats),
            }),
            PayoffCurve::new(vec![
                (start, Amount::ZERO),
                (start + Duration::from_secs(60), Amount::from_sat(sats)),
            ])
            .expect("well formed"),
        )
    }

    #[test]
    fn an_empty_queue_has_nothing_in_it() {
        let queue = Queue::new([]);

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
        assert_eq!(queue.iter().count(), 0);
        assert_eq!(queue.get(IntentId(0)), None);
    }

    #[test]
    fn a_queue_keeps_every_intent_it_was_given() {
        let start = Instant::now();
        let queue = Queue::new([intent(start, 1), intent(start, 2), intent(start, 3)]);

        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.iter().count(), 3);
        assert_eq!(queue.clone(), queue);
    }

    /// Ids are handed out in the order the intents arrived.
    #[test]
    fn an_id_names_the_intent_it_was_given() {
        let start = Instant::now();
        let queue = Queue::new([intent(start, 1), intent(start, 2)]);

        assert_eq!(queue.get(IntentId(0)), Some(&intent(start, 1)));
        assert_eq!(queue.get(IntentId(1)), Some(&intent(start, 2)));
    }

    #[test]
    fn an_id_the_queue_does_not_hold_names_nothing() {
        let start = Instant::now();
        let queue = Queue::new([intent(start, 1)]);

        assert_eq!(queue.get(IntentId(1)), None);
    }
}
