pub mod ring;

pub use ring::run_overlay;

/// Events from the overlay thread back to the daemon (pointer tracking while open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayEvent {
    /// Global screen coordinates from the layer surface while the ring is visible.
    Pointer { x: i32, y: i32 },
}
