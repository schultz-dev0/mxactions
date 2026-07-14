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

impl RingLayout {
    pub fn new(action_count: usize, ring_radius: f32) -> Self {
        let hub_radius = ring_radius * 0.3;
        let bubble_radius = ring_radius * 0.28;
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
}
