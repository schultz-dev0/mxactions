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
    YdotoolInjector, parse_command,
};
pub use overlay::OverlayEvent;
pub use config::{
    config_path, load_or_init, parse_config_str, select_ring, Config, ConfigError, Ring,
    RingAction, UiSettings, DEFAULT_CONFIG_JSON,
};
pub use controller::{Controller, ControllerEvent, RingCommand};
pub use focus::{FocusSource, StaticFocus, WaylandFocus};
pub use pointer::query_pointer_position;
pub use geometry::{Hit, RingLayout, hit_test};
pub use hidpp::{HidError, HidEvent, HidEventSource, MockHid, MxMaster4, SENSE_PANEL_CID};
