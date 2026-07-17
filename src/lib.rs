pub mod action;
pub mod config;
pub mod controller;
pub mod focus;
pub mod geometry;
pub mod hidpp;
pub mod overlay;
pub mod pointer;

pub use action::{
    Action, ActionError, ActionRunner, ClickButton, InputInjector, RecordingInjector,
    SystemInjector, parse_command,
};
pub use config::{
    Config, ConfigError, ConfigReloader, DEFAULT_CONFIG_JSON, Ring, RingAction, TriggerMode,
    UiSettings, config_path, load_or_init, parse_config_str, select_ring,
};
pub use controller::{Controller, ControllerEvent, RingCommand};
pub use focus::{FocusSource, StaticFocus, WaylandFocus};
pub use geometry::{Hit, RingLayout, hit_test};
pub use hidpp::{HidError, HidEvent, HidEventSource, MockHid, MxMaster4, SENSE_PANEL_CID};
pub use overlay::OverlayEvent;
pub use pointer::query_pointer_position;
