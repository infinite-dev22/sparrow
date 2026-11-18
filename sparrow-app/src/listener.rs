use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};

/// A signal listener that receives 'events' from the outside and runs the callback.  
/// This is implemented using effects and normal rw signals. This should be used when it doesn't  
/// make sense to think of it as 'storing' a value, like an `RwSignal` would typically be used for.
///  
/// Copied/Cloned listeners refer to the same listener.
#[derive(Debug)]
pub struct Listener<T: 'static> {
    cx: Scope,
    val: RwSignal<Option<T>>,
}

impl<T: Clone + 'static> Listener<T> {
    pub fn new(cx: Scope, on_val: impl Fn(T) + 'static) -> Listener<T> {
        let val = cx.create_rw_signal(None);

        let listener = Listener { val, cx };
        listener.listen(on_val);

        listener
    }

    /// Construct a listener when you can't yet give it a callback.  
    /// Call `listen` to set a callback.
    pub fn new_empty(cx: Scope) -> Listener<T> {
        let val = cx.create_rw_signal(None);
        Listener { val, cx }
    }

    pub fn scope(&self) -> Scope {
        self.cx
    }

    /// Listen for values sent to this listener.      
    pub fn listen(self, on_val: impl Fn(T) + 'static) {
        self.listen_with(self.cx, on_val)
    }

    /// Listen for values sent to this listener.  
    /// Allows creating the effect with a custom scope, letting it be disposed of.  
    pub fn listen_with(self, cx: Scope, on_val: impl Fn(T) + 'static) {
        let val = self.val;

        cx.create_effect(move |_| {
            // TODO(minor): Signals could have a `take` method to avoid cloning.
            if let Some(cmd) = val.get() {
                // Commands are events, not derived state. Reading a signal in
                // their callback must not subscribe and replay the command.
                floem::reactive::untrack(|| on_val(cmd));
            }
        });
    }

    /// Send a value to the listener.
    pub fn send(&self, v: T) {
        self.val.set(Some(v));
    }
}

impl<T: 'static> Copy for Listener<T> {}

impl<T: 'static> Clone for Listener<T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_state_changes_never_replay_a_command() {
        let cx = Scope::new();
        let state = cx.create_rw_signal(0);
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = count.clone();
        let listener = Listener::new(cx, move |_: &'static str| {
            observed.set(observed.get() + 1);
            if state.get() == 0 {
                state.set(1);
            }
        });
        listener.send("run");
        assert_eq!(count.get(), 1);
        state.set(2);
        assert_eq!(count.get(), 1);
        listener.send("stop");
        assert_eq!(count.get(), 2);
        cx.dispose();
    }
}
