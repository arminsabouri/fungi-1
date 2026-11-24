//! Storage for the event logs a state machine replays.

use std::convert::Infallible;
use std::sync::Mutex;

/// An append only log of the events one session produced.
pub trait Persister {
    /// Whatever the storage layer underneath fails with.
    type InternalStorageError: std::error::Error + Send + Sync + 'static;

    /// The events this session records.
    type SessionEvent;

    /// Append one event to the log.
    fn save_event(&self, event: Self::SessionEvent) -> Result<(), Self::InternalStorageError>;

    /// Every event of the session, in the order they were saved.
    fn load(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Self::SessionEvent>>, Self::InternalStorageError>;

    /// Close the session, after which nothing more is appended.
    fn close(&self) -> Result<(), Self::InternalStorageError>;
}

/// What a state transition asks its persister to do.
pub enum PersistActions<Event> {
    /// Nothing happened worth recording.
    NoOp,
    /// Record one event.
    Save(Event),
    /// Record one event and end the session.
    /// If close fails for any reason, the event is still recorded.
    /// Similarly if save fails, the session is not closed.
    SaveAndClose(Event),
}

impl<Event> PersistActions<Event> {
    /// Carry out the action against `persister`.
    pub fn execute<P>(self, persister: &P) -> Result<(), P::InternalStorageError>
    where
        P: Persister<SessionEvent = Event>,
    {
        match self {
            Self::NoOp => {}
            Self::Save(event) => persister.save_event(event)?,
            Self::SaveAndClose(event) => {
                persister.save_event(event)?;
                persister.close()?;
            }
        }
        Ok(())
    }
}

/// A session log held in memory, lost when it is dropped.
///
/// # Panics
///
/// [`Persister::save_event`] panics if the session is closed.
pub struct InMemoryPersister<Event> {
    log: Mutex<Log<Event>>,
}

struct Log<Event> {
    events: Vec<Event>,
    closed: bool,
}

impl<Event> Default for InMemoryPersister<Event> {
    fn default() -> Self {
        Self {
            log: Mutex::new(Log {
                events: Vec::new(),
                closed: false,
            }),
        }
    }
}

impl<Event: Clone + 'static> Persister for InMemoryPersister<Event> {
    type InternalStorageError = Infallible;
    type SessionEvent = Event;

    fn save_event(&self, event: Event) -> Result<(), Infallible> {
        let mut log = self.log.lock().expect("lock poisoned");
        assert!(!log.closed, "event saved to a closed session");
        log.events.push(event);
        Ok(())
    }

    fn load(&self) -> Result<Box<dyn Iterator<Item = Event>>, Infallible> {
        let log = self.log.lock().expect("lock poisoned");
        Ok(Box::new(log.events.clone().into_iter()))
    }

    fn close(&self) -> Result<(), Infallible> {
        self.log.lock().expect("lock poisoned").closed = true;
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::cell::RefCell;

    impl<Event: Clone> InMemoryPersister<Event> {
        fn events(&self) -> Vec<Event> {
            self.log.lock().unwrap().events.clone()
        }

        fn closed(&self) -> bool {
            self.log.lock().unwrap().closed
        }
    }

    #[test]
    fn events_load_in_the_order_they_were_saved() {
        let persister = InMemoryPersister::<u8>::default();

        persister
            .save_event(1)
            .expect("in memory storage cannot fail");
        persister
            .save_event(2)
            .expect("in memory storage cannot fail");

        let replayed: Vec<u8> = persister
            .load()
            .expect("in memory storage cannot fail")
            .collect();

        assert_eq!(replayed, [1, 2]);
    }

    #[test]
    fn a_closed_session_says_so() {
        let persister = InMemoryPersister::<u8>::default();

        persister.close().expect("in memory storage cannot fail");

        assert!(persister.closed());
    }

    fn execute(action: PersistActions<u8>) -> InMemoryPersister<u8> {
        let persister = InMemoryPersister::<u8>::default();

        action
            .execute(&persister)
            .expect("in memory storage cannot fail");

        persister
    }

    #[test]
    fn a_no_op_records_nothing() {
        let persister = execute(PersistActions::NoOp);

        assert!(persister.events().is_empty());
        assert!(!persister.closed());
    }

    #[test]
    fn a_save_leaves_the_session_open() {
        let persister = execute(PersistActions::Save(1));

        assert_eq!(persister.events(), [1]);
        assert!(!persister.closed());
    }

    #[test]
    fn a_closing_save_records_the_event_first() {
        let persister = execute(PersistActions::SaveAndClose(1));

        assert_eq!(persister.events(), [1]);
        assert!(persister.closed());
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Save(u8),
        Close,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
    enum TestError {
        #[error("save failed")]
        Save,
        #[error("close failed")]
        Close,
    }

    struct MockPersister {
        calls: RefCell<Vec<Call>>,
        save_result: Result<(), TestError>,
        close_result: Result<(), TestError>,
    }

    impl Persister for MockPersister {
        type InternalStorageError = TestError;
        type SessionEvent = u8;

        fn save_event(&self, event: u8) -> Result<(), TestError> {
            self.calls.borrow_mut().push(Call::Save(event));
            self.save_result
        }

        fn load(&self) -> Result<Box<dyn Iterator<Item = u8>>, TestError> {
            panic!("unexpected load call");
        }

        fn close(&self) -> Result<(), TestError> {
            self.calls.borrow_mut().push(Call::Close);
            self.close_result
        }
    }

    #[test]
    fn a_closing_save_calls_save_then_close_once() {
        let persister = MockPersister {
            calls: RefCell::new(vec![]),
            save_result: Ok(()),
            close_result: Ok(()),
        };

        let result = PersistActions::SaveAndClose(1).execute(&persister);

        assert_eq!(result, Ok(()));
        assert_eq!(*persister.calls.borrow(), [Call::Save(1), Call::Close]);
    }

    #[test]
    fn a_save_propagates_the_storage_error() {
        let persister = MockPersister {
            calls: RefCell::new(vec![]),
            save_result: Err(TestError::Save),
            close_result: Ok(()),
        };

        let result = PersistActions::Save(1).execute(&persister);

        assert_eq!(result, Err(TestError::Save));
        assert_eq!(*persister.calls.borrow(), [Call::Save(1)]);
    }

    #[test]
    fn a_failed_save_does_not_close_the_session() {
        let persister = MockPersister {
            calls: RefCell::new(vec![]),
            save_result: Err(TestError::Save),
            close_result: Ok(()),
        };

        let result = PersistActions::SaveAndClose(1).execute(&persister);

        assert_eq!(result, Err(TestError::Save));
        assert_eq!(*persister.calls.borrow(), [Call::Save(1)]);
    }

    #[test]
    fn a_failed_close_propagates_the_error_after_saving() {
        let persister = MockPersister {
            calls: RefCell::new(vec![]),
            save_result: Ok(()),
            close_result: Err(TestError::Close),
        };

        let result = PersistActions::SaveAndClose(1).execute(&persister);

        assert_eq!(result, Err(TestError::Close));
        assert_eq!(*persister.calls.borrow(), [Call::Save(1), Call::Close]);
    }
}
