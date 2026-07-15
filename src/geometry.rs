#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Hub,
    Bubble(usize),
    Miss,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RingLayout {
    pub hub_radius: f32,
    pub bubble_radius: f32,
    pub bubbles: Vec<(f32, f32)>, // centers
}

/// Hub is a small fixed-ratio marker dot — it does not grow with action count.
const HUB_RADIUS_RATIO: f32 = 0.12;
/// v1's bubble size; kept as the ceiling so small rings look unchanged.
const BUBBLE_RADIUS_RATIO: f32 = 0.28;

impl RingLayout {
    pub fn new(action_count: usize, ring_radius: f32) -> Self {
        let hub_radius = ring_radius * HUB_RADIUS_RATIO;
        let base_bubble_radius = ring_radius * BUBBLE_RADIUS_RATIO;
        let bubble_radius = if action_count <= 1 {
            base_bubble_radius
        } else {
            // Shrink bubbles only once they'd otherwise overlap their neighbors,
            // so a config with more than the old bubble_count_max=8 still fits
            // instead of being silently truncated.
            let half_angle = std::f32::consts::PI / action_count as f32;
            let max_by_spacing = ring_radius * half_angle.sin() * 0.9;
            base_bubble_radius.min(max_by_spacing)
        };
        let bubbles = if action_count == 0 {
            vec![]
        } else {
            (0..action_count)
                .map(|i| {
                    let a = (i as f32) * std::f32::consts::TAU / action_count as f32
                        - std::f32::consts::FRAC_PI_2;
                    (ring_radius * a.cos(), ring_radius * a.sin())
                })
                .collect()
        };
        Self { hub_radius, bubble_radius, bubbles }
    }
}

pub fn hit_test(layout: &RingLayout, x: f32, y: f32) -> Hit {
    for (i, (bx, by)) in layout.bubbles.iter().enumerate() {
        let dx = x - bx;
        let dy = y - by;
        if dx * dx + dy * dy <= layout.bubble_radius * layout.bubble_radius {
            return Hit::Bubble(i);
        }
    }
    if x * x + y * y <= layout.hub_radius * layout.hub_radius {
        return Hit::Hub;
    }
    Hit::Miss
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_top_bubble_and_hub() {
        let layout = RingLayout::new(4, 100.0);
        // first bubble at angle -PI/2 → (0, -100)
        assert_eq!(hit_test(&layout, 0.0, -100.0), Hit::Bubble(0));
        assert_eq!(hit_test(&layout, 0.0, 0.0), Hit::Hub);
        assert_eq!(hit_test(&layout, 200.0, 200.0), Hit::Miss);
    }

    #[test]
    fn bubbles_never_overlap_regardless_of_count() {
        for n in 1..=20usize {
            let layout = RingLayout::new(n, 120.0);
            if n <= 1 {
                continue; // no neighbor to overlap with
            }
            let half_angle = std::f32::consts::PI / n as f32;
            let chord = 2.0 * 120.0 * half_angle.sin();
            assert!(
                chord >= 2.0 * layout.bubble_radius - 0.01,
                "n={n}: chord {chord} too small for bubble_radius {}",
                layout.bubble_radius
            );
        }
    }

    #[test]
    fn small_counts_keep_the_v1_bubble_size() {
        // At counts the old bubble_count_max=8 default already handled comfortably,
        // sizing must be unchanged from the v1 constant (0.28 * ring_radius).
        let layout = RingLayout::new(3, 120.0);
        assert!((layout.bubble_radius - 120.0 * 0.28).abs() < 0.001);
    }
}
