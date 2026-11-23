use super::{Material, Prop};
use crate::rubble::{self, Piece};
impl Prop {
    pub(super) fn rebuild_rubble(&mut self) {
        let seed = rubble::hash(self.x.to_bits() ^ self.z.to_bits().rotate_left(11) ^ self.variant);
        let mut result = Vec::new();
        let world = |x: f32, z: f32| {
            [
                self.x + self.cs * x - self.sn * z,
                self.z + self.sn * x + self.cs * z,
            ]
        };
        let mut place = |id: u16, point: [f32; 3], step: [f32; 2], mat: Material| {
            let seed = rubble::hash(seed ^ u32::from(id));
            let from = world(point[0], point[2]);
            let finish = world(point[0] + step[0], point[2] + step[1]);
            result.push(rubble::encode(
                id,
                finish,
                [from[0], point[1], from[1]],
                seed,
                mat,
            ));
        };
        if let Some(layout) = &self.layout {
            // Keep the same cells when a late client joins or an unrelated wall changes.
            let mut ids: Vec<_> = self.fracture.broken.iter().copied().collect();
            if ids.len() > rubble::LIMIT {
                ids.select_nth_unstable_by_key(rubble::LIMIT, |&id| {
                    rubble::hash(seed ^ u32::from(id))
                });
                ids.truncate(rubble::LIMIT);
            }
            ids.sort_unstable();
            for id in ids {
                let cell = &layout.cells[usize::from(id)];
                let [x, _, z] = cell.center;
                let k = seed ^ u32::from(id);
                let spread = 0.35 + rubble::unit(k ^ 7) * 0.88;
                let tangent = (rubble::unit(k ^ 9) - 0.5) * 1.12;
                let step = if (x.abs() / self.w) > (z.abs() / self.d) {
                    [x.signum() * spread, tangent]
                } else {
                    [tangent, if z >= 0. { spread } else { -spread }]
                };
                place(id, cell.center, step, cell.material);
            }
        } else if self.dead && self.kind == "stone" {
            for id in 0..8 {
                let k = seed ^ u32::from(id);
                let a = rubble::unit(k ^ 7) * std::f32::consts::TAU;
                let r =
                    (0.2 + rubble::unit(k ^ 8) * 0.8) * self.w.max(self.d).clamp(0.6, 3.) * 0.66;
                place(
                    id,
                    [0., self.h * 0.42, 0.],
                    [a.cos() * r, a.sin() * r],
                    Material::Stone,
                );
            }
        }
        self.rubble = result;
    }
    pub(super) fn loose_footing(&self, x: f32, z: f32) -> bool {
        !self.rubble.is_empty()
            && (x - self.x).abs() <= self.world_rx + 2.
            && (z - self.z).abs() <= self.world_rz + 2.
            && self
                .rubble
                .iter()
                .any(|p: &Piece| rubble::underfoot(p, x, z))
    }
}
