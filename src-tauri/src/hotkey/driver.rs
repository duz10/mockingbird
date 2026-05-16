//! State-machine driver thread.
//!
//! The driver owns the [`HotkeyStateMachine`] and bridges the input
//! channel from [`super::HotkeyListener`] to an output channel of
//! [`StateAction`]s for the orchestrator.
//!
//! ## Cadence
//!
//! The driver loops on `Receiver::recv_timeout(20 ms)`. On timeout it
//! synthesises a [`HotkeyEvent::Tick { at: Instant::now() }`] and
//! hands it to the machine. 20 ms gives ≥ 4 ticks inside the 80 ms
//! `hold_threshold` (plenty of resolution) and 15 ticks inside the
//! 300 ms LL-hook watchdog window (so a tick-based watchdog log
//! has fresh data on every check).
//!
//! ## Shutdown
//!
//! When the listener's `Sender` is dropped, `recv_timeout` returns
//! `Disconnected` and the loop exits cleanly. No special "stop"
//! signal is needed — closing the channel IS the signal.
//!
//! ## Tests
//!
//! [`drive_until_disconnected`] is exposed publicly so tests can run
//! the loop with a manually-driven channel; the OS-side [`run_in_thread`]
//! is the thin wrapper for production.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::state::{HotkeyStateMachine, StateAction, StateConfig};
use super::HotkeyEvent;

/// Default 20 ms driver cadence (PLAN §6.1 + ADR 0015 §3 watchdog).
pub const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(20);

/// Builder/owner for the driver thread.
///
/// Construct, call [`Self::start`] with the listener's `Receiver`, and
/// receive `StateAction`s on the returned `Receiver`. Join via
/// [`DriverHandle::stop`] (or let `Drop` close the action channel
/// once you're done consuming).
pub struct StateDriver {
    config: StateConfig,
    tick_interval: Duration,
}

/// Handle to a running driver.
pub struct DriverHandle {
    join: Option<JoinHandle<()>>,
}

impl DriverHandle {
    /// Wait for the driver to finish. The driver exits naturally when
    /// the input channel closes; this just joins.
    pub fn stop(mut self) {
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for DriverHandle {
    fn drop(&mut self) {
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Default for StateDriver {
    fn default() -> Self {
        Self::new(StateConfig::default(), DEFAULT_TICK_INTERVAL)
    }
}

impl StateDriver {
    /// Construct a driver with explicit config + cadence. Tests use
    /// this; production uses [`Default`].
    pub fn new(config: StateConfig, tick_interval: Duration) -> Self {
        Self {
            config,
            tick_interval,
        }
    }

    /// Spawn the driver on a new OS thread named `mockingbird-state`.
    ///
    /// Returns the action channel and a handle to join the thread.
    pub fn start(self, events: Receiver<HotkeyEvent>) -> (Receiver<StateAction>, DriverHandle) {
        let (action_tx, action_rx) = mpsc::channel();
        let config = self.config;
        let tick = self.tick_interval;

        let join = thread::Builder::new()
            .name("mockingbird-state".into())
            .spawn(move || {
                drive_until_disconnected(HotkeyStateMachine::new(config), events, action_tx, tick);
            })
            .expect("OS thread spawn must succeed for state driver");

        (action_rx, DriverHandle { join: Some(join) })
    }
}

/// The actual loop. Pulled out for testability — call directly from a
/// unit test with handcrafted channels.
///
/// Exits when `events` returns `RecvTimeoutError::Disconnected`.
pub fn drive_until_disconnected(
    mut machine: HotkeyStateMachine,
    events: Receiver<HotkeyEvent>,
    actions: Sender<StateAction>,
    tick_interval: Duration,
) {
    loop {
        let ev = match events.recv_timeout(tick_interval) {
            Ok(ev) => ev,
            Err(RecvTimeoutError::Timeout) => HotkeyEvent::Tick { at: Instant::now() },
            Err(RecvTimeoutError::Disconnected) => break,
        };

        let action = machine.handle(ev);
        if !matches!(action, StateAction::None) && actions.send(action).is_err() {
            // Consumer dropped — no point keeping the machine alive.
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::state::{HotkeyMode, StateConfig};
    use std::time::Duration;

    const VK: u32 = 0xA5;

    fn spawn_test_driver(
        config: StateConfig,
        tick: Duration,
    ) -> (Sender<HotkeyEvent>, Receiver<StateAction>, DriverHandle) {
        let (ev_tx, ev_rx) = mpsc::channel();
        let (action_rx, handle) = StateDriver::new(config, tick).start(ev_rx);
        (ev_tx, action_rx, handle)
    }

    #[test]
    fn ticks_alone_produce_no_actions() {
        // Without any real events the driver just synthesises ticks
        // and never escalates — IDLE stays IDLE.
        let (ev_tx, action_rx, handle) =
            spawn_test_driver(StateConfig::default(), Duration::from_millis(5));
        std::thread::sleep(Duration::from_millis(60));
        // Drop the sender — driver exits.
        drop(ev_tx);
        handle.stop();
        // Drain — must be empty.
        assert!(
            action_rx.try_recv().is_err(),
            "no actions should have fired from pure-tick IDLE"
        );
    }

    #[test]
    fn keydown_then_wait_triggers_start_capture() {
        // Real flow: hook sends KeyDown; driver ticks past 80 ms;
        // state machine emits StartCapture(Normal); driver forwards.
        let (ev_tx, action_rx, handle) =
            spawn_test_driver(StateConfig::default(), Duration::from_millis(5));
        ev_tx
            .send(HotkeyEvent::KeyDown {
                vk: VK,
                at: Instant::now(),
            })
            .unwrap();
        // Wait for the driver to tick past the 80 ms hold threshold.
        let action = action_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("StartCapture should arrive");
        assert_eq!(action, StateAction::StartCapture(HotkeyMode::Normal));

        drop(ev_tx);
        handle.stop();
    }

    #[test]
    fn keydown_then_keyup_round_trips_to_stop_capture() {
        let (ev_tx, action_rx, handle) =
            spawn_test_driver(StateConfig::default(), Duration::from_millis(5));

        let t0 = Instant::now();
        ev_tx.send(HotkeyEvent::KeyDown { vk: VK, at: t0 }).unwrap();
        // Sleep long enough for the driver to cross hold_threshold.
        std::thread::sleep(Duration::from_millis(150));

        ev_tx
            .send(HotkeyEvent::KeyUp {
                vk: VK,
                at: Instant::now(),
            })
            .unwrap();

        // Drain — first action should be StartCapture, second StopCapture.
        let mut got: Vec<StateAction> = Vec::new();
        for _ in 0..2 {
            if let Ok(a) = action_rx.recv_timeout(Duration::from_millis(500)) {
                got.push(a);
            }
        }
        assert!(
            got.contains(&StateAction::StartCapture(HotkeyMode::Normal)),
            "expected StartCapture in {got:?}"
        );
        assert!(
            got.contains(&StateAction::StopCapture),
            "expected StopCapture in {got:?}"
        );

        drop(ev_tx);
        handle.stop();
    }

    #[test]
    fn tap_under_threshold_emits_nothing() {
        // The shortest legal hold is hold_threshold (clamped to 40 ms).
        // A tap below that should yield no action.
        let (ev_tx, action_rx, handle) =
            spawn_test_driver(StateConfig::default(), Duration::from_millis(5));
        let t = Instant::now();
        ev_tx.send(HotkeyEvent::KeyDown { vk: VK, at: t }).unwrap();
        ev_tx
            .send(HotkeyEvent::KeyUp {
                vk: VK,
                at: t + Duration::from_millis(30),
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        drop(ev_tx);
        handle.stop();
        assert!(
            action_rx.try_recv().is_err(),
            "tap should not produce actions"
        );
    }

    #[test]
    fn channel_close_exits_loop_cleanly() {
        // Smoke: if we drop the sender immediately, the driver thread
        // exits within one tick_interval — no hang.
        let (ev_tx, _action_rx, handle) =
            spawn_test_driver(StateConfig::default(), Duration::from_millis(5));
        drop(ev_tx);
        // The Drop on DriverHandle joins; if this returns we're good.
        let start = Instant::now();
        handle.stop();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "driver shutdown took too long: {elapsed:?}"
        );
    }

    #[test]
    fn pause_toggle_blocks_subsequent_keydown() {
        let (ev_tx, action_rx, handle) =
            spawn_test_driver(StateConfig::default(), Duration::from_millis(5));
        ev_tx
            .send(HotkeyEvent::PauseToggle { paused: true })
            .unwrap();
        // Wait briefly for the PauseToggle to be consumed.
        std::thread::sleep(Duration::from_millis(20));
        ev_tx
            .send(HotkeyEvent::KeyDown {
                vk: VK,
                at: Instant::now(),
            })
            .unwrap();
        // Sleep past hold_threshold — nothing should fire.
        std::thread::sleep(Duration::from_millis(120));
        drop(ev_tx);
        handle.stop();
        assert!(
            action_rx.try_recv().is_err(),
            "paused state should suppress KeyDown"
        );
    }
}
