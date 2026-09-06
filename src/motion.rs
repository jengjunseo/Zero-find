//! Time based, retargetable geometry. No timer is needed once settled.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub width: f64,
    pub height: f64,
    pub target_width: f64,
    pub target_height: f64,
}
impl Motion {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            target_width: width,
            target_height: height,
        }
    }
    pub fn step(&mut self, seconds: f64, reduced: bool) -> bool {
        let blend = if reduced {
            1.0
        } else {
            1.0 - (-seconds.min(0.1) * 32.0).exp()
        };
        self.width += (self.target_width - self.width) * blend;
        self.height += (self.target_height - self.height) * blend;
        let settled = (self.width - self.target_width).abs() < 0.5
            && (self.height - self.target_height).abs() < 0.5;
        if settled {
            self.width = self.target_width;
            self.height = self.target_height;
        }
        settled
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reversal_continues_from_current_geometry_and_settles() {
        let mut m = Motion::new(720., 120.);
        m.target_width = 64.;
        m.step(0.016, false);
        let current = m.width;
        m.target_width = 720.;
        m.step(0.016, false);
        assert!(m.width > current && m.width < 720.);
        for _ in 0..60 {
            m.step(0.016, false);
        }
        assert_eq!(m.width, 720.);
    }
    #[test]
    fn reduced_motion_is_immediate() {
        let mut m = Motion::new(64., 64.);
        m.target_height = 500.;
        assert!(m.step(0., true));
        assert_eq!(m.height, 500.);
    }
}
