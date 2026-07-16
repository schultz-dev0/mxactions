pub trait FocusSource {
    fn focused_app_id(&self) -> Option<String>;
}

#[derive(Debug, Default, Clone)]
pub struct StaticFocus(pub Option<String>);

impl FocusSource for StaticFocus {
    fn focused_app_id(&self) -> Option<String> {
        self.0.clone()
    }
}

mod wayland_impl {
    use std::collections::HashMap;

    use wayland_client::backend::ObjectId;
    use wayland_client::{
        Connection, Dispatch, EventQueue, Proxy, QueueHandle, event_created_child,
        globals::registry_queue_init, protocol::wl_registry,
    };
    use wayland_protocols_wlr::foreign_toplevel::v1::client::{
        zwlr_foreign_toplevel_handle_v1::{
            Event as HandleEvent, State as HandleState, ZwlrForeignToplevelHandleV1,
        },
        zwlr_foreign_toplevel_manager_v1::{Event as ManagerEvent, ZwlrForeignToplevelManagerV1},
    };

    #[derive(Debug, Default)]
    pub(super) struct WlFocusState {
        pub active: Option<ObjectId>,
        pub app_ids: HashMap<ObjectId, String>,
    }

    /// Tracks focused Wayland toplevel via `zwlr_foreign_toplevel_manager_v1`.
    ///
    /// Works on wlroots-based compositors (Sway, Hyprland, River, etc.). GNOME (Mutter)
    /// and KDE (KWin) typically do not expose this protocol; [`focused_app_id`](super::FocusSource::focused_app_id)
    /// then returns `None` and ring matching falls back to the Desktop (`*`) ring.
    ///
    /// The newer `ext-foreign-toplevel-list-v1` protocol is list-only (no activated state);
    /// focus tracking requires `zwlr_foreign_toplevel_manager_v1` or a future
    /// `ext-foreign-toplevel-state` extension bound to ext handles.
    #[derive(Debug)]
    pub struct WaylandFocus {
        state: WlFocusState,
        queue: Option<EventQueue<WlFocusState>>,
        available: bool,
        connect_attempted: bool,
    }

    impl Default for WaylandFocus {
        fn default() -> Self {
            Self::new()
        }
    }

    impl WaylandFocus {
        pub fn new() -> Self {
            Self {
                state: WlFocusState::default(),
                queue: None,
                available: false,
                connect_attempted: false,
            }
        }

        /// Whether `zwlr_foreign_toplevel_manager_v1` was bound on this compositor.
        pub fn is_available(&self) -> bool {
            self.available
        }

        /// Poll the Wayland connection for focus changes. Call from the daemon loop
        /// (e.g. alongside HID event polling).
        pub fn poll(&mut self) {
            if !self.ensure_connected() {
                return;
            }
            let Some(queue) = self.queue.as_mut() else {
                return;
            };
            if queue.roundtrip(&mut self.state).is_err() {
                log::debug!("Wayland focus roundtrip failed; reconnecting");
                self.reset_connection();
                let _ = self.ensure_connected();
            }
        }

        fn reset_connection(&mut self) {
            self.queue = None;
            self.available = false;
            self.connect_attempted = false;
            self.state = WlFocusState::default();
        }

        fn ensure_connected(&mut self) -> bool {
            if self.available {
                return true;
            }
            if self.connect_attempted {
                return false;
            }
            self.connect_attempted = true;

            let connection = match Connection::connect_to_env() {
                Ok(c) => c,
                Err(err) => {
                    log::debug!("Wayland unavailable for focus tracking: {err}");
                    return false;
                }
            };

            let (globals, mut queue) = match registry_queue_init::<WlFocusState>(&connection) {
                Ok(init) => init,
                Err(err) => {
                    log::debug!("Wayland registry init failed: {err}");
                    return false;
                }
            };

            if globals
                .bind::<ZwlrForeignToplevelManagerV1, _, _>(&queue.handle(), 1..=3, ())
                .is_err()
            {
                log::debug!("zwlr_foreign_toplevel_manager_v1 not advertised by compositor");
                return false;
            }

            if queue.roundtrip(&mut self.state).is_err() {
                log::debug!("Wayland focus initial roundtrip failed");
                return false;
            }

            self.queue = Some(queue);
            self.available = true;
            log::debug!("Wayland focus tracker connected");
            true
        }
    }

    impl super::FocusSource for WaylandFocus {
        fn focused_app_id(&self) -> Option<String> {
            let id = self.state.active.as_ref()?;
            let app_id = self.state.app_ids.get(id)?.trim();
            if app_id.is_empty() {
                None
            } else {
                Some(app_id.to_string())
            }
        }
    }

    impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents>
        for WlFocusState
    {
        fn event(
            _state: &mut Self,
            _registry: &wl_registry::WlRegistry,
            _event: wl_registry::Event,
            _data: &wayland_client::globals::GlobalListContents,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WlFocusState {
        fn event(
            state: &mut Self,
            _: &ZwlrForeignToplevelManagerV1,
            event: ManagerEvent,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let ManagerEvent::Toplevel { toplevel } = event {
                state.app_ids.entry(toplevel.id()).or_default();
            }
        }

        event_created_child!(WlFocusState, ZwlrForeignToplevelManagerV1, [
            _ => (ZwlrForeignToplevelHandleV1, ())
        ]);
    }

    impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WlFocusState {
        fn event(
            state: &mut Self,
            handle: &ZwlrForeignToplevelHandleV1,
            event: HandleEvent,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                HandleEvent::AppId { app_id } => {
                    state.app_ids.insert(handle.id(), app_id);
                }
                HandleEvent::Closed => {
                    if state.active.as_ref() == Some(&handle.id()) {
                        state.active = None;
                    }
                    state.app_ids.remove(&handle.id());
                }
                HandleEvent::State {
                    state: handle_state,
                } => {
                    let activated = handle_state.contains(&(HandleState::Activated as u8));
                    if activated {
                        state.active = Some(handle.id());
                    } else if state.active.as_ref() == Some(&handle.id()) {
                        state.active = None;
                    }
                }
                _ => {}
            }
        }
    }
}

pub use wayland_impl::WaylandFocus;

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Serializes env manipulation so parallel `cargo test` does not race on
    /// `WAYLAND_DISPLAY` / `XDG_RUNTIME_DIR`.
    static WAYLAND_ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Hide Wayland session env vars so unit tests never talk to a real compositor.
    struct NoWaylandDisplay {
        _env_lock: std::sync::MutexGuard<'static, ()>,
        saved_display: Option<String>,
        saved_socket: Option<String>,
        saved_runtime: Option<String>,
        _temp: tempfile::TempDir,
    }

    impl NoWaylandDisplay {
        fn new() -> Self {
            let env_lock = WAYLAND_ENV_LOCK.lock().expect("env lock");
            let temp = tempfile::tempdir().expect("tempdir");
            let saved_display = std::env::var("WAYLAND_DISPLAY").ok();
            let saved_socket = std::env::var("WAYLAND_SOCKET").ok();
            let saved_runtime = std::env::var("XDG_RUNTIME_DIR").ok();
            // SAFETY: guarded by WAYLAND_ENV_LOCK; no concurrent env access in tests.
            unsafe {
                std::env::remove_var("WAYLAND_DISPLAY");
                std::env::remove_var("WAYLAND_SOCKET");
                std::env::set_var("XDG_RUNTIME_DIR", temp.path());
            }
            Self {
                _env_lock: env_lock,
                saved_display,
                saved_socket,
                saved_runtime,
                _temp: temp,
            }
        }
    }

    impl Drop for NoWaylandDisplay {
        fn drop(&mut self) {
            unsafe {
                match &self.saved_display {
                    Some(val) => std::env::set_var("WAYLAND_DISPLAY", val),
                    None => std::env::remove_var("WAYLAND_DISPLAY"),
                }
                match &self.saved_socket {
                    Some(val) => std::env::set_var("WAYLAND_SOCKET", val),
                    None => std::env::remove_var("WAYLAND_SOCKET"),
                }
                match &self.saved_runtime {
                    Some(val) => std::env::set_var("XDG_RUNTIME_DIR", val),
                    None => std::env::remove_var("XDG_RUNTIME_DIR"),
                }
            }
        }
    }

    #[test]
    fn static_focus_returns_configured_app_id() {
        let focus = StaticFocus(Some("cursor".into()));
        assert_eq!(focus.focused_app_id().as_deref(), Some("cursor"));
    }

    #[test]
    fn wayland_focus_without_display_returns_none() {
        let _guard = NoWaylandDisplay::new();
        let focus = WaylandFocus::new();
        assert!(!focus.is_available());
        assert!(focus.focused_app_id().is_none());
    }

    #[test]
    fn wayland_focus_poll_without_display_is_noop() {
        let _guard = NoWaylandDisplay::new();
        let mut focus = WaylandFocus::new();
        focus.poll();
        assert!(!focus.is_available());
        assert!(focus.focused_app_id().is_none());
    }

    /// Manual check on a wlroots compositor: `cargo test -q focus_live -- --ignored --nocapture`
    #[test]
    #[ignore = "requires zwlr_foreign_toplevel_manager_v1 on a live Wayland session"]
    fn wayland_focus_live_session() {
        let mut focus = WaylandFocus::new();
        focus.poll();
        if focus.is_available() {
            eprintln!("focused app_id: {:?}", focus.focused_app_id());
        } else {
            eprintln!("foreign-toplevel protocol unavailable on this compositor");
        }
    }
}
