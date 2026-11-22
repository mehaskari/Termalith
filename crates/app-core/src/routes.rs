//! Where a terminal's output goes (P8-10, ADR-042). Every terminal emits
//! through a [`Route`], so its output can move to another window: the route
//! holds events back while it is detached and hands them to the next
//! window's sink when that one attaches.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use terminal_pty::{TerminalEvent, TerminalEventSink};

/// The visible text a moved terminal starts with, at most this many bytes.
pub const MAX_HISTORY_BYTES: usize = 1024 * 1024;

type BoxedSink = Box<dyn TerminalEventSink>;

#[derive(Default)]
struct RouteState {
    target: Option<BoxedSink>,
    /// The target before [`Route::detach`], kept until a new one attaches so
    /// a failed move can go back.
    previous: Option<BoxedSink>,
    detached: bool,
    /// Events held back while detached. Bounded by the terminal's flow
    /// control: nothing acknowledges them, so the reader stops sending.
    held: VecDeque<TerminalEvent>,
}

/// A terminal's switchable output path.
#[derive(Default)]
pub struct Route {
    state: Mutex<RouteState>,
}

impl Route {
    pub fn new(target: impl TerminalEventSink) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(RouteState {
                target: Some(Box::new(target)),
                ..RouteState::default()
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RouteState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The sink the terminal emits into. Returns `false` (the terminal stops)
    /// only when the window is gone and no move is in progress.
    pub fn sink(self: &Arc<Self>) -> impl TerminalEventSink + use<> {
        let route = Arc::clone(self);
        move |event: TerminalEvent| route.emit(event)
    }

    fn emit(&self, event: TerminalEvent) -> bool {
        let mut st = self.lock();
        if st.detached {
            st.held.push_back(event);
            return true;
        }
        match st.target.as_mut() {
            Some(t) => {
                if t.emit(event) {
                    true
                } else {
                    st.target = None;
                    false
                }
            }
            None => false,
        }
    }

    /// Stops delivering to the current window and holds output back.
    pub fn detach(&self) {
        let mut st = self.lock();
        st.detached = true;
        if let Some(t) = st.target.take() {
            st.previous = Some(t);
        }
    }

    /// Undoes [`Route::detach`] (the new window could not be opened): held
    /// events go to the old window again.
    pub fn restore(&self) {
        let mut st = self.lock();
        if !st.detached {
            return;
        }
        st.target = st.previous.take();
        st.detached = false;
        Self::flush(&mut st);
    }

    /// Delivers to `target` from now on, first the events held back.
    pub fn attach(&self, target: impl TerminalEventSink) {
        let mut st = self.lock();
        st.previous = None;
        st.target = Some(Box::new(target));
        st.detached = false;
        Self::flush(&mut st);
    }

    fn flush(st: &mut RouteState) {
        while let Some(event) = st.held.pop_front() {
            let delivered = st.target.as_mut().is_some_and(|t| t.emit(event));
            if !delivered {
                st.target = None;
                st.held.clear();
                return;
            }
        }
    }

    /// Whether output is being held back for a window that has not attached.
    pub fn is_detached(&self) -> bool {
        self.lock().detached
    }
}

/// Text a moved terminal starts with: the end of `text`, at most
/// [`MAX_HISTORY_BYTES`], cut at a character boundary.
pub fn clip_history(text: &str) -> &str {
    if text.len() <= MAX_HISTORY_BYTES {
        return text;
    }
    let mut start = text.len() - MAX_HISTORY_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The routes of open terminals and the history of moved ones, by terminal id.
#[derive(Default)]
pub struct TerminalRoutes {
    routes: Mutex<HashMap<u64, Arc<Route>>>,
    /// Terminals living in their own window: id → history not yet taken.
    moved: Mutex<HashMap<u64, Option<String>>>,
}

impl TerminalRoutes {
    pub fn register(&self, id: u64, route: Arc<Route>) {
        lock(&self.routes).insert(id, route);
    }

    pub fn get(&self, id: u64) -> Option<Arc<Route>> {
        lock(&self.routes).get(&id).cloned()
    }

    /// Forgets a closed terminal.
    pub fn remove(&self, id: u64) {
        lock(&self.routes).remove(&id);
        lock(&self.moved).remove(&id);
    }

    /// Records that terminal `id` moves to its own window with `history`.
    pub fn mark_moved(&self, id: u64, history: &str) {
        lock(&self.moved).insert(id, Some(clip_history(history).to_owned()));
    }

    /// The move did not happen after all.
    pub fn unmark_moved(&self, id: u64) {
        lock(&self.moved).remove(&id);
    }

    /// The history of a moved terminal, once (a reload gets an empty one).
    /// `None` when `id` is not a moved terminal.
    pub fn take_history(&self, id: u64) -> Option<String> {
        lock(&self.moved)
            .get_mut(&id)
            .map(|h| h.take().unwrap_or_default())
    }

    pub fn is_moved(&self, id: u64) -> bool {
        lock(&self.moved).contains_key(&id)
    }

    /// Terminals in their own windows (kept when the main window reloads).
    pub fn moved_ids(&self) -> Vec<u64> {
        lock(&self.moved).keys().copied().collect()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sink that records output and can be told to fail.
    fn recorder() -> (
        Arc<Mutex<Vec<String>>>,
        Arc<Mutex<bool>>,
        impl TerminalEventSink,
    ) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let alive = Arc::new(Mutex::new(true));
        let (g, a) = (Arc::clone(&got), Arc::clone(&alive));
        let sink = move |ev: TerminalEvent| {
            if !*a.lock().unwrap() {
                return false;
            }
            g.lock().unwrap().push(match ev {
                TerminalEvent::Output(b) => String::from_utf8(b).unwrap(),
                TerminalEvent::Exited(_) => "<exit>".to_owned(),
            });
            true
        };
        (got, alive, sink)
    }

    fn out(s: &str) -> TerminalEvent {
        TerminalEvent::Output(s.as_bytes().to_vec())
    }

    #[test]
    fn output_moves_to_the_new_window_without_loss() {
        let (old, _, old_sink) = recorder();
        let route = Route::new(old_sink);
        let mut sink = route.sink();
        assert!(sink.emit(out("a")));
        route.detach();
        assert!(route.is_detached());
        assert!(sink.emit(out("b")));
        assert!(sink.emit(TerminalEvent::Exited(None)));
        let (new, _, new_sink) = recorder();
        route.attach(new_sink);
        assert!(sink.emit(out("c")));
        assert_eq!(*old.lock().unwrap(), ["a"]);
        assert_eq!(*new.lock().unwrap(), ["b", "<exit>", "c"]);
    }

    #[test]
    fn a_failed_move_goes_back_to_the_old_window() {
        let (old, _, old_sink) = recorder();
        let route = Route::new(old_sink);
        let mut sink = route.sink();
        route.detach();
        assert!(sink.emit(out("held")));
        route.restore();
        assert!(sink.emit(out("after")));
        assert_eq!(*old.lock().unwrap(), ["held", "after"]);
    }

    #[test]
    fn a_closed_window_stops_the_terminal_unless_it_is_moving() {
        let (_, alive, sink0) = recorder();
        let route = Route::new(sink0);
        let mut sink = route.sink();
        *alive.lock().unwrap() = false;
        assert!(!sink.emit(out("x")), "no window: the terminal stops");

        let (_, alive, sink1) = recorder();
        let route = Route::new(sink1);
        let mut sink = route.sink();
        route.detach();
        *alive.lock().unwrap() = false;
        assert!(sink.emit(out("x")), "moving: output is held");
    }

    #[test]
    fn history_is_taken_once_and_clipped() {
        let routes = TerminalRoutes::default();
        assert_eq!(routes.take_history(7), None);
        routes.mark_moved(7, "hello");
        assert!(routes.is_moved(7));
        assert_eq!(routes.moved_ids(), [7]);
        assert_eq!(routes.take_history(7).as_deref(), Some("hello"));
        assert_eq!(
            routes.take_history(7).as_deref(),
            Some(""),
            "a reload starts empty"
        );
        routes.remove(7);
        assert!(!routes.is_moved(7));

        let long = format!("é{}", "x".repeat(MAX_HISTORY_BYTES));
        let clipped = clip_history(&long);
        assert!(clipped.len() <= MAX_HISTORY_BYTES);
        assert!(clipped.chars().all(|c| c == 'x'));
        assert_eq!(clip_history("short"), "short");
    }
}
