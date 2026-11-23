//! Open trail buggy in model space (+Z forward, +X toward the driver's seat).
//! The server and the optics both trace the drawn body instead of a solid cabin.
use crate::fracture::box_ray;

pub const SEAT: [f32; 3] = [0.31, 0.96, -0.09];
pub const EYE: [f32; 3] = [0.31, 1.50, -0.02];
pub const FLAG_SOCKET: [f32; 3] = [0.60, 1.41, -1.08];

pub fn world_point(x: f32, z: f32, yaw: f32, local: [f32; 3]) -> [f32; 3] {
    let (s, c) = yaw.sin_cos();
    [x - c * local[0] + s * local[2], local[1], z - s * local[0] - c * local[2]]
}

pub struct SightLine {
    origin: [f32; 3],
    dir: [f32; 3],
}

impl SightLine {
    pub fn new(x: f32, z: f32, yaw: f32, origin: [f32; 3], dir: [f32; 3]) -> Self {
        let (s, c) = yaw.sin_cos();
        let rotate = |[x, y, z]: [f32; 3]| [-c * x - s * z, y, s * x - c * z];
        Self { origin: rotate([origin[0] - x, origin[1], origin[2] - z]), dir: rotate(dir) }
    }

    pub fn hull(&self, limit: f32) -> Option<f32> {
        // Broad parts of the baked model. The spaces between them stay open.
        const PARTS: &[([f32; 3], [f32; 3])] = &[
            ([0., 0.60, 0.], [1.50, 0.42, 2.94]),
            ([0., 0.91, 1.04], [1.36, 0.21, 0.95]),
            ([0., 1.17, -0.88], [1.32, 0.49, 0.63]),
            ([0., 1.68, -0.24], [1.44, 0.08, 1.10]),
            ([0., 1.48, 0.37], [1.20, 0.30, 0.055]),
            ([-0.84, 0.43, 1.06], [0.25, 0.78, 0.76]),
            ([ 0.84, 0.43, 1.06], [0.25, 0.78, 0.76]),
            ([-0.84, 0.43,-1.05], [0.25, 0.78, 0.76]),
            ([ 0.84, 0.43,-1.05], [0.25, 0.78, 0.76]),
        ];
        box_ray([0., 0.91, 0.], [2., 1.82, 3.48], self.origin, self.dir, limit)?;
        let mut best = PARTS.iter().filter_map(|&(center, size)| {
            box_ray(center, size, self.origin, self.dir, limit)
        }).min_by(f32::total_cmp);
        // Sloping bars can stop a hit at the edge of the visible opening.
        for side in [-1., 1.] {
            for (x, low, high) in [
                (0.65, [0.63, 0.85], [1.69, 0.28]),
                (0.65, [0.77, -1.37], [1.65, -0.69]),
                (0.72, [0.78, 0.63], [0.99, -0.58]),
            ] {
                let (dy, dz): (f32, f32) = (high[0] - low[0], high[1] - low[1]);
                let length = dy.hypot(dz);
                let rotate = |[x, y, z]: [f32; 3]| [x, (y * dy + z * dz) / length, (z * dy - y * dz) / length];
                let o = rotate([self.origin[0] - side * x, self.origin[1] - (low[0] + high[0]) * 0.5,
                                self.origin[2] - (low[1] + high[1]) * 0.5]);
                if let Some(t) = box_ray([0.; 3], [0.08, length, 0.08], o, rotate(self.dir), best.unwrap_or(limit)) {
                    best = Some(t);
                }
            }
        }
        best
    }

    pub fn driver(&self, limit: f32) -> Option<f32> {
        [([SEAT[0], 1.21, SEAT[2]], [0.39, 0.49, 0.30]),
         ([EYE[0], 1.51, SEAT[2]], [0.25, 0.24, 0.27])]
            .into_iter().filter_map(|(center, size)| {
                box_ray(center, size, self.origin, self.dir, limit)
            }).min_by(f32::total_cmp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_open_seat_is_visible_from_the_side_and_covered_by_the_roof() {
        for yaw in [0., 1.3, -2.1] {
            let local_ray = |from, to| {
                let a = world_point(19., -7., yaw, from);
                let b = world_point(19., -7., yaw, to);
                SightLine::new(19., -7., yaw, a, std::array::from_fn(|i| b[i] - a[i]))
            };
            let side = local_ray([4., 1.50, -0.09], [3., 1.50, -0.09]);
            assert!(side.hull(9.).is_none());
            assert!(side.driver(9.).is_some());
            let roof = local_ray([0.31, 4., -0.09], [0.31, 3., -0.09]);
            assert!(roof.hull(9.).unwrap() < roof.driver(9.).unwrap());
            assert!(local_ray([-0.31, 1.25, 4.], [-0.31, 1.25, 3.]).driver(9.).is_none());
        }
    }
}
