//! The wallet's outstanding obligations.

use std::fmt::Debug;
use std::hash::Hash;

use crate::intent::IntentWithPolicy;

/// All the information the wallet has about what the user wants to do.
///
/// Where intents are kept, and how their ids are made, is up to the implementation. A
/// queue may be held in memory or persisted, but either way the ids it hands out follow
/// the rules on [`Queue::Id`].
pub trait Queue {
    /// Names one intent in this queue.
    ///
    /// An id keeps naming the same intent for as long as the queue holds it, and is
    /// never reused for another.
    type Id: Copy + Eq + Hash + Debug;

    /// Every intent with its id, in no particular order.
    fn iter(&self) -> impl Iterator<Item = (Self::Id, &IntentWithPolicy)>;
}
