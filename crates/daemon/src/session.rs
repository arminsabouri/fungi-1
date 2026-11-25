//! The state machine `fungid` drives.

use cost_function::plan::PlanIntent;
use fungi_core::persist::PersistActions;
use fungi_core::state_machine::{Decision, StateMachine};

// TODO: replace with the real session state machine.
/// A session that settles every plan it is given, and never closes.
pub struct StubSession;

pub enum Input {
    Plan(PlanIntent<u64>),
    Settled,
}

#[derive(Clone)]
pub enum Event {
    Planned(
        #[expect(dead_code, reason = "recorded for replay; the stub reads nothing back")]
        PlanIntent<u64>,
    ),
    Settled,
}

pub enum Effect {
    Settle(PlanIntent<u64>),
}

impl StateMachine for StubSession {
    type Input = Input;
    type Event = Event;
    type Effect = Effect;

    fn decide(&self, input: Input) -> Decision<Event, Effect> {
        match input {
            Input::Plan(plan) => Decision {
                persist: PersistActions::Save(Event::Planned(plan.clone())),
                effects: vec![Effect::Settle(plan)],
            },
            Input::Settled => Decision {
                persist: PersistActions::Save(Event::Settled),
                effects: vec![],
            },
        }
    }

    fn apply(self, _event: &Event) -> Self {
        self
    }

    fn is_closed(&self) -> bool {
        false
    }
}

/// Run an effect. Settling only logs the plan.
pub async fn run_effect(effect: Effect) -> Input {
    match effect {
        Effect::Settle(plan) => {
            tracing::info!(?plan, "settling");
            Input::Settled
        }
    }
}
