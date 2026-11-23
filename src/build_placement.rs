//! Server-owned barricade joints and the recent previews a builder can confirm.
use super::{Match, Player, Prop, EYE, RADIUS};
use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;

const SNAP_RADIUS: f32 = 1.08;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Joint {
    pub id: u32,
    pub kind: &'static str,
    pub x: f32,
    pub z: f32,
}

pub(super) struct Preview {
    pub wall: Prop,
    pub support: Option<usize>,
    pub joint: Option<Joint>,
    pub clear: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pose {
    x: f32,
    z: f32,
    yaw: f32,
    kind: &'static str,
    support: Option<u32>,
    joint: Option<Joint>,
}
impl Pose {
    pub(super) fn new(w: &Prop, support: Option<u32>, joint: Option<Joint>) -> Self {
        let kind = match w.kind.as_str() {
            "refinery" => "refinery",
            "workshop" => "workshop",
            _ => "barricade",
        };
        Self {
            x: w.x,
            z: w.z,
            yaw: w.yaw,
            kind,
            support,
            joint,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub(super) struct History {
    serial: u64,
    recent: VecDeque<(u64, f32, Pose)>,
}
impl History {
    pub(super) fn remember(&mut self, pose: Pose, now: f32) -> u64 {
        if let Some((id, at, old)) = self.recent.back_mut() {
            if *old == pose {
                *at = now;
                return *id;
            }
        }
        self.serial += 1;
        if self.recent.len() == 40 {
            self.recent.pop_front();
        }
        self.recent.push_back((self.serial, now, pose));
        self.serial
    }
    pub(super) fn find(&self, id: u64, now: f32) -> Option<Pose> {
        self.recent
            .iter()
            .rev()
            .find(|(key, at, _)| *key == id && now - at <= 0.65)
            .map(|e| e.2)
    }
    pub(super) fn clear(&mut self) {
        self.recent.clear();
    }
}
#[derive(Clone, Copy)]
struct Candidate {
    x: f32,
    z: f32,
    yaw: f32,
    rank: f32,
    joint: Joint,
}

impl Match {
    pub(super) fn build_preview(&self, p: &Player) -> Preview {
        let (sn, cs) = p.yaw.sin_cos();
        let kind = p.build_kind.as_str();
        let (w, d, h, hp) = match kind {
            "refinery" => (3.3, 2.6, 3.1, 260.),
            "workshop" => (3.8, 3., 2.5, 300.),
            _ => (3.1, 0.6, 1.33, 140.),
        };
        let gap = 2.2 + d * 0.5;
        let mut build = Prop {
            kind: if kind == "wall" { "barricade" } else { kind }.into(),
            x: p.x + sn * gap,
            z: p.z - cs * gap,
            yaw: p.yaw,
            w,
            d,
            h,
            solid: true,
            built: true,
            team: Some(p.team.clone()),
            hp: Some(hp),
            max_hp: Some(hp),
            ..Default::default()
        };
        let support = if kind == "wall" {
            self.wall_support(p)
        } else {
            None
        };
        build.level = 1;
        if let Some(i) = support {
            let base = &self.props[i];
            build.x = base.x;
            build.z = base.z;
            build.yaw = base.yaw;
            build.level = 2;
        }

        build.prepare_geometry();
        if kind == "wall" && support.is_none() {
            let joints = self.nearby_joints(&build);
            let mut first = None;
            for c in joints {
                build.x = c.x;
                build.z = c.z;
                build.yaw = c.yaw;
                build.prepare_geometry();
                if self.site_clear(p, &build, None) {
                    return Preview {
                        wall: build,
                        support: None,
                        joint: Some(c.joint),
                        clear: true,
                    };
                }
                first.get_or_insert(c);
            }
            if let Some(c) = first {
                build.x = c.x;
                build.z = c.z;
                build.yaw = c.yaw;
                build.prepare_geometry();
                return Preview {
                    wall: build,
                    support: None,
                    joint: Some(c.joint),
                    clear: false,
                };
            }
        }
        let clear = self.site_clear(p, &build, support);
        Preview {
            wall: build,
            support,
            joint: None,
            clear,
        }
    }
    fn nearby_joints(&self, raw: &Prop) -> Vec<Candidate> {
        let mut out = Vec::new();
        for base in self.props[self.level.props.len()..].iter().filter(|q| {
            q.built
                && !q.dead
                && q.kind == "barricade"
                && q.team == raw.team
                && (q.x - raw.x).abs() < 5.
                && (q.z - raw.z).abs() < 5.
        }) {
            let Some(id) = base.build_id else { continue };
            let world = |along: f32, across: f32| {
                (
                    base.x + base.cs * along - base.sn * across,
                    base.z + base.sn * along + base.cs * across,
                )
            };
            let mut add =
                |along: f32, across: f32, yaw: f32, end: (f32, f32), kind: &'static str| {
                    let (x, z) = world(along, across);
                    let dist2 = (x - raw.x).powi(2) + (z - raw.z).powi(2);
                    if dist2 > SNAP_RADIUS * SNAP_RADIUS {
                        return;
                    }
                    let alignment = (yaw - raw.yaw).cos().abs();
                    let (jx, jz) = world(end.0, end.1);
                    if !base.fracture.broken.is_empty() {
                        // A breached endpoint has no surviving face to attach to.
                        let dx = base.x - jx;
                        let dz = base.z - jz;
                        let len = dx.hypot(dz);
                        let dir = [dx / len, 0., dz / len];
                        if base
                            .ray([jx - dir[0] * 0.06, 0.42, jz - dir[2] * 0.06], dir, 0.15)
                            .is_none()
                        {
                            return;
                        }
                    }
                    out.push(Candidate {
                        x,
                        z,
                        yaw,
                        rank: dist2 + (1. - alignment) * 0.42,
                        joint: Joint {
                            id,
                            kind,
                            x: jx,
                            z: jz,
                        },
                    });
                };
            for sign in [-1., 1.] {
                let end = sign * base.w * 0.5;
                add(
                    end + sign * raw.w * 0.5,
                    0.,
                    base.yaw,
                    (end, 0.),
                    "straight",
                );
                add(
                    0.,
                    sign * (base.d + raw.d) * 0.5,
                    base.yaw,
                    (0., sign * base.d * 0.5),
                    "side",
                );
                for side in [-1., 1.] {
                    let along = end - sign * raw.d * 0.5;
                    add(
                        along,
                        side * (base.d + raw.w) * 0.5,
                        base.yaw + FRAC_PI_2,
                        (along, side * base.d * 0.5),
                        "corner",
                    );
                }
            }
        }
        out.sort_by(|a, b| {
            a.rank
                .total_cmp(&b.rank)
                .then_with(|| a.joint.id.cmp(&b.joint.id))
        });
        out
    }
    pub(super) fn restore_preview(&self, p: &Player, shown: &mut Preview, old: Pose) -> bool {
        if shown.wall.kind != old.kind {
            return false;
        }
        let support = shown.support.and_then(|i| self.props[i].build_id);
        if support != old.support {
            return false;
        }
        if let Some(j) = old.joint {
            let mut search = shown.wall.clone();
            search.x = old.x;
            search.z = old.z;
            search.yaw = old.yaw;
            search.prepare_geometry();
            if !self
                .nearby_joints(&search)
                .iter()
                .any(|c| c.joint == j && c.x == old.x && c.z == old.z && c.yaw == old.yaw)
            {
                return false;
            }
        }
        shown.wall.x = old.x;
        shown.wall.z = old.z;
        shown.wall.yaw = old.yaw;
        shown.wall.prepare_geometry();
        shown.joint = old.joint;
        shown.clear = self.site_clear(p, &shown.wall, shown.support);
        true
    }
    fn visible_build_site(&self, p: &Player, build: &Prop) -> bool {
        let eye = [p.x, EYE, p.z];
        let delta = [build.x - p.x, build.h.min(1.3) * 0.85 - EYE, build.z - p.z];
        let distance = delta[0].hypot(delta[1]).hypot(delta[2]);
        if distance < 0.05 {
            return false;
        }
        let direction = delta.map(|v| v / distance);
        self.ray(eye, direction, distance - 0.03).0.is_none()
            && self
                .logistics
                .ray(eye, direction, distance - 0.03, None)
                .is_none()
    }
    fn site_clear(&self, p: &Player, build: &Prop, support: Option<usize>) -> bool {
        let (w, d) = (build.w, build.d);
        let b = &self.level.bounds;
        let mut clear = !build.intersects(p.x, p.z, RADIUS + 0.04)
            && (p.x - build.x).hypot(p.z - build.z) <= 2.2 + build.d * 0.5 + SNAP_RADIUS + 0.01
            && build.x - build.world_rx >= b.min_x
            && build.x + build.world_rx <= b.max_x
            && build.z - build.world_rz >= b.min_z
            && build.z + build.world_rz <= b.max_z
            && self
                .level
                .team_spawns
                .values()
                .flatten()
                .all(|sp| (sp.x - build.x).hypot(sp.z - build.z) >= 2.5 - 1e-5)
            && self
                .players
                .values()
                .all(|q| !q.alive || !build.intersects(q.x, q.z, RADIUS + 0.04))
            && self.props.iter().enumerate().all(|(i, q)| {
                if Some(i) == support || q.dead || !q.solid {
                    return true;
                }
                if build.kind == "barricade"
                    && q.built
                    && q.kind == "barricade"
                    && q.team == build.team
                {
                    return !build.overlaps_margin(q, -0.0001);
                }
                !build.touches(q)
            })
            && (support.is_some() || self.visible_build_site(p, build));
        // Tile the oriented build footprint with small conservative circles for
        // dynamic vehicle clearance, including long hulls and each corner.
        let nx = (w / 0.35).ceil() as i32;
        let nz = (d / 0.35).ceil() as i32;
        if clear {
            'tiles: for i in 0..=nx {
                for j in 0..=nz {
                    let lx = (i as f32 / nx as f32 - 0.5) * w;
                    let lz = (j as f32 / nz as f32 - 0.5) * d;
                    if self.logistics.blocked(
                        build.x + build.cs * lx - build.sn * lz,
                        build.z + build.sn * lx + build.cs * lz,
                        0.24,
                        None,
                    ) {
                        clear = false;
                        break 'tiles;
                    }
                }
            }
        }
        clear
    }
}
