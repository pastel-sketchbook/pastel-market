//! Event system: background thread polling crossterm + tick timer.
//!
//! Uses [`EventSource`] trait abstraction for testability.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent};

/// Application-level events.
#[derive(Debug)]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// A mouse event occurred.
    Mouse(MouseEvent),
    /// The terminal was resized to (columns, rows).
    #[allow(dead_code)]
    Resize(u16, u16),
    /// The refresh timer fired — time to re-fetch quotes.
    Tick,
}

/// Abstraction over terminal event polling.
///
/// Production code uses [`CrosstermSource`]; tests inject fakes via
/// [`EventHandler::with_source`].
pub trait EventSource: Send + 'static {
    /// Poll for a terminal event, blocking for at most `timeout`.
    ///
    /// Returns `true` if an event is ready to be read.
    fn poll(&self, timeout: Duration) -> bool;

    /// Read the next terminal event.
    ///
    /// Only called after [`poll`](Self::poll) returned `true`.
    fn read(&self) -> Option<CrosstermEvent>;
}

/// Default [`EventSource`] backed by crossterm.
pub struct CrosstermSource;

impl EventSource for CrosstermSource {
    fn poll(&self, timeout: Duration) -> bool {
        event::poll(timeout).unwrap_or(false)
    }

    fn read(&self) -> Option<CrosstermEvent> {
        event::read().ok()
    }
}

/// Polls crossterm events and emits tick events on a fixed interval.
///
/// Runs in a dedicated background thread so the main loop never blocks.
pub struct EventHandler {
    rx: mpsc::Receiver<Event>,
    // Keep the handle alive so the thread isn't detached silently.
    _tx_thread: thread::JoinHandle<()>,
}

impl EventHandler {
    /// Spawn the event-polling thread with the given tick rate.
    #[must_use]
    pub fn new(tick_rate: Duration) -> Self {
        Self::with_source(tick_rate, CrosstermSource)
    }

    /// Spawn the event-polling thread with a custom [`EventSource`].
    ///
    /// This constructor enables testing without a real terminal.
    #[must_use]
    pub fn with_source<S: EventSource>(tick_rate: Duration, source: S) -> Self {
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            let mut last_tick = Instant::now();
            loop {
                let timeout = tick_rate.saturating_sub(last_tick.elapsed());

                if source.poll(timeout) {
                    match source.read() {
                        Some(CrosstermEvent::Key(key)) if tx.send(Event::Key(key)).is_err() => {
                            return;
                        }
                        Some(CrosstermEvent::Resize(w, h))
                            if tx.send(Event::Resize(w, h)).is_err() =>
                        {
                            return;
                        }
                        Some(CrosstermEvent::Mouse(m)) if tx.send(Event::Mouse(m)).is_err() => {
                            return;
                        }
                        _ => {}
                    }
                }

                if last_tick.elapsed() >= tick_rate {
                    if tx.send(Event::Tick).is_err() {
                        return;
                    }
                    last_tick = Instant::now();
                }
            }
        });

        Self {
            rx,
            _tx_thread: handle,
        }
    }

    /// Block until the next event is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the sender thread has exited.
    pub fn next(&self) -> Result<Event> {
        self.rx.recv().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fake event source that emits a fixed sequence of key events.
    struct FakeSource {
        events: std::sync::Mutex<Vec<CrosstermEvent>>,
        poll_count: Arc<AtomicUsize>,
    }

    impl FakeSource {
        fn new(events: Vec<CrosstermEvent>) -> Self {
            Self {
                events: std::sync::Mutex::new(events.into_iter().rev().collect()),
                poll_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl EventSource for FakeSource {
        fn poll(&self, _timeout: Duration) -> bool {
            self.poll_count.fetch_add(1, Ordering::SeqCst);
            let events = self.events.lock().expect("lock");
            !events.is_empty()
        }

        fn read(&self) -> Option<CrosstermEvent> {
            let mut events = self.events.lock().expect("lock");
            events.pop()
        }
    }

    fn make_key_event(code: KeyCode) -> CrosstermEvent {
        CrosstermEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn handler_new_does_not_panic() {
        let source = FakeSource::new(vec![]);
        let handler = EventHandler::with_source(Duration::from_millis(10), source);
        drop(handler);
    }

    #[test]
    fn handler_receives_tick_event() {
        let source = FakeSource::new(vec![]);
        let handler = EventHandler::with_source(Duration::from_millis(5), source);

        let event = handler.next().expect("should receive event");
        assert!(
            matches!(event, Event::Tick),
            "expected Tick, got: {event:?}"
        );
    }

    #[test]
    fn handler_receives_key_event() {
        let source = FakeSource::new(vec![make_key_event(KeyCode::Char('q'))]);
        let handler = EventHandler::with_source(Duration::from_secs(10), source);

        let event = handler.next().expect("should receive event");
        match event {
            Event::Key(key) => assert_eq!(key.code, KeyCode::Char('q')),
            other => panic!("expected Key event, got: {other:?}"),
        }
    }

    #[test]
    fn handler_receives_multiple_keys_then_ticks() {
        let source = FakeSource::new(vec![
            make_key_event(KeyCode::Char('a')),
            make_key_event(KeyCode::Char('b')),
        ]);
        let handler = EventHandler::with_source(Duration::from_millis(5), source);

        let e1 = handler.next().expect("event 1");
        let e2 = handler.next().expect("event 2");

        assert!(matches!(e1, Event::Key(_)), "e1 should be Key: {e1:?}");
        assert!(matches!(e2, Event::Key(_)), "e2 should be Key: {e2:?}");

        let e3 = handler.next().expect("event 3");
        assert!(matches!(e3, Event::Tick), "e3 should be Tick: {e3:?}");
    }

    #[test]
    fn handler_thread_exits_when_receiver_dropped() {
        let source = FakeSource::new(vec![]);
        let handler = EventHandler::with_source(Duration::from_millis(5), source);
        let _ = handler.next();
        drop(handler);
    }

    #[test]
    fn handler_non_key_events_are_ignored() {
        let source = FakeSource::new(vec![CrosstermEvent::FocusGained]);
        let handler = EventHandler::with_source(Duration::from_millis(5), source);

        let event = handler.next().expect("should receive event");
        assert!(
            matches!(event, Event::Tick),
            "expected Tick (non-key events filtered), got: {event:?}"
        );
    }

    #[test]
    fn handler_forwards_resize_event() {
        let source = FakeSource::new(vec![CrosstermEvent::Resize(120, 40)]);
        let handler = EventHandler::with_source(Duration::from_secs(10), source);

        let event = handler.next().expect("should receive event");
        assert!(
            matches!(event, Event::Resize(120, 40)),
            "expected Resize(120, 40), got: {event:?}"
        );
    }
}
