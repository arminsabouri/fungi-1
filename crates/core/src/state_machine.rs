//! The interface a sans-io state machine exposes to whatever drives it.

use crate::persist::PersistActions;

/// A state machine that turns inputs into events to record and effects to run.
pub trait StateMachine {
    /// Something that happened outside the state machine.
    type Input;

    /// A state transition, recorded so the state can be rebuilt by replay.
    type Event;

    /// IO the driver performs on the state machine's behalf.
    type Effect;

    /// React to `input`. An input that changes nothing yields an empty decision.
    fn decide(&self, input: Self::Input) -> Decision<Self::Event, Self::Effect>;

    /// The state after `event`. Replay calls this for every recorded event, so it must not do IO.
    fn apply(self, event: &Self::Event) -> Self;

    /// Whether the session is over and the driver should stop.
    fn is_closed(&self) -> bool;
}

/// What a state machine wants done in response to one input.
pub struct Decision<Event, Effect> {
    pub persist: PersistActions<Event>,
    pub effects: Vec<Effect>,
}
