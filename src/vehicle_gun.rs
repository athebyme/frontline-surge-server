//! Arcade gun mounts in the vehicle mesh's local axes (+Z forward, +X left).
pub const TURN_RATE: f32 = 1.28;
pub const MG_RANGE: f32 = 180.;

/// Playtest timings in map units. Keep the original maximum travel distance.
pub const TANK_RELOAD: f32 = 3.6;
pub const APC_RELOAD: f32 = 0.52;
pub const TANK_SHOT_SPEED: f32 = 246.;
pub const APC_SHOT_SPEED: f32 = 270.;
pub const MAX_SHOT_TRAVEL: f32 = 386.;

#[derive(Clone, Copy)]
pub struct Mount {
    pub pivot: [f32; 3],
    pub hinge: [f32; 3],
    pub mg_tip: [f32; 3],
    pub primary_tip: [f32; 3],
    pub minimum: f32,
    pub maximum: f32,
}

pub struct GunRay {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub pitch: f32,
}

impl Mount {
    pub fn for_kind(kind: &str) -> Option<Self> {
        match kind {
            "mbt" => Some(Self {
                pivot: [0., 1.73, 0.34],
                hinge: [0., 0.24, 0.96],
                mg_tip: [0.265, -0.065, 0.95],
                primary_tip: [0., 0., 2.31],
                // At full depression the lowest barrel ring still clears the rear deck.
                minimum: -0.105,
                maximum: 0.60,
            }),
            "apc" => Some(Self {
                pivot: [0., 2., 0.35],
                hinge: [0.12, 0.26, 0.22],
                mg_tip: [0., 0., 0.96],
                primary_tip: [0., 0., 0.96],
                minimum: -0.105,
                maximum: 0.70,
            }),
            _ => None,
        }
    }

    pub fn pitch(self, request: f32) -> f32 {
        request.clamp(self.minimum, self.maximum)
    }

    pub fn ray(
        self,
        x: f32,
        z: f32,
        hull: f32,
        turret: f32,
        request: f32,
        machinegun: bool,
    ) -> GunRay {
        let pitch = self.pitch(request);
        let (sp, cp) = pitch.sin_cos();
        let (sh, ch) = hull.sin_cos();
        let (st, ct) = turret.sin_cos();
        let socket = if machinegun {
            self.mg_tip
        } else {
            self.primary_tip
        };
        // Trace from the inner end of the visible barrel. A muzzle intersecting
        // a nearby wall must contact the wall even if the tip pokes through it.
        let side = self.hinge[0] + socket[0];
        let forward = self.hinge[2] - socket[1] * sp;
        GunRay {
            origin: [
                x - ch * self.pivot[0] + sh * self.pivot[2] - ct * side + st * forward,
                self.pivot[1] + self.hinge[1] + socket[1] * cp,
                z - sh * self.pivot[0] - ch * self.pivot[2] - st * side - ct * forward,
            ],
            direction: [st * cp, sp, -ct * cp],
            pitch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn articulated_barrels_clear_the_deck_at_full_depression() {
        for (kind, deck, radius) in [("mbt", 1.61, 0.108), ("apc", 2.08, 0.049)] {
            let mount = Mount::for_kind(kind).unwrap();
            for mg in [false, true] {
                let ray = mount.ray(0., 0., 1., -1., -1.28, mg);
                let tip = if mg { mount.mg_tip } else { mount.primary_tip };
                assert_eq!(ray.pitch, mount.minimum);
                assert!(ray.origin[1] + ray.direction[1] * tip[2] - radius > deck);
            }
        }
    }
}
