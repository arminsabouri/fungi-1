//! Groups of intents realized together.

/// Intents realized by one transaction, named by the ids of the queue they came from.
///
/// Only the grouping. Funding is decided separately, so the same batch can be priced
/// against whatever coins are left when its turn comes.
///
/// Generic over the id rather than the queue, so a batch from a [`crate::queue::Queue`]
/// `Q` is a `Batch<Q::Id>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch<Id> {
    /// The intents this transaction realizes.
    pub intents: Vec<Id>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{Action, FixedPaymentInstructions, IntentWithPolicy, PayoffCurve};
    use crate::queue::{InMemoryQueue, Queue};
    use bitcoin::{Amount, ScriptBuf};
    use std::time::Instant;

    fn intent(start: Instant, sats: u64) -> IntentWithPolicy {
        IntentWithPolicy::new(
            Action::OutputCreation(FixedPaymentInstructions {
                script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
                amount: Amount::from_sat(sats),
            }),
            PayoffCurve::new(start, vec![]),
        )
    }

    #[test]
    fn a_batch_names_intents_by_their_queue_ids() {
        let start = Instant::now();
        let mut queue = InMemoryQueue::new([]);
        let first = queue.push(intent(start, 1));
        let second = queue.push(intent(start, 2));

        let batch: Batch<<InMemoryQueue as Queue>::Id> = Batch {
            intents: vec![first, second],
        };

        assert_eq!(batch.intents, [first, second]);
        assert_eq!(batch.clone(), batch);
    }
}
