//! Arcade area effects. Each payload reads one pre-impact scene and applies the
//! results once. Profile distances are map units tuned for this game's health scale.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Burst {
    Grenade,
    Tank,
    Apc,
}
struct Profile {
    radius: f32,
    peak: f32,
    falloff: f32,
    prop_range: f32,
    hull_range: f32,
}
impl Profile {
    fn damage_at(&self, distance: f32) -> f32 {
        if distance >= self.radius {
            return 0.;
        }
        (self.peak - distance * self.falloff).max(0.)
    }
}
impl Burst {
    pub(super) fn weapon(self) -> &'static str {
        match self {
            Self::Grenade => "grenade",
            Self::Tank => "tank",
            Self::Apc => "apc_he",
        }
    }
    pub(super) fn vehicle(self) -> Option<&'static str> {
        match self {
            Self::Grenade => None,
            Self::Tank => Some("mbt"),
            Self::Apc => Some("apc"),
        }
    }
    fn profile(self) -> Profile {
        let (radius, peak, falloff, prop_range, hull_range) = match self {
            Self::Grenade => (6.3, 124., 17.5, 3.6, 5.),
            Self::Tank => (8.0, 141., 16.5, 4.2, 5.5),
            Self::Apc => (4.5, 114., 23., 2.0, 3.0),
        };
        Profile {
            radius,
            peak,
            falloff,
            prop_range,
            hull_range,
        }
    }
}
impl Match {
    fn blast_clear(
        &self,
        o: [f32; 3],
        target: [f32; 3],
        allow_prop: Option<usize>,
        allow_vehicle: Option<usize>,
    ) -> bool {
        let delta = [target[0] - o[0], target[1] - o[1], target[2] - o[2]];
        let range = delta[0].hypot(delta[1]).hypot(delta[2]);
        if range < 0.08 {
            return true;
        }
        let dir = delta.map(|v| v / range);
        let (hit, _) = self.ray(o, dir, range - 0.04);
        if hit.is_some() && hit != allow_prop {
            return false;
        }
        self.logistics
            .ray(o, dir, range - 0.04, None)
            .map_or(true, |(k, _)| Some(k) == allow_vehicle)
    }
    pub(super) fn apply_burst(
        &mut self,
        p: &mut Player,
        o: [f32; 3],
        source: Burst,
        direct: Option<Contact>,
    ) {
        let spec = source.profile();
        // Compute visibility against the pre-blast scene. A wall broken by this
        // very explosion still shelters characters on its far side for that hit.
        let mut victims = vec![];
        for (&id, q) in &self.players {
            if !q.alive
                || q.team == p.team
                || Some(Contact::Player(id)) == direct
            {
                continue;
            }
            let target = match self.logistics.driver_vehicle(id) {
                None => [q.x, 0.95, q.z],
                Some(v) => match self.logistics.exposed_driver_target(v) {
                    Some(eye) => eye,
                    None => continue,
                },
            };
            let dist = (target[0] - o[0]).hypot(target[2] - o[2]).hypot(target[1] - o[1]);
            if dist < spec.radius && self.blast_clear(o, target, None, None) {
                victims.push((id, spec.damage_at(dist)));
            }
        }
        let mut objects = vec![];
        for (i, obj) in self.props.iter().enumerate() {
            if Some(Contact::Prop(i)) == direct
                || obj.dead
                || (!obj.solid)
                || obj.footprint(o[0], o[2]) > spec.prop_range.powi(2)
            {
                continue;
            }
            let local = obj.local(o);
            let clamp_box = |c: [f32; 3], e: [f32; 3]| {
                [
                    local[0].clamp(c[0] - e[0] * 0.5, c[0] + e[0] * 0.5),
                    local[1].clamp(c[1] - e[1] * 0.5, c[1] + e[1] * 0.5),
                    local[2].clamp(c[2] - e[2] * 0.5, c[2] + e[2] * 0.5),
                ]
            };
            let dist = |a: [f32; 3]| {
                (a[0] - local[0])
                    .hypot(a[2] - local[2])
                    .hypot(a[1] - local[1])
            };
            let near = if let Some(layout) = &obj.layout {
                layout
                    .cells
                    .iter()
                    .enumerate()
                    .filter(|(n, _)| !obj.fracture.broken.contains(&(*n as u16)))
                    .map(|(id,c)| obj.fracture.clipped(id as u16, *c))
                    .map(|c| clamp_box(c.center, c.size))
                    .min_by(|a, b| dist(*a).total_cmp(&dist(*b)))
            } else {
                Some(clamp_box(
                    [0., obj.h * 0.5, 0.],
                    [
                        obj.collision_w.unwrap_or(obj.w),
                        obj.h,
                        obj.collision_d.unwrap_or(obj.d),
                    ],
                ))
            };
            if let Some(a) = near {
                let distance = dist(a);
                if distance < spec.prop_range {
                    let at = [
                        obj.x + obj.cs * a[0] - obj.sn * a[2],
                        a[1],
                        obj.z + obj.sn * a[0] + obj.cs * a[2],
                    ];
                    if self.blast_clear(o, at, Some(i), None) {
                        objects.push((i, obj.build_id, at, 90. - distance * 14.));
                    }
                }
            }
        }
        let hulls: Vec<_> = self
            .logistics
            .blast_targets(o[0], o[2], spec.hull_range)
            .into_iter()
            .filter(|(i, x, y, z)| {
                Some(Contact::Vehicle(*i)) != direct
                    && self.blast_clear(o, [*x, *y, *z], None, Some(*i))
            })
            .collect();
        for (id, amount) in victims {
            self.harm(p, id, amount, source.weapon(), false, source.vehicle());
        }
        for (i, x, y, z) in hulls {
            let dist = (x - o[0]).hypot(z - o[2]).hypot(y - o[1]);
            let impact = object_damage(
                self.logistics.hull_kind(i).unwrap_or(""),
                None,
                Damage::Grenade,
                (86. - dist * 12.).max(0.),
            );
            let ev = self.logistics.hit(i, impact, p.id, &p.team);
            self.ingest_logistics(ev, p);
        }
        for (i, expected, at, hp) in objects {
            if self.props[i].build_id == expected {
                self.damage_prop(i, at, Damage::Grenade, hp, p);
            }
        }
    }
}
