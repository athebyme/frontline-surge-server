//! Authoritative fixed-tick team deathmatch. No renderer or network dependencies.
#[path = "inventory.rs"]
pub(crate) mod inventory;
use crate::arsenal::{Arsenal, FireControl};
use crate::drone_sim::DroneFleet;
use crate::fracture::{Damage, FractureState, Layout};
use crate::logistics::{Logistics, VehicleSpawn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

const EYE: f32 = 1.68;
const RADIUS: f32 = 0.36;
const TEAM_BUILD_LIMIT: usize = 64;
const DEBRIS_LIMIT: usize = 32;
const DYNAMIC_LIMIT: usize = TEAM_BUILD_LIMIT * 2 + DEBRIS_LIMIT;
const ROUND_SECONDS: f64 = 7200.0;
const FRAG_LIMIT: u32 = 0;
/// The weapon table contains game balance values in map units and seconds.
#[derive(Clone, Copy, PartialEq, Default, Serialize, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Weapon {
    #[default]
    Mr7,
    Vx9,
    Ar4,
}
#[derive(Clone, Copy)]
struct WeaponStats {
    magazine: u32,
    reserve: u32,
    cooldown: f32,
    reload: f32,
    range: f32,
    body: f32,
    head_scale: f32,
    hard: f32,
    falloff_start: f32,
    falloff_end: f32,
    far_scale: f32,
    hip_spread: f32,
    aim_spread: f32,
    mobility: f32,
}
impl Weapon {
    fn parse(v: &Value) -> Option<Self> {
        match (v.as_str(), v.as_u64()) {
            (Some("mr7" | "1"), _) | (_, Some(1)) => Some(Self::Mr7),
            (Some("vx9" | "2"), _) | (_, Some(2)) => Some(Self::Vx9),
            (Some("ar4" | "3"), _) | (_, Some(3)) => Some(Self::Ar4),
            _ => None,
        }
    }
    fn slot(self) -> usize {
        match self {
            Self::Mr7 => 0,
            Self::Vx9 => 1,
            Self::Ar4 => 2,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Mr7 => "mr7",
            Self::Vx9 => "vx9",
            Self::Ar4 => "ar4",
        }
    }
    fn stats(self) -> WeaponStats {
        match self {
            Self::Mr7 => WeaponStats {
                magazine: 8,
                reserve: 40,
                cooldown: 0.38,
                reload: 2.1,
                range: 275.,
                body: 55.,
                head_scale: 1.9,
                hard: 25.,
                falloff_start: 45.,
                falloff_end: 200.,
                far_scale: 0.74,
                hip_spread: 0.007,
                aim_spread: 0.0006,
                mobility: 1.0,
            },
            Self::Vx9 => WeaponStats {
                magazine: 20,
                reserve: 80,
                cooldown: 0.103,
                reload: 2.7,
                range: 120.,
                body: 31.,
                head_scale: 1.55,
                hard: 9.,
                falloff_start: 15.,
                falloff_end: 75.,
                far_scale: 0.45,
                hip_spread: 0.019,
                aim_spread: 0.007,
                mobility: 0.98,
            },
            Self::Ar4 => WeaponStats {
                magazine: 15,
                reserve: 60,
                cooldown: 0.205,
                reload: 1.65,
                range: 195.,
                body: 41.,
                head_scale: 1.7,
                hard: 16.,
                falloff_start: 34.,
                falloff_end: 135.,
                far_scale: 0.65,
                hip_spread: 0.006,
                aim_spread: 0.0018,
                mobility: 1.11,
            },
        }
    }
}
impl WeaponStats {
    fn damage(self, d: f32, head: bool) -> f32 {
        let t = ((d - self.falloff_start) / (self.falloff_end - self.falloff_start)).clamp(0., 1.);
        self.body * (1. - t * (1. - self.far_scale)) * if head { self.head_scale } else { 1. }
    }
}
#[derive(Clone, Copy, Debug, Default)]
struct Pocket {
    ammo: u32,
    reserve: u32,
}
impl Pocket {
    fn full(w: Weapon) -> Self {
        let s = w.stats();
        Self {
            ammo: s.magazine,
            reserve: s.reserve,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub(crate) struct Spawn {
    pub(crate) x: f32,
    pub(crate) z: f32,
    pub(crate) yaw: f32,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Bounds {
    pub(crate) min_x: f32,
    pub(crate) max_x: f32,
    pub(crate) min_z: f32,
    pub(crate) max_z: f32,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Level {
    #[serde(default)]
    resource_points: Vec<inventory::ResourcePoint>,
    #[serde(default)]
    vehicle_spawns: Vec<VehicleSpawn>,
    bounds: Bounds,
    props: Vec<Prop>,
    team_spawns: BTreeMap<String, Vec<Spawn>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Prop {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) style: Option<String>,
    pub(crate) variant: u32,
    #[serde(skip)]
    pub(crate) layout: Option<Arc<Layout>>,
    #[serde(skip)]
    pub(crate) fracture: FractureState,
    pub(crate) kind: String,
    pub(crate) x: f32,
    pub(crate) z: f32,
    pub(crate) w: f32,
    pub(crate) d: f32,
    pub(crate) h: f32,
    pub(crate) yaw: f32,
    pub(crate) solid: bool,
    pub(crate) collision_w: Option<f32>,
    pub(crate) collision_d: Option<f32>,
    pub(crate) collision_h: Option<f32>,
    pub(crate) hp: Option<f32>,
    pub(crate) max_hp: Option<f32>,
    pub(crate) resource: Option<String>,
    pub(crate) stock: u32,
    pub(crate) dead: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) build_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) vehicle_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) team: Option<String>,
    pub(crate) built: bool,
    #[serde(skip)]
    pub(crate) capacity: u32,
    #[serde(skip)]
    pub(crate) restock_at: Option<f32>,
    #[serde(skip)]
    pub(crate) debris_source: Option<usize>,
    #[serde(skip)]
    pub(crate) dirty: bool,
    #[serde(skip)]
    pub(crate) sn: f32,
    #[serde(skip)]
    pub(crate) cs: f32,
    #[serde(skip)]
    pub(crate) world_rx: f32,
    #[serde(skip)]
    pub(crate) world_rz: f32,
}
/// Game material response for single-piece scenery and moving hulls. Buildings
/// use the independent per-cell material table in the shared fracture layout.
fn object_damage(kind: &str, style: Option<&str>, source: Damage, incoming: f32) -> f32 {
    let hardened = kind == "mbt"
        || matches!(
            (kind, style),
            ("crate", Some("shippingContainer")) | ("barrels", Some("tankBattery"))
        );
    let scale = if hardened {
        match source {
            Damage::Bullet(_) => 0.,
            Damage::Ram(_) => 0.08,
            Damage::Grenade => 0.25,
            Damage::Tank => 1.,
        }
    } else if kind == "apc" {
        match source {
            Damage::Bullet(_) => 0.28,
            Damage::Ram(_) => 0.4,
            Damage::Grenade => 0.65,
            Damage::Tank => 1.,
        }
    } else if kind == "cargo6x6" {
        match source {
            Damage::Bullet(_) => 0.55,
            Damage::Ram(_) => 0.7,
            Damage::Grenade => 0.85,
            Damage::Tank => 1.,
        }
    } else {
        1.
    };
    incoming * scale
}
impl Prop {
    fn wood_tree(&self) -> bool {
        matches!(self.kind.as_str(), "tree" | "pine")
    }
    pub(crate) fn prepare_geometry(&mut self) {
        (self.sn, self.cs) = self.yaw.sin_cos();
        let hw = self.collision_w.unwrap_or(self.w) * 0.5;
        let hd = self.collision_d.unwrap_or(self.d) * 0.5;
        self.world_rx = hw * self.cs.abs() + hd * self.sn.abs();
        self.world_rz = hd * self.cs.abs() + hw * self.sn.abs();
    }
    fn initialize_layout(&mut self) {
        self.layout = Layout::new(&self.kind, self.w, self.h, self.d, self.variant).map(Arc::new);
    }
    fn prepare_static(&mut self) {
        self.initialize_layout();
        self.prepare_geometry();
        self.hp = self.hp.or(match self.kind.as_str() {
            "tree" => Some(125.),
            "pine" => Some(110.),
            "fence" => Some(76.),
            "pole" => Some(92.),
            "wall" | "logs" => Some(140.),
            "sandbag" => Some(154.),
            "barrels" | "crate" => Some(95.),
            "house" => Some(420.),
            "barn" => Some(370.),
            "ruin" => Some(310.),
            "cargo6x6" => Some(245.),
            "patrol4x4" => Some(185.),
            "scout_buggy" => Some(95.),
            "apc" => Some(390.),
            "mbt" => Some(480.),
            "tower" => Some(235.),
            "oilwell" => Some(380.),
            _ => None,
        });
        self.max_hp = self.hp;
        if self.wood_tree() {
            self.resource = Some("wood".into());
            self.stock = 3;
        } else if self.resource.is_some() {
            self.stock = self.stock.max(1);
            self.capacity = self.stock;
        }
    }
    pub(crate) fn footprint(&self, x: f32, z: f32) -> f32 {
        let (dx, dz) = (x - self.x, z - self.z);
        let a = ((dx * self.cs + dz * self.sn).abs() - self.collision_w.unwrap_or(self.w) * 0.5)
            .max(0.);
        let b = ((dz * self.cs - dx * self.sn).abs() - self.collision_d.unwrap_or(self.d) * 0.5)
            .max(0.);
        a * a + b * b
    }
    pub(crate) fn touches(&self, other: &Prop) -> bool {
        if (self.x - other.x).abs() > self.world_rx + other.world_rx + 0.05
            || (self.z - other.z).abs() > self.world_rz + other.world_rz + 0.05
        {
            return false;
        }
        let delta = (other.x - self.x, other.z - self.z);
        let extent = |p: &Prop, x: f32, z: f32| {
            (p.cs * x + p.sn * z).abs() * p.collision_w.unwrap_or(p.w) * 0.5
                + (-p.sn * x + p.cs * z).abs() * p.collision_d.unwrap_or(p.d) * 0.5
        };
        [
            (self.cs, self.sn),
            (-self.sn, self.cs),
            (other.cs, other.sn),
            (-other.sn, other.cs),
        ]
        .into_iter()
        .all(|(x, z)| {
            (delta.0 * x + delta.1 * z).abs() < extent(self, x, z) + extent(other, x, z) + 0.04
        })
    }
    fn local(&self, o: [f32; 3]) -> [f32; 3] {
        let dx = o[0] - self.x;
        let dz = o[2] - self.z;
        [
            dx * self.cs + dz * self.sn,
            o[1],
            dz * self.cs - dx * self.sn,
        ]
    }
    fn local_dir(&self, d: [f32; 3]) -> [f32; 3] {
        [
            d[0] * self.cs + d[2] * self.sn,
            d[1],
            d[2] * self.cs - d[0] * self.sn,
        ]
    }
    pub(crate) fn impact_material(&self) -> &str {
        if matches!(
            self.kind.as_str(),
            "patrol4x4" | "scout_buggy" | "cargo6x6" | "apc" | "mbt" | "barrels"
        ) || self.style.as_deref() == Some("shippingContainer")
        {
            "metal"
        } else {
            &self.kind
        }
    }
    pub(crate) fn contact(&self, x: f32, z: f32, r: f32, height: f32) -> Option<[f32; 3]> {
        if self.dead
            || !self.solid
            || (x - self.x).abs() > self.world_rx + r + 0.01
            || (z - self.z).abs() > self.world_rz + r + 0.01
        {
            return None;
        }
        let q = self.local([x, height * 0.46, z]);
        let clamp_box = |c: [f32; 3], wh: [f32; 3]| {
            [
                q[0].clamp(c[0] - wh[0] * 0.5, c[0] + wh[0] * 0.5),
                q[1].clamp(c[1] - wh[1] * 0.5, c[1] + wh[1] * 0.5),
                q[2].clamp(c[2] - wh[2] * 0.5, c[2] + wh[2] * 0.5),
            ]
        };
        let local = if let Some(l) = &self.layout {
            l.cells
                .iter()
                .enumerate()
                .filter(|(id, c)| {
                    c.center[1] - c.size[1] * 0.5 < height
                        && !self.fracture.broken.contains(&(*id as u16))
                })
                .map(|(_, c)| clamp_box(c.center, c.size))
                .filter(|h| (h[0] - q[0]).hypot(h[2] - q[2]) <= r)
                .min_by(|a, b| {
                    ((a[0] - q[0]).hypot(a[2] - q[2]).hypot(a[1] - q[1]))
                        .total_cmp(&((b[0] - q[0]).hypot(b[2] - q[2]).hypot(b[1] - q[1])))
                })?
        } else {
            if self.footprint(x, z) > r * r {
                return None;
            }
            clamp_box(
                [0., self.h * 0.5, 0.],
                [
                    self.collision_w.unwrap_or(self.w),
                    self.h,
                    self.collision_d.unwrap_or(self.d),
                ],
            )
        };
        Some([
            self.x + self.cs * local[0] - self.sn * local[2],
            local[1],
            self.z + self.sn * local[0] + self.cs * local[2],
        ])
    }
    pub(crate) fn intersects(&self, x: f32, z: f32, r: f32) -> bool {
        if self.dead
            || !self.solid
            || (x - self.x).abs() > self.world_rx + r + 0.6
            || (z - self.z).abs() > self.world_rz + r + 0.6
        {
            return false;
        }
        if let Some(layout) = &self.layout {
            let local = self.local([x, 0., z]);
            return layout.intersects(&self.fracture, local[0], local[2], r);
        }
        self.footprint(x, z) <= r * r
    }
    pub(crate) fn ray(&self, o: [f32; 3], v: [f32; 3], limit: f32) -> Option<f32> {
        if self.dead || !self.solid {
            return None;
        }
        if let Some(layout) = &self.layout {
            let ex = o[0] + v[0] * limit;
            let ez = o[2] + v[2] * limit;
            if self.x + self.world_rx + 1.0 < o[0].min(ex)
                || self.x - self.world_rx - 1.0 > o[0].max(ex)
                || self.z + self.world_rz + 1.0 < o[2].min(ez)
                || self.z - self.world_rz - 1.0 > o[2].max(ez)
            {
                return None;
            }
            return layout.ray(&self.fracture, self.local(o), self.local_dir(v), limit);
        }
        let dx = o[0] - self.x;
        let dz = o[2] - self.z;
        let h = self.collision_h.unwrap_or(self.h) * 0.5;
        let origin = [
            dx * self.cs + dz * self.sn,
            o[1] - h,
            dz * self.cs - dx * self.sn,
        ];
        let direction = [
            v[0] * self.cs + v[2] * self.sn,
            v[1],
            v[2] * self.cs - v[0] * self.sn,
        ];
        let half = [
            self.collision_w.unwrap_or(self.w) * 0.5,
            h,
            self.collision_d.unwrap_or(self.d) * 0.5,
        ];
        let (mut enter, mut exit) = (0.0f32, limit);
        for n in 0..3 {
            if direction[n].abs() < 1e-7 {
                if origin[n].abs() > half[n] {
                    return None;
                }
            } else {
                let a = (-half[n] - origin[n]) / direction[n];
                let b = (half[n] - origin[n]) / direction[n];
                enter = enter.max(a.min(b));
                exit = exit.min(a.max(b));
                if enter > exit {
                    return None;
                }
            }
        }
        Some(enter)
    }
}
#[derive(Clone, Default, Debug)]
struct Input {
    forward: f32,
    strafe: f32,
    sprint: bool,
    aim: bool,
    shoot: bool,
    secondary: bool,
}
#[derive(Clone, Default, Debug)]
struct Edges {
    inventory: Option<inventory::Command>,
    reload: bool,
    interact: bool,
    build: bool,
    place: bool,
    trigger: bool,
    vehicle: bool,
    service: bool,
    upgrade: bool,
    ammo_craft: bool,
    cargo: bool,
    upgrade_select: bool,
    cycle_build: bool,
    grenade: bool,
    drone: bool,
    ping: bool,
    pulse: bool,
    cycle_mode: bool,
    fit_optic: Option<String>,
    fit_grip: Option<String>,
    craft: Option<&'static str>,
}

#[derive(Clone, Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Player {
    #[serde(skip)]
    arsenal: Arsenal,
    #[serde(skip)]
    gate: FireControl,
    pub(crate) points: u32,
    pub(crate) assists: u32,
    pub(crate) grenades: u32,
    #[serde(skip)]
    last_throw: f32,
    #[serde(skip)]
    contributors: BTreeMap<u32, f32>,
    pub(crate) weapon: Weapon,
    pub(crate) mag_size: u32,
    pub(crate) oil: u32,
    pub(crate) fuel: u32,
    pub(crate) powder: u32,
    pub(crate) build_kind: String,
    #[serde(skip)]
    pockets: [Pocket; 3],
    #[serde(skip)]
    shot_index: u32,
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) team: String,
    pub(crate) bot: bool,
    pub(crate) kills: u32,
    pub(crate) deaths: u32,
    pub(crate) seq: i64,
    pub(crate) x: f32,
    pub(crate) z: f32,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) eye_height: f32,
    pub(crate) hp: f32,
    pub(crate) alive: bool,
    pub(crate) respawn_in: f32,
    pub(crate) shield: f32,
    pub(crate) ammo: u32,
    pub(crate) reserve: u32,
    pub(crate) wood: u32,
    pub(crate) metal: u32,
    pub(crate) stamina: f32,
    pub(crate) reload: f32,
    pub(crate) reload_time: f32,
    pub(crate) recoil: f32,
    pub(crate) bob: f32,
    pub(crate) moving: bool,
    pub(crate) sprinting: bool,
    pub(crate) aim: f32,
    pub(crate) hit_flash: f32,
    pub(crate) hit_confirm: f32,
    pub(crate) damage_yaw: f32,
    pub(crate) build_mode: bool,
    pub(crate) preview: Option<Value>,
    pub(crate) hint: String,
    pub(crate) message: String,
    pub(crate) message_t: f32,
    #[serde(skip)]
    control: Input,
    #[serde(skip)]
    edges: Edges,
    #[serde(skip)]
    cooldown: f32,
    #[serde(skip)]
    last_input: f32,
    #[serde(skip)]
    hurt_at: f32,
    #[serde(skip)]
    exhausted: bool,
    #[serde(skip)]
    last_use: f32,
    #[serde(skip)]
    hold_trigger: bool,
    #[serde(skip)]
    next_context: f32,
}
impl Player {
    fn switch_weapon(&mut self, w: Weapon) {
        if self.weapon == w {
            return;
        }
        self.pockets[self.weapon.slot()] = Pocket {
            ammo: self.ammo,
            reserve: self.reserve,
        };
        let load = self.pockets[w.slot()];
        let stats = w.stats();
        self.weapon = w;
        self.gate.reset();
        self.ammo = load.ammo;
        self.reserve = load.reserve;
        self.mag_size = stats.magazine;
        self.reload = 0.;
        self.reload_time = stats.reload;
        self.cooldown = self.cooldown.max(0.26);
        self.aim = 0.;
        self.hold_trigger = self.control.shoot;
    }
    pub(crate) fn notice(&mut self, text: &str) {
        self.message = text.into();
        self.message_t = 2.5;
    }
    fn ray(&self, o: [f32; 3], d: [f32; 3], limit: f32) -> Option<f32> {
        let dx = o[0] - self.x;
        let dz = o[2] - self.z;
        let a = d[0] * d[0] + d[2] * d[2];
        let b = dx * d[0] + dz * d[2];
        let c = dx * dx + dz * dz - 0.39 * 0.39;
        let (mut lo, mut hi) = (0.0f32, limit);
        if a < 1e-9 {
            if c > 0.0 {
                return None;
            }
        } else {
            let disc = b * b - a * c;
            if disc < 0.0 {
                return None;
            }
            let root = disc.sqrt();
            lo = lo.max((-b - root) / a);
            hi = hi.min((-b + root) / a);
        }
        if d[1].abs() < 1e-7 {
            if !(0.0..=1.84).contains(&o[1]) {
                return None;
            }
        } else {
            let a = -o[1] / d[1];
            let b = (1.84 - o[1]) / d[1];
            lo = lo.max(a.min(b));
            hi = hi.min(a.max(b));
        }
        if hi >= lo {
            Some(lo)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Grenade {
    id: u64,
    actor: u32,
    team: String,
    x: f32,
    y: f32,
    z: f32,
    remaining: f32,
    total: f32,
    #[serde(skip)]
    velocity: [f32; 3],
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Shell {
    id: u64,
    actor: u32,
    team: String,
    #[serde(skip)]
    name: String,
    vehicle_id: u32,
    x: f32,
    y: f32,
    z: f32,
    vx: f32,
    vy: f32,
    vz: f32,
    remaining: f32,
}
#[derive(Clone, Copy)]
enum Strike {
    Infantry,
    Machinegun,
    Shell { vehicle: u32, id: u64 },
}
pub struct Match {
    ground: inventory::Ground,
    shells: Vec<Shell>,
    next_shell: u64,
    pub(crate) drones: DroneFleet,
    grenades: Vec<Grenade>,
    next_grenade: u64,
    next_build_id: u32,
    level: Level,
    pub(crate) props: Vec<Prop>,
    pub(crate) players: BTreeMap<u32, Player>,
    pub(crate) round: u32,
    pub(crate) elapsed: f32,
    pub(crate) remaining: f64,
    pub(crate) round_seconds: u32,
    frag_limit: u32,
    pub(crate) logistics: Logistics,
    next_round_in: f32,
    phase: &'static str,
    winner: Option<&'static str>,
    blue: u32,
    red: u32,
    events: VecDeque<Value>,
    next_event: u64,
    brains: BTreeMap<u32, Brain>,
    nav: Option<NavGrid>,
}
fn held(input: &Value, key: &str) -> bool {
    input.get(key).and_then(Value::as_bool).unwrap_or(false)
}
fn num(input: &Value, key: &str, default: f32, min: f32, max: f32) -> f32 {
    input
        .get(key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
        .unwrap_or(default)
        .clamp(min, max)
}
fn reduce(value: &mut f32, delta: f32) {
    *value = (*value - delta).max(0.0);
}
impl Default for Match {
    fn default() -> Self {
        Self::new()
    }
}
impl Match {
    pub fn new() -> Self {
        Self::from_level(
            serde_json::from_str(include_str!("../assets/level.json"))
                .expect("embedded campaign map"),
        )
    }
    fn from_level(level: Level) -> Self {
        let mut game = Self {
            ground: Default::default(),
            shells: vec![],
            next_shell: 1,
            drones: DroneFleet::default(),
            grenades: vec![],
            next_grenade: 1,
            next_build_id: 1,
            level,
            props: vec![],
            players: BTreeMap::new(),
            round: 0,
            elapsed: 0.,
            remaining: 0.,
            round_seconds: ROUND_SECONDS as u32,
            frag_limit: FRAG_LIMIT,
            logistics: Logistics::default(),
            next_round_in: 0.,
            phase: "playing",
            winner: None,
            blue: 0,
            red: 0,
            events: VecDeque::new(),
            next_event: 1,
            brains: BTreeMap::new(),
            nav: None,
        };
        game.reset();
        game
    }
    fn reset(&mut self) {
        self.round += 1;
        self.ground.reset(&self.level.resource_points, self.elapsed);
        self.shells.clear();
        self.next_build_id = 1;
        self.drones = DroneFleet::default();
        self.grenades.clear();
        self.logistics.reset(&self.level.vehicle_spawns);
        self.props = self.level.props.clone();
        for obj in &mut self.props {
            obj.prepare_static();
        }
        self.nav = None;
        for b in self.brains.values_mut() {
            b.path.clear();
            b.next_plan = 0.;
            b.target = 0;
        }
        self.events.clear();
        self.blue = 0;
        self.red = 0;
        self.winner = None;
        self.phase = "playing";
        self.remaining = f64::from(self.round_seconds);
        self.next_round_in = 0.;
        for p in self.players.values_mut() {
            p.arsenal = Default::default();
            p.kills = 0;
            p.points = 0;
            p.assists = 0;
            p.deaths = 0;
            p.alive = false;
        }
        let ids: Vec<_> = self.players.keys().copied().collect();
        for id in ids {
            let mut p = self.players.remove(&id).unwrap();
            self.respawn(&mut p, false);
            self.players.insert(id, p);
        }
    }
    pub fn add_player(&mut self, id: u32, name: &str) {
        self.add_player_with_team(id, name, "auto");
    }
    pub fn add_player_with_team(&mut self, id: u32, name: &str, preferred_team: &str) {
        if self.players.contains_key(&id) || self.players.len() >= 12 {
            return;
        }
        let blue = self.players.values().filter(|q| q.team == "blue").count();
        let name: String = name
            .trim()
            .chars()
            .filter(|c| !c.is_control() && !matches!(*c, '<' | '>' | '[' | ']'))
            .take(18)
            .collect();
        let mut player = Player {
            id,
            name: if name.is_empty() {
                "Боец".into()
            } else {
                name
            },
            team: if preferred_team == "blue" || preferred_team == "red" {
                preferred_team.to_owned()
            } else if blue <= self.players.len() - blue {
                "blue".into()
            } else {
                "red".into()
            },
            seq: -1,
            ..Default::default()
        };
        self.respawn(&mut player, false);
        self.event(
            "join",
            &player,
            json!({"actorName":player.name,"team":player.team}),
        );
        self.players.insert(id, player);
    }
    pub fn remove_player(&mut self, id: u32) {
        self.brains.remove(&id);
        self.drones.remove(id);
        self.logistics.remove_player(id);
        if let Some(p) = self.players.remove(&id) {
            self.event("leave", &p, json!({"actorName":p.name,"team":p.team}));
        }
    }
    fn respawn(&mut self, p: &mut Player, announce: bool) {
        self.drones.remove(p.id);
        let spots = &self.level.team_spawns[&p.team];
        let mut best = spots[0];
        let mut best_score = f32::NEG_INFINITY;
        for i in 0..spots.len() {
            let s = spots[(i + p.deaths as usize) % spots.len()];
            if self.blocked(s.x, s.z, 0.38) {
                continue;
            }
            let mut score = 90.0f32;
            let mut penalty = 0.;
            for q in self.players.values().filter(|q| q.alive && q.id != p.id) {
                let dx = q.x - s.x;
                let dz = q.z - s.z;
                let d = dx.hypot(dz);
                if q.team == p.team {
                    if d < 0.9 {
                        penalty += 100.;
                    }
                } else if d > 0.001 {
                    let blocked = self
                        .ray([s.x, 1.32, s.z], [dx / d, 0., dz / d], d)
                        .0
                        .is_some();
                    score = score.min(d + if blocked { 15. } else { 0. });
                } else {
                    score = 0.;
                }
            }
            if score - penalty > best_score {
                best = s;
                best_score = score - penalty;
            }
        }
        *p = Player {
            id: p.id,
            name: std::mem::take(&mut p.name),
            team: std::mem::take(&mut p.team),
            kills: p.kills,
            points: p.points,
            arsenal: std::mem::take(&mut p.arsenal),
            assists: p.assists,
            grenades: 1,
            deaths: p.deaths,
            seq: p.seq,
            bot: p.bot,
            x: best.x,
            z: best.z,
            yaw: best.yaw,
            eye_height: EYE,
            hp: 100.,
            alive: true,
            shield: 2.3,
            weapon: p.weapon,
            mag_size: p.weapon.stats().magazine,
            ammo: p.weapon.stats().magazine,
            reserve: p.weapon.stats().reserve,
            build_kind: "wall".into(),
            pockets: [
                Pocket::full(Weapon::Mr7),
                Pocket::full(Weapon::Vx9),
                Pocket::full(Weapon::Ar4),
            ],
            stamina: 100.,
            reload_time: p.weapon.stats().reload,
            hurt_at: -100.,
            last_throw: -100.,
            last_use: -100.,
            last_input: self.elapsed,
            ..Default::default()
        };
        self.update_hint(p);
        if announce {
            self.event("respawn", p, json!({}));
        }
    }
    pub fn input(&mut self, id: u32, input: &Value) {
        if self.phase != "playing" {
            return;
        }
        let Some(p) = self.players.get_mut(&id) else {
            return;
        };
        if !p.alive {
            return;
        }
        if let Some(s) = input.get("seq") {
            let Some(seq) = s.as_i64() else {
                return;
            };
            if seq < 0 || seq <= p.seq {
                return;
            }
            p.seq = seq;
        }
        if let Some(w) = Weapon::parse(&input["weapon"]) {
            p.switch_weapon(w);
        }
        p.last_input = self.elapsed;
        p.yaw = (num(input, "yaw", p.yaw, -100000., 100000.) + PI).rem_euclid(TAU) - PI;
        p.pitch = num(input, "pitch", p.pitch, -1.28, 1.28);
        let shoot = held(input, "shoot");
        p.edges.trigger |= shoot && !p.control.shoot;
        if !shoot {
            p.hold_trigger = false;
        }
        p.control = Input {
            forward: num(input, "forward", 0., -1., 1.),
            strafe: num(input, "strafe", 0., -1., 1.),
            sprint: held(input, "sprint"),
            aim: held(input, "aim"),
            shoot,
            secondary: held(input, "secondary") || held(input, "aim"),
        };
        p.edges.grenade |= held(input, "grenade");
        p.edges.drone |= held(input, "drone");
        p.edges.ping |= held(input, "ping");
        p.edges.pulse |= held(input, "pulse");
        p.edges.cycle_mode |= held(input, "cycleMode");
        if let Some(f) = input["fitOptic"].as_str().filter(|s| s.len() < 12) {
            p.edges.fit_optic = Some(f.to_string());
        }
        if let Some(f) = input["fitGrip"].as_str().filter(|s| s.len() < 12) {
            p.edges.fit_grip = Some(f.to_string());
        }
        if let Some(key) = input["craft"].as_str() {
            p.edges.craft = match key {
                "wall" => Some("wall"),
                "refinery" => Some("refinery"),
                "workshop" => Some("workshop"),
                "ammo" => Some("ammo"),
                "grenade" => Some("grenade"),
                "refine" => Some("refine"),
                "service" => Some("service"),
                "upgrade" => Some("upgrade"),
                "upgradeSelect" => Some("upgradeSelect"),
                _ => p.edges.craft,
            };
        }
        if let Some(cmd) = inventory::Command::parse(&input["inventory"]) {
            p.edges.inventory = Some(cmd);
        }
        p.edges.reload |= held(input, "reload");
        p.edges.interact |= held(input, "interact");
        p.edges.build |= held(input, "build");
        p.edges.place |= held(input, "place");
        p.edges.vehicle |= held(input, "vehicle");
        p.edges.service |= held(input, "service");
        p.edges.upgrade |= held(input, "upgrade");
        p.edges.ammo_craft |= held(input, "ammoCraft");
        p.edges.cargo |= held(input, "cargo");
        p.edges.upgrade_select |= held(input, "upgradeSelect");
        p.edges.cycle_build |=
            held(input, "cycleBuild") || held(input, "buildCycle") || held(input, "cycle");
    }
    fn event(&mut self, kind: &str, p: &Player, mut details: Value) {
        details["actor"] = p.id.into();
        details["x"] = p.x.into();
        details["z"] = p.z.into();
        self.scene_event(kind, details);
    }
    pub(crate) fn scene_event(&mut self, kind: &str, mut details: Value) {
        details["id"] = self.next_event.into();
        details["at"] = self.elapsed.into();
        details["type"] = kind.into();
        self.next_event += 1;
        if self.events.len() >= 144 {
            self.events.pop_front();
        }
        self.events.push_back(details);
    }
    fn blocked(&self, x: f32, z: f32, r: f32) -> bool {
        let b = &self.level.bounds;
        x - r < b.min_x
            || x + r > b.max_x
            || z - r < b.min_z
            || z + r > b.max_z
            || self.props.iter().any(|p| p.intersects(x, z, r))
            || self.logistics.blocked(x, z, r, None)
    }
    fn ray(&self, o: [f32; 3], d: [f32; 3], range: f32) -> (Option<usize>, f32) {
        let (mut hit, mut best) = (None, range);
        for (i, p) in self.props.iter().enumerate() {
            if let Some(t) = p.ray(o, d, best) {
                if t < best {
                    hit = Some(i);
                    best = t;
                }
            }
        }
        (hit, best)
    }
    pub fn step(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let dt = dt.min(0.1);
        self.elapsed += dt;
        self.events.retain(|e| {
            self.elapsed - (e["at"].as_f64().unwrap_or(0.) as f32)
                < if e["type"] == "kill" { 8. } else { 1.3 }
        });
        if self.phase == "ended" {
            reduce(&mut self.next_round_in, dt);
            if self.next_round_in < 1e-5 {
                self.reset();
            }
            return;
        }
        self.ground.tick(self.elapsed);
        let static_ct = self.level.props.len();
        for i in 0..static_ct {
            let o = &self.props[i];
            if !o.restock_at.is_some_and(|t| self.elapsed >= t) {
                continue;
            }
            if o.dead {
                // A returning footprint must not close on a standing soldier or
                // a new construction. Retry when the space becomes available.
                let occupied = self
                    .players
                    .values()
                    .any(|q| q.alive && o.footprint(q.x, q.z) < 0.46 * 0.46)
                    || self.props[static_ct..]
                        .iter()
                        .any(|q| !q.dead && q.built && q.solid && o.touches(q))
                    || self
                        .logistics
                        .blocked(o.x, o.z, o.world_rx.max(o.world_rz), None);
                if occupied {
                    self.props[i].restock_at = Some(self.elapsed + 5.0);
                    continue;
                }
                let o = &mut self.props[i];
                o.dead = false;
                o.hp = o.max_hp;
                o.stock = if o.wood_tree() { 3 } else { o.capacity };
                o.dirty = true;
                o.restock_at = None;
                self.nav = None;
            } else {
                let o = &mut self.props[i];
                o.restock_at = None;
                o.stock = o.capacity;
                o.dirty = true;
            }
        }
        self.logistics.tick(dt, &self.props, &self.players);
        self.step_shells(dt);
        self.step_grenades(dt);
        if self.phase == "ended" {
            return;
        }
        self.practice_inputs();
        let mut teams = (false, false);
        let ids: Vec<_> = self.players.keys().copied().collect();
        for id in ids {
            let mut p = self.players.remove(&id).unwrap();
            if p.team == "blue" {
                teams.0 = true;
            } else {
                teams.1 = true;
            }
            if !p.alive {
                reduce(&mut p.respawn_in, dt);
                if p.respawn_in < 1e-5 {
                    self.respawn(&mut p, true);
                }
            } else {
                self.update(&mut p, dt);
            }
            self.players.insert(id, p);
            if self.phase == "ended" {
                return;
            }
        }
        self.drones.tick(dt, &self.players);
        if teams.0 && teams.1 {
            self.remaining = (self.remaining - f64::from(dt)).max(0.);
        }
        if self.remaining < 1e-5 {
            self.end_round();
        }
    }
    fn end_round(&mut self) {
        self.phase = "ended";
        self.next_round_in = 10.;
        self.winner = Some(match self.blue.cmp(&self.red) {
            std::cmp::Ordering::Greater => "blue",
            std::cmp::Ordering::Less => "red",
            _ => "draw",
        });
        for p in self.players.values_mut() {
            p.control = Input::default();
            p.edges = Edges::default();
            p.moving = false;
        }
        self.scene_event("roundEnd", json!({"winner":self.winner}));
    }
    fn update(&mut self, p: &mut Player, dt: f32) {
        if self.elapsed - p.last_input > 0.45 {
            p.control = Input::default();
        }
        let ctl = p.control.clone();
        let mut action = std::mem::take(&mut p.edges);
        if self.logistics.driver_vehicle(p.id).is_none() {
            let control = json!({"drone":action.drone,"ping":action.ping,"forward":ctl.forward,"strafe":ctl.strafe,"up":ctl.sprint,"down":ctl.aim});
            if self
                .drones
                .control(p, &control, dt, &self.level.bounds, &self.props)
            {
                if action.pulse {
                    self.drone_pulse(p);
                }
                p.gate.reset();
                p.moving = false;
                p.sprinting = false;
                p.aim = 0.;
                p.build_mode = false;
                p.preview = None;
                p.hold_trigger = ctl.shoot;
                reduce(&mut p.shield, dt);
                reduce(&mut p.hit_flash, dt * 2.7);
                reduce(&mut p.hit_confirm, dt * 3.8);
                return;
            }
        }
        if self.logistics.driver_vehicle(p.id).is_none() {
            let slot = p.weapon.slot();
            if action.cycle_mode {
                p.arsenal.cycle_mode(slot);
                p.gate.reset();
            }
            if action.fit_optic.is_some() || action.fit_grip.is_some() {
                if self.props.iter().any(|q| {
                    !q.dead
                        && q.kind == "workshop"
                        && q.team.as_ref() == Some(&p.team)
                        && q.footprint(p.x, p.z) <= 9.0
                }) {
                    if let Some(key) = &action.fit_optic {
                        p.arsenal.fit(slot, "optic", key);
                    }
                    if let Some(key) = &action.fit_grip {
                        p.arsenal.fit(slot, "grip", key);
                    }
                } else {
                    p.notice("Нужна своя мастерская рядом");
                }
            }
        }
        if let Some(cmd) = action.inventory {
            self.apply_inventory(p, cmd);
        }
        let kit = p.arsenal.equipped(p.weapon.slot());
        if let Some(craft) = action.craft {
            match craft {
                "wall" | "refinery" | "workshop" => {
                    if self.logistics.driver_vehicle(p.id).is_none() {
                        p.build_kind = craft.into();
                        p.build_mode = true;
                        action.build = false;
                        action.cycle_build = false;
                    }
                }
                "ammo" => action.ammo_craft = true,
                "service" => action.service = true,
                "upgrade" => action.upgrade = true,
                "upgradeSelect" => action.upgrade_select = true,
                "refine" => {
                    action.interact = self.props.iter().any(|o| {
                        !o.dead
                            && o.kind == "refinery"
                            && o.team.as_ref() == Some(&p.team)
                            && o.footprint(p.x, p.z) <= 2.6 * 2.6
                    });
                    if !action.interact {
                        p.notice("Нужен свой НПЗ рядом");
                    }
                }
                "grenade" => {
                    if self.elapsed - p.last_use >= 0.15
                        && self.logistics.driver_vehicle(p.id).is_none()
                    {
                        if self.props.iter().any(|o| {
                            !o.dead
                                && o.kind == "workshop"
                                && o.team.as_ref() == Some(&p.team)
                                && o.footprint(p.x, p.z) <= 9.0
                        }) {
                            if p.powder >= 2 && p.metal >= 1 && p.grenades < 4 {
                                p.powder -= 2;
                                p.metal -= 1;
                                p.grenades += 1;
                                self.event("craft", p, json!({"item":"grenade"}));
                                p.notice("Граната готова");
                            } else {
                                p.notice(if p.grenades >= 4 {
                                    "Подсумок полон"
                                } else {
                                    "Нужно 2 пороха и 1 металл"
                                });
                            }
                        } else {
                            p.notice("Нужна своя мастерская рядом");
                        }
                        p.last_use = self.elapsed;
                    }
                }
                _ => {}
            }
        }
        reduce(&mut p.shield, dt);
        reduce(&mut p.message_t, dt);
        reduce(&mut p.cooldown, dt);
        reduce(&mut p.recoil, dt * 6.);
        reduce(&mut p.hit_flash, dt * 2.7);
        reduce(&mut p.hit_confirm, dt * 3.8);
        if p.reload > 0. {
            reduce(&mut p.reload, dt);
            if p.reload < 1e-5 {
                p.reload = 0.;
                let n = (p.mag_size - p.ammo).min(p.reserve);
                p.ammo += n;
                p.reserve -= n;
                self.event("ready", p, json!({}));
            }
        }
        let debounce = self.elapsed - p.last_use >= 0.15;
        let station_use =
            action.interact && debounce && self.logistics.station_action(p, &self.props);
        let was_driving = self.logistics.driver_vehicle(p.id).is_some();
        let wants_ops = was_driving
            || action.vehicle
            || action.service
            || action.upgrade
            || action.ammo_craft
            || action.cargo
            || action.upgrade_select
            || station_use;
        if wants_ops {
            let cmd = json!({"forward":ctl.forward,"strafe":ctl.strafe,"sprint":ctl.sprint,"yaw":p.yaw,"pitch":p.pitch,"shoot":ctl.shoot||action.trigger,
                "secondary":ctl.secondary,"vehicle":action.vehicle,"service":action.service,"upgrade":action.upgrade,"ammoCraft":action.ammo_craft,"cargo":action.cargo,"upgradeSelect":action.upgrade_select,"interact":station_use});
            let effects = self
                .logistics
                .update(p, &cmd, dt, &self.props, &self.level.bounds);
            self.ingest_logistics(effects, p);
            if station_use {
                p.last_use = self.elapsed;
            }
        } else {
            // Even motionless passengers must be ejected on the tick their vehicle is lost.
            let effects =
                self.logistics
                    .update(p, &Value::Null, dt, &self.props, &self.level.bounds);
            self.ingest_logistics(effects, p);
        }
        if was_driving || self.logistics.driver_vehicle(p.id).is_some() {
            p.gate.reset();
            p.sprinting = false;
            p.preview = None;
            p.build_mode = false;
            p.aim = 0.;
            self.update_hint(p);
            return;
        }
        if action.grenade && !p.build_mode {
            self.throw_grenade(p);
        }
        if action.cycle_build {
            p.build_kind = match p.build_kind.as_str() {
                "wall" => "refinery",
                "refinery" => "workshop",
                _ => "wall",
            }
            .into();
        }
        if action.build {
            p.build_mode = !p.build_mode;
        }
        let aiming = ctl.aim && !p.build_mode && p.reload <= 0.;
        p.aim += ((if aiming { 1. } else { 0. }) - p.aim) * (dt * 13. * kit.aim_speed()).min(1.);
        let sprint = ctl.sprint
            && ctl.forward > 0.
            && !aiming
            && !ctl.shoot
            && !p.build_mode
            && !p.exhausted;
        let speed = dt
            * kit.mobility()
            * p.weapon.stats().mobility
            * (if sprint {
                6.25
            } else if aiming {
                2.35
            } else {
                4.25
            })
            / ctl.forward.hypot(ctl.strafe).max(1.);
        let (sn, cs) = p.yaw.sin_cos();
        let dx = (sn * ctl.forward + cs * ctl.strafe) * speed;
        let dz = (-cs * ctl.forward + sn * ctl.strafe) * speed;
        p.moving = false;
        if dx.abs() > 1e-6 && !self.blocked(p.x + dx, p.z, RADIUS) {
            p.x += dx;
            p.moving = true;
        }
        if dz.abs() > 1e-6 && !self.blocked(p.x, p.z + dz, RADIUS) {
            p.z += dz;
            p.moving = true;
        }
        p.sprinting = p.moving && sprint;
        if p.moving {
            p.bob =
                (p.bob + dt * p.weapon.stats().mobility * (if sprint { 12. } else { 8.6 })) % TAU;
        }
        p.stamina = (p.stamina + dt * (if p.sprinting { -17. } else { 18. })).clamp(0., 100.);
        if p.stamina <= 0. {
            p.exhausted = true;
        } else if p.stamina >= 23. {
            p.exhausted = false;
        }
        if self.elapsed - p.hurt_at > 9. {
            p.hp = (p.hp + dt * 1.9).min(100.);
        }
        if action.reload {
            self.reload(p);
        }
        if p.build_mode {
            p.gate.reset();
            let (wall, clear) = self.preview(p);
            let (wood, metal) = Self::build_cost(&wall.kind);
            let has_space = self.team_build_room(&p.team);
            let valid = clear && has_space && p.wood >= wood && p.metal >= metal;
            p.preview = Some(
                json!({"kind":wall.kind,"x":wall.x,"z":wall.z,"yaw":wall.yaw,"w":wall.w,"h":wall.h,"d":wall.d,
                "valid":valid,"clear":clear,"teamLimit":!has_space}),
            );
            if action.place {
                if !has_space {
                    p.notice("Лимит команды · 64 постройки");
                } else if !clear {
                    p.notice("Здесь мало места");
                } else if !valid {
                    p.notice(&format!("Нужно дерева: {wood}, металла: {metal}"));
                } else {
                    let kind = wall.kind.clone();
                    if let Some(slot) = self.dynamic(wall) {
                        p.wood -= wood;
                        p.metal -= metal;
                        p.build_mode = false;
                        p.preview = None;
                        p.hold_trigger = ctl.shoot;
                        self.event("build",p,json!({"buildKind":kind,"index":slot,"buildId":self.props[slot].build_id}));
                        p.notice("Постройка готова");
                    } else {
                        p.notice("Место для новой постройки занято");
                    }
                }
            }
        } else {
            p.preview = None;
            if p.gate.ready(
                ctl.shoot,
                action.trigger,
                !p.hold_trigger && p.reload <= 0. && p.cooldown < 1e-5,
                kit.mode,
            ) {
                self.shoot(p);
            }
        }
        if action.interact
            && !station_use
            && self.elapsed - p.last_use >= 0.15
            && self.phase == "playing"
        {
            self.interact(p);
            p.last_use = self.elapsed;
        }
        if action.interact || action.build || action.place || self.elapsed >= p.next_context {
            self.update_hint(p);
            p.next_context = self.elapsed + 0.12;
        }
    }
    fn reload(&mut self, p: &mut Player) {
        if p.reload > 0. || p.ammo == p.mag_size {
            return;
        }
        if p.reserve == 0 {
            p.notice("Запас пуст. Ищи ящик с патронами");
            return;
        }
        p.gate.reset();
        p.reload = p.reload_time;
        self.event("reload", p, json!({}));
    }
    fn shoot(&mut self, p: &mut Player) {
        if p.ammo == 0 {
            p.cooldown = 0.5;
            self.event("dry", p, json!({"weapon":p.weapon}));
            self.reload(p);
            return;
        }
        let stats = p.weapon.stats();
        p.ammo -= 1;
        p.recoil = 1.;
        p.cooldown = p
            .gate
            .cooldown(stats.cooldown, p.arsenal.equipped(p.weapon.slot()).mode);
        p.shield = 0.;
        p.shot_index = p.shot_index.wrapping_add(1);
        let mut seed = p.shot_index.wrapping_mul(2246822519) ^ p.id.wrapping_mul(3266489917);
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 8388608. - 1.
        };
        let spread = p.arsenal.equipped(p.weapon.slot()).spread_multiplier()
            * (stats.hip_spread + (stats.aim_spread - stats.hip_spread) * p.aim)
            * (if p.moving { 1.55 } else { 1. });
        let (yaw, pitch) = (p.yaw + noise() * spread, p.pitch + noise() * spread);
        self.infantry_ray(p, [p.x, EYE, p.z], yaw, pitch);
    }
    fn infantry_ray(&mut self, p: &mut Player, o: [f32; 3], yaw: f32, pitch: f32) {
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();
        self.trace_shot(
            p,
            o,
            [sy * cp, sp, -cy * cp],
            p.weapon.stats().range,
            Strike::Infantry,
        );
    }
    fn trace_shot(
        &mut self,
        p: &mut Player,
        o: [f32; 3],
        d: [f32; 3],
        range: f32,
        kind: Strike,
    ) -> bool {
        let stats = p.weapon.stats();
        let tank = matches!(kind, Strike::Shell { .. });
        let mg = matches!(kind, Strike::Machinegun);
        let ignore_vehicle = if let Strike::Shell { vehicle, .. } = kind {
            Some(vehicle)
        } else {
            None
        };
        let floor = if d[1] < -1e-6 {
            -o[1] / d[1]
        } else {
            f32::INFINITY
        };
        let (mut solid, mut best) = self.ray(o, d, range.min(floor));
        let mut vehicle = None;
        if let Some((id, t)) = self
            .logistics
            .ray_except(o, d, best, Some(p.id), ignore_vehicle)
        {
            if t < best {
                best = t;
                solid = None;
                vehicle = Some(id);
            }
        }
        let mut drone = None;
        if let Some((id, t)) = self.drones.ray(o, d, best, &p.team) {
            if t < best {
                best = t;
                solid = None;
                vehicle = None;
                drone = Some(id);
            }
        }
        let mut victim = None;
        for (&id, q) in &self.players {
            if q.alive && self.logistics.driver_vehicle(id).is_none() {
                if let Some(t) = q.ray(o, d, best) {
                    if t < best {
                        victim = Some(id);
                        best = t;
                    }
                }
            }
        }
        let end = [o[0] + d[0] * best, o[1] + d[1] * best, o[2] + d[2] * best];
        let any_hit = victim.is_some()
            || vehicle.is_some()
            || drone.is_some()
            || solid.is_some()
            || floor <= range;
        if tank && !any_hit {
            return false;
        }
        let weapon = if tank {
            "tank"
        } else if mg {
            "vehicle_mg"
        } else {
            p.weapon.name()
        };
        let mut info = json!({"actor":p.id,"x":o[0],"y":o[1],"z":o[2],"ex":end[0],"ey":end[1],"ez":end[2],"weapon":weapon});
        if victim.is_none() {
            if vehicle.is_some() || drone.is_some() {
                info["impactMaterial"] = "metal".into();
            } else if let Some(i) = solid {
                let obj = &self.props[i];
                info["impactMaterial"] = if matches!(
                    obj.style.as_deref(),
                    Some("shippingContainer" | "tankBattery")
                ) || matches!(
                    obj.kind.as_str(),
                    "patrol4x4" | "cargo6x6" | "mbt" | "apc" | "scout_buggy"
                ) {
                    "metal".into()
                } else {
                    obj.kind.clone().into()
                };
            } else if floor <= range {
                info["impactMaterial"] = "earth".into();
            }
        }
        if let Strike::Shell { id, vehicle } = kind {
            info["shellId"] = id.into();
            info["vehicleId"] = vehicle.into();
            info["x"] = end[0].into();
            info["y"] = end[1].into();
            info["z"] = end[2].into();
            self.scene_event("shellImpact", info);
        } else {
            self.scene_event("shot", info);
        }
        if let Some(id) = victim {
            let head = end[1] > 1.53;
            self.harm(
                p,
                id,
                if tank {
                    100.
                } else if mg {
                    if head {
                        55.
                    } else {
                        28.
                    }
                } else {
                    stats.damage(best, head)
                },
                weapon,
                head,
            );
        } else if let Some(id) = drone {
            p.hit_confirm = 1.;
            if self.drones.hit(
                id,
                if tank {
                    135.0
                } else if mg {
                    28.
                } else {
                    stats.damage(best, false)
                },
            ) {
                self.scene_event("droneDown",json!({"droneId":id,"actor":p.id,"team":p.team,"x":end[0],"y":end[1],"z":end[2]}));
            } else {
                self.event("hit", p, json!({"droneId":id,"weapon":weapon}));
            }
        } else if let Some(key) = vehicle {
            let d = if tank {
                Damage::Tank
            } else {
                Damage::Bullet(stats.hard)
            };
            let impact = object_damage(
                self.logistics.hull_kind(key).unwrap_or(""),
                None,
                d,
                if tank {
                    135.
                } else if mg {
                    11.
                } else {
                    stats.hard
                },
            );
            let events = self.logistics.hit(key, impact, p.id, &p.team);
            self.ingest_logistics(events, p);
        } else if let Some(i) = solid {
            if tank {
                self.damage_prop(i, end, Damage::Tank, 155., p);
            }
        }
        any_hit
    }

    pub(crate) fn harm(&mut self, p: &mut Player, id: u32, amount: f32, weapon: &str, head: bool) {
        let Some(mut q) = self.players.remove(&id) else {
            return;
        };
        if q.team != p.team && q.shield <= 0. && q.alive && amount > 0.0 {
            q.contributors.retain(|_, at| self.elapsed - *at <= 10.0);
            q.contributors.insert(p.id, self.elapsed);
            q.hp = (q.hp - amount).max(0.);
            q.hurt_at = self.elapsed;
            q.hit_flash = 1.;
            q.damage_yaw = (p.x - q.x).atan2(q.z - p.z);
            p.hit_confirm = 1.;
            self.event("hurt", &q, json!({"attacker":p.id,"weapon":weapon}));
            if q.hp <= 0. {
                q.alive = false;
                q.respawn_in = 4.;
                q.deaths += 1;
                q.moving = false;
                q.sprinting = false;
                q.build_mode = false;
                q.preview = None;
                q.hint.clear();
                q.reload = 0.;
                q.recoil = 0.;
                q.control = Input::default();
                q.edges = Edges::default();
                self.logistics.remove_player(id);
                self.drones.remove(id);
                p.kills += 1;
                p.points += 100;
                let assists: Vec<_> = q
                    .contributors
                    .keys()
                    .copied()
                    .filter(|a| *a != p.id && self.players.get(a).is_some_and(|a| a.team == p.team))
                    .collect();
                for a in &assists {
                    let a = self.players.get_mut(a).unwrap();
                    a.assists += 1;
                    a.points += 50;
                }
                if p.team == "blue" {
                    self.blue += 1;
                } else {
                    self.red += 1;
                }
                self.event("kill",p,json!({"target":q.id,"actorName":p.name,"targetName":q.name,"team":p.team,"head":head,"weapon":weapon,"assistIds":assists}));
                if self.frag_limit > 0 && self.blue.max(self.red) >= self.frag_limit {
                    self.end_round();
                }
            } else {
                self.event("hit", p, json!({"target":q.id,"weapon":weapon}));
            }
        }
        self.players.insert(id, q);
    }

    fn damage_prop(
        &mut self,
        i: usize,
        hit: [f32; 3],
        damage: Damage,
        nonstructural: f32,
        p: &Player,
    ) {
        if self.props[i].dead {
            return;
        }
        if let Some(layout) = self.props[i].layout.clone() {
            let local = self.props[i].local(hit);
            let obj = &mut self.props[i];
            let delta = layout.damage(&mut obj.fracture, local, damage);
            if !delta.touched {
                return;
            }
            let gone = obj.fracture.broken.len();
            // HP reflects patchable wear; cell occupancy decides whether a wall
            // still blocks movement, shots and sight. Only an empty layout dies.
            if let Some(hp) = obj.hp {
                obj.hp = Some(if gone == layout.cells.len() {
                    0.
                } else {
                    (hp - nonstructural).max(1.)
                });
            }
            obj.dirty = true;
            if !delta.removed.is_empty() || !delta.collapsed.is_empty() {
                self.nav = None;
                let all: std::collections::BTreeSet<_> = delta
                    .removed
                    .iter()
                    .chain(&delta.collapsed)
                    .copied()
                    .collect();
                let details = json!({"actor":p.id,"index":i,"buildId":obj.build_id,"kind":obj.kind,"cells":all,"collapsedCells":delta.collapsed,
                    "collapsed":!delta.collapsed.is_empty(),"revision":obj.fracture.revision,"x":hit[0],"y":hit[1],"z":hit[2]});
                self.scene_event("fracture", details);
            }
            if gone == layout.cells.len() {
                self.break_prop(i, p);
            }
        } else if let Some(hp) = self.props[i].hp {
            let obj = &self.props[i];
            let impact = object_damage(&obj.kind, obj.style.as_deref(), damage, nonstructural);
            if impact <= 0.0 {
                return;
            }
            let left = (hp - impact).max(0.);
            self.props[i].hp = Some(left);
            self.props[i].dirty = true;
            if left <= 0. {
                self.break_prop(i, p);
            }
        }
    }

    fn launch_shell(&mut self, p: &Player, o: [f32; 3], yaw: f32, pitch: f32, vehicle_id: u32) {
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();
        let shell = Shell {
            id: self.next_shell,
            actor: p.id,
            team: p.team.clone(),
            name: p.name.clone(),
            vehicle_id,
            x: o[0],
            y: o[1],
            z: o[2],
            vx: sy * cp * 92.,
            vy: sp * 92.,
            vz: -cy * cp * 92.,
            remaining: 4.2,
        };
        self.next_shell += 1;
        self.shells.push(shell);
    }
    fn step_shells(&mut self, dt: f32) {
        let active = std::mem::take(&mut self.shells);
        for mut s in active {
            let t = dt.min(s.remaining);
            let end = [s.x + s.vx * t, s.y + s.vy * t - 3. * t * t, s.z + s.vz * t];
            let delta = [end[0] - s.x, end[1] - s.y, end[2] - s.z];
            let limit = delta[0].hypot(delta[1]).hypot(delta[2]);
            let real = self.players.remove(&s.actor);
            let absent = real.is_none();
            let mut p = real.unwrap_or_else(|| Player {
                id: s.actor,
                team: s.team.clone(),
                name: s.name.clone(),
                ..Default::default()
            });
            let collided = limit > 1e-7
                && self.trace_shot(
                    &mut p,
                    [s.x, s.y, s.z],
                    delta.map(|d| d / limit),
                    limit,
                    Strike::Shell {
                        vehicle: s.vehicle_id,
                        id: s.id,
                    },
                );
            if !absent {
                self.players.insert(p.id, p);
            }
            s.remaining -= t;
            s.vy -= 6. * t;
            s.x = end[0];
            s.y = end[1];
            s.z = end[2];
            if !collided && s.remaining > 1e-5 && self.phase == "playing" {
                self.shells.push(s);
            }
        }
    }
    fn throw_grenade(&mut self, p: &mut Player) {
        if p.grenades == 0 {
            p.notice("Гранат нет");
            return;
        }
        if self.elapsed - p.last_throw < 0.45 {
            return;
        }
        p.grenades -= 1;
        p.last_throw = self.elapsed;
        p.shield = 0.;
        let (sn, cs) = p.yaw.sin_cos();
        let (sp, cp) = p.pitch.sin_cos();
        let g = Grenade {
            id: self.next_grenade,
            actor: p.id,
            team: p.team.clone(),
            x: p.x,
            y: EYE - 0.13,
            z: p.z,
            velocity: [sn * cp * 9.7, 2.8 + sp * 8.5, -cs * cp * 9.7],
            remaining: 1.8,
            total: 1.8,
        };
        self.next_grenade += 1;
        self.event("throw", p, json!({"grenadeId":g.id,"y":g.y}));
        self.grenades.push(g);
    }
    fn step_grenades(&mut self, dt: f32) {
        if self.grenades.is_empty() {
            return;
        }
        let active = std::mem::take(&mut self.grenades);
        for mut g in active {
            let old = [g.x, g.y, g.z];
            g.velocity[1] -= 14.0 * dt;
            let disp = g.velocity.map(|a| a * dt);
            let dist = disp[0].hypot(disp[2]).hypot(disp[1]);
            let b = &self.level.bounds;
            if dist > 1e-5 {
                let dir = disp.map(|c| c / dist);
                let (_, s) = self.ray(old, dir, dist);
                let stop = self.logistics.ray(old, dir, s, None).map_or(s, |(_, t)| t);
                if stop < dist {
                    let t = (stop - 0.025).max(0.);
                    g.x += dir[0] * t;
                    g.y += dir[1] * t;
                    g.z += dir[2] * t;
                    g.velocity = [0., g.velocity[1].min(0.), 0.];
                } else {
                    g.x += disp[0];
                    g.y += disp[1];
                    g.z += disp[2];
                }
            }
            g.x = g.x.clamp(b.min_x + 0.05, b.max_x - 0.05);
            g.z = g.z.clamp(b.min_z + 0.05, b.max_z - 0.05);
            if g.y <= 0.16 {
                g.y = 0.16;
                g.velocity[1] = 0.;
                let friction = (1. - dt * 8.).max(0.);
                g.velocity[0] *= friction;
                g.velocity[2] *= friction;
            }
            g.remaining -= dt;
            if g.remaining <= 0. {
                self.explode(g);
            } else {
                self.grenades.push(g);
            }
        }
    }
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
    fn explode(&mut self, g: Grenade) {
        let o = [g.x, g.y, g.z];
        self.scene_event(
            "explosion",
            json!({"actor":g.actor,"grenadeId":g.id,"team":g.team,"x":g.x,"y":g.y,"z":g.z}),
        );
        let real = self.players.remove(&g.actor);
        let absent = real.is_none();
        let mut p = real.unwrap_or_else(|| Player {
            id: g.actor,
            team: g.team.clone(),
            name: "Боец".into(),
            ..Default::default()
        });
        // Compute visibility against the pre-blast scene. A wall broken by this
        // very explosion still shelters characters on its far side for that hit.
        let mut victims = vec![];
        for (&id, q) in &self.players {
            if !q.alive || q.team == g.team || self.logistics.driver_vehicle(id).is_some() {
                continue;
            }
            let target = [q.x, 0.95, q.z];
            let dist = (q.x - g.x).hypot(q.z - g.z).hypot(target[1] - g.y);
            if dist < 6.3 && self.blast_clear(o, target, None, None) {
                victims.push((id, (124. - dist * 17.5).max(0.)));
            }
        }
        let mut objects = vec![];
        for (i, obj) in self.props.iter().enumerate() {
            if obj.dead || (!obj.solid) || obj.footprint(g.x, g.z) > 3.6 * 3.6 {
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
                    .map(|(_, c)| clamp_box(c.center, c.size))
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
                if distance < 3.6 {
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
            .blast_targets(g.x, g.z, 5.0)
            .into_iter()
            .filter(|(i, x, y, z)| self.blast_clear(o, [*x, *y, *z], None, Some(*i)))
            .collect();
        for (id, amount) in victims {
            self.harm(&mut p, id, amount, "grenade", false);
        }
        for (i, x, y, z) in hulls {
            let dist = (x - g.x).hypot(z - g.z).hypot(y - g.y);
            let impact = object_damage(
                self.logistics.hull_kind(i).unwrap_or(""),
                None,
                Damage::Grenade,
                (86. - dist * 12.).max(0.),
            );
            let ev = self.logistics.hit(i, impact, g.actor, &g.team);
            self.ingest_logistics(ev, &mut p);
        }
        for (i, expected, at, hp) in objects {
            if self.props[i].build_id == expected {
                self.damage_prop(i, at, Damage::Grenade, hp, &p);
            }
        }
        if !absent {
            self.players.insert(p.id, p);
        }
    }
    pub(crate) fn ingest_logistics(&mut self, events: Vec<Value>, p: &mut Player) {
        for ev in events {
            let Some(kind) = ev["type"].as_str().map(str::to_owned) else {
                continue;
            };
            if kind == "vehicleImpact" {
                if let Some(i) = ev["index"]
                    .as_u64()
                    .map(|v| v as usize)
                    .filter(|i| *i < self.props.len())
                {
                    let power = num(&ev, "power", 0., 0., 235.);
                    let xyz = [
                        num(&ev, "x", p.x, -10000., 10000.),
                        num(&ev, "y", 1., 0., 50.),
                        num(&ev, "z", p.z, -10000., 10000.),
                    ];
                    self.damage_prop(i, xyz, Damage::Ram(power), power, p);
                }
                self.scene_event(&kind, ev);
                continue;
            }
            if kind == "mgFire" {
                let yaw = num(&ev, "yaw", p.yaw, -100., 100.);
                let pitch = num(&ev, "pitch", p.pitch, -1.0, 1.0);
                let (sp, cp) = pitch.sin_cos();
                let (sy, cy) = yaw.sin_cos();
                self.trace_shot(
                    p,
                    [p.x, 2.08, p.z],
                    [sy * cp, sp, -cy * cp],
                    180.,
                    Strike::Machinegun,
                );
                continue;
            }
            if kind == "tankFire" {
                let origin = [
                    num(&ev, "x", p.x, -10000., 10000.),
                    num(&ev, "y", 2.5, 0., 100.),
                    num(&ev, "z", p.z, -10000., 10000.),
                ];
                let yaw = num(&ev, "yaw", p.yaw, -10000., 10000.);
                let pitch = num(&ev, "pitch", p.pitch, -1.45, 1.45);
                let vid = ev["vehicleId"].as_u64().unwrap_or_default() as u32;
                self.scene_event(&kind, ev);
                self.launch_shell(p, origin, yaw, pitch, vid);
            } else {
                self.scene_event(&kind, ev);
            }
        }
    }
    fn team_build_room(&self, team: &str) -> bool {
        matches!(team, "red" | "blue")
            && self.props[self.level.props.len()..]
                .iter()
                .filter(|o| o.built && !o.dead && o.team.as_deref() == Some(team))
                .count()
                < TEAM_BUILD_LIMIT
    }
    /// Slots remain stable for spatial updates. buildId identifies their actual
    /// occupant and changes even if a new object reuses the very same position.
    fn dynamic(&mut self, mut obj: Prop) -> Option<usize> {
        let first = self.level.props.len();
        let slots = &self.props[first..];
        let replace = if obj.built {
            if !obj.team.as_ref().is_some_and(|t| self.team_build_room(t)) {
                return None;
            }
            slots.iter().position(|o| o.dead)
        } else {
            // A tree can regrow at its authored site. Keep only its newest
            // salvage pile, then recycle oldest environmental debris at capacity.
            let active = slots
                .iter()
                .enumerate()
                .filter(|(_, o)| !o.built && !o.dead);
            let previous = active
                .clone()
                .find(|(_, o)| obj.debris_source.is_some() && o.debris_source == obj.debris_source)
                .map(|(i, _)| i);
            previous.or_else(|| {
                if active.clone().count() >= DEBRIS_LIMIT {
                    active.min_by_key(|(_, o)| o.build_id).map(|(i, _)| i)
                } else {
                    slots.iter().position(|o| o.dead)
                }
            })
        };
        if replace.is_none() && slots.len() >= DYNAMIC_LIMIT {
            return None;
        }
        obj.build_id = Some(self.next_build_id);
        self.next_build_id = self.next_build_id.checked_add(1)?;
        obj.prepare_geometry();
        obj.initialize_layout();
        let slot = if let Some(i) = replace {
            let slot = first + i;
            self.logistics.forget_site(slot);
            self.props[slot] = obj;
            slot
        } else {
            self.props.push(obj);
            self.props.len() - 1
        };
        self.nav = None;
        Some(slot)
    }
    fn break_prop(&mut self, i: usize, p: &Player) {
        if self.props[i].dead {
            return;
        }
        self.logistics.forget_site(i);
        self.props[i].dead = true;
        self.props[i].dirty = true;
        self.nav = None;
        if i < self.level.props.len() {
            if self.props[i].wood_tree() {
                self.props[i].restock_at = Some(self.elapsed + 480.0);
            } else if self.props[i].capacity > 0 {
                self.props[i].restock_at = Some(self.elapsed + 120.0);
            }
        }
        let v = self.props[i].clone();
        self.event(
            "destroy",
            p,
            json!({"ex":v.x,"ez":v.z,"kind":v.kind,"index":i,"buildId":v.build_id}),
        );
        if matches!(
            v.kind.as_str(),
            "mbt" | "patrol4x4" | "cargo6x6" | "apc" | "scout_buggy"
        ) {
            self.dynamic(Prop {
                debris_source: Some(i),
                kind: "wreck".into(),
                vehicle_kind: Some(v.kind),
                x: v.x,
                z: v.z,
                yaw: v.yaw,
                w: v.w * 0.91,
                d: v.d * 0.92,
                h: (v.h * 0.45).min(1.1),
                solid: true,
                resource: Some("metal".into()),
                stock: 3,
                ..Default::default()
            });
        } else if v.wood_tree() && v.stock > 0 {
            self.dynamic(Prop {
                debris_source: Some(i),
                kind: "logs".into(),
                x: v.x,
                z: v.z,
                yaw: v.yaw,
                w: 1.85,
                d: 0.65,
                h: 0.31,
                solid: false, // A felled tree leaves loose, recoverable timber on the ground.
                hp: Some(18.),
                max_hp: Some(18.),
                resource: Some("wood".into()),
                stock: 1,
                ..Default::default()
            });
        }
    }
    fn build_cost(kind: &str) -> (u32, u32) {
        match kind {
            "refinery" => (8, 6),
            "workshop" => (6, 8),
            _ => (3, 1),
        }
    }
    fn preview(&self, p: &Player) -> (Prop, bool) {
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
        build.prepare_geometry();
        let b = &self.level.bounds;
        let mut clear = build.x - build.world_rx >= b.min_x
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
            && self
                .props
                .iter()
                .all(|q| q.dead || !q.solid || !build.touches(q));
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
                        build.x + cs * lx - sn * lz,
                        build.z + sn * lx + cs * lz,
                        0.24,
                        None,
                    ) {
                        clear = false;
                        break 'tiles;
                    }
                }
            }
        }
        (build, clear)
    }
    fn target(&self, p: &Player) -> Option<(usize, &'static str)> {
        let mut nearest = f32::INFINITY;
        let mut target = None;
        let (sn, cs) = p.yaw.sin_cos();
        for (i, obj) in self.props.iter().enumerate() {
            if obj.dead {
                continue;
            }
            let text = if obj.resource.is_some() && obj.stock > 0 {
                if obj.wood_tree() {
                    "[E] Рубить дерево"
                } else if obj.kind == "wreck" {
                    "[E] Разобрать остов"
                } else {
                    match obj.resource.as_deref() {
                        Some("wood") => "[E] Взять дерево",
                        Some("metal") => "[E] Взять металл",
                        Some("oil") => "[E] Взять нефть",
                        Some("gunpowder" | "powder") => "[E] Взять порох",
                        Some("ammo") => "[E] Взять патроны",
                        _ => continue,
                    }
                }
            } else if obj.built
                && obj.team.as_ref() == Some(&p.team)
                && obj.hp.unwrap_or(0.) + 1. < obj.max_hp.unwrap_or(0.)
            {
                "[E] Починить укрытие · 1 дерево"
            } else {
                continue;
            };
            if obj.footprint(p.x, p.z) > 2.35 * 2.35 {
                continue;
            }
            let dx = obj.x - p.x;
            let dz = obj.z - p.z;
            let distance = dx.hypot(dz);
            let face = if distance > 0.001 {
                (dx * sn - dz * cs) / distance
            } else {
                1.
            };
            let weight = distance + (1. - face) * 1.6;
            if face < -0.45 || weight >= nearest {
                continue;
            }
            if distance > 0.01 {
                let dy = (obj.h * 0.7).min(1.1) - 1.35;
                let len = dy.hypot(distance);
                let (obstacle, _) = self.ray([p.x, 1.35, p.z], [dx / len, dy / len, dz / len], len);
                if obstacle.is_some_and(|n| n != i) {
                    continue;
                }
            }
            nearest = weight;
            target = Some((i, text));
        }
        target
    }
    fn update_hint(&self, p: &mut Player) {
        if !p.build_mode {
            if let Some(h) = self.logistics.hint(p, &self.props) {
                p.hint = h;
                return;
            }
        }
        if !p.build_mode && self.logistics.driver_vehicle(p.id).is_none() {
            if let Some(h) = self.ground_hint(p) {
                p.hint = h;
                return;
            }
        }
        p.hint = if p.build_mode {
            "[ЛКМ] Построить · [Q] Выбрать · [B] Отмена"
        } else {
            self.target(p).map(|(_, t)| t).unwrap_or("")
        }
        .into();
    }
    fn interact(&mut self, p: &mut Player) {
        if p.build_mode {
            return;
        }
        if self.pickup_ground(p) {
            return;
        }
        let Some((i, _)) = self.target(p) else {
            return;
        };
        let o = &mut self.props[i];
        if o.built {
            if p.wood == 0 {
                p.notice("Для ремонта нужно 1 дерево");
                return;
            }
            p.wood -= 1;
            if o.layout.is_some() {
                let revision = o.fracture.revision.wrapping_add(1);
                let mut keep = o.fracture.broken.clone();
                for id in keep.iter().copied().take(3).collect::<Vec<_>>() {
                    keep.remove(&id);
                }
                o.fracture = FractureState::default();
                o.fracture.revision = revision;
                o.fracture.broken = keep;
                self.nav = None;
            }
            o.hp = Some((o.hp.unwrap_or(0.) + 85.).min(o.max_hp.unwrap_or(140.)));
            o.dirty = true;
            self.event("repair", p, json!({}));
            p.notice("Укрытие отремонтировано");
            return;
        }
        let timber = o.wood_tree();
        let Some(resource) = o.resource.as_deref() else {
            return;
        };
        let paced = i < self.level.props.len() && !timber && resource != "ammo";
        if paced && !self.ground.depot_ready(i, self.elapsed) {
            p.notice("Новая партия скоро будет готова");
            return;
        }
        if resource == "ammo" {
            p.reserve += 24;
            p.notice("+24 патрона");
        } else {
            let room = p.inventory_room(resource);
            if room == 0 {
                p.notice("Эта стопка полна · 32 ед.");
                return;
            }
            let (n, label) = match resource {
                "wood" => {
                    let n = room.min(if timber { 2 } else { 4 });
                    p.wood += n;
                    (n, "дерева")
                }
                "metal" => {
                    let n = room.min(2);
                    p.metal += n;
                    (n, "металла")
                }
                "oil" => {
                    let n = room.min(4);
                    p.oil += n;
                    (n, "нефти")
                }
                "gunpowder" | "powder" => {
                    let n = room.min(3);
                    p.powder += n;
                    (n, "пороха")
                }
                _ => return,
            };
            p.notice(&format!("+{n} {label}"));
        }
        if paced {
            self.ground.record_harvest(i, self.elapsed);
        }
        o.stock -= 1;
        o.dirty = true;
        if o.stock == 0 {
            if timber {
                o.hp = Some(0.);
                self.break_prop(i, p);
            } else if o.build_id.is_some() && !o.built {
                // Collecting the final salvage unit removes that physical pile.
                o.dead = true;
                self.nav = None;
            } else if o.capacity > 0 {
                o.restock_at = Some(self.elapsed + 45.);
            }
        }
        self.event("pickup", p, json!({}));
    }
    pub fn snapshot(&self) -> Value {
        let static_count = self.level.props.len();
        let deltas: Vec<_> = self.props[..static_count]
            .iter()
            .enumerate()
            .filter(|(_, p)| p.dirty)
            .map(|(i, p)| {
                let mut o = json!({"index":i,"dead":p.dead});
                if p.layout.is_some() {
                    o["broken"] = json!(p.fracture.broken);
                    o["revision"] = json!(p.fracture.revision);
                }
                if let Some(hp) = p.hp {
                    o["hp"] = hp.into();
                }
                if p.resource.is_some() {
                    o["stock"] = p.stock.into();
                }
                o
            })
            .collect();
        let dynamic: Vec<_> = self.props[static_count..]
            .iter()
            .map(|p| {
                if p.dead{return json!({"kind":p.kind,"buildId":p.build_id,"dead":true,"hp":0,"stock":0,"revision":p.fracture.revision});}
                let mut o = json!(p);
                if p.layout.is_some() {
                    o["broken"] = json!(p.fracture.broken);
                    o["revision"] = json!(p.fracture.revision);
                }
                o
            })
            .collect();
        let published: Vec<_> = self
            .players
            .values()
            .map(|p| {
                let mut out = json!(p);
                p.arsenal.publish(p.weapon.slot(), &mut out);
                out
            })
            .collect();
        let mut out = json!({"shells":self.shells,"grenades":self.grenades,"roundSeconds":self.round_seconds,"round":self.round,"phase":self.phase,"winner":self.winner,"elapsed":self.elapsed,"remaining":self.remaining,
            "nextRoundIn":self.next_round_in,"fragLimit":self.frag_limit,"scores":{"blue":self.blue,"red":self.red},
            "players":published,"propChanges":deltas,"builtProps":dynamic,"events":self.events});
        out["groundDrops"] = self.ground.snapshot();
        self.logistics.write_snapshot(&mut out);
        self.drones.snapshot(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skirmish(geometry: Value) -> Match {
        let level = serde_json::from_value(json!({
            "bounds":{"minX":-16,"maxX":16,"minZ":-16,"maxZ":16},
            "teamSpawns":{"blue":[{"x":0,"z":8,"yaw":0},{"x":2.5,"z":8,"yaw":0}],
                "red":[{"x":0,"z":-8,"yaw":3.14159}]},
            "props":geometry
        }))
        .unwrap();
        let mut m = Match::from_level(level);
        m.add_player(1, "Blue");
        m.add_player(2, "Red");
        m
    }
    fn advance(m: &mut Match, seconds: f32) {
        for _ in 0..(seconds / 0.05).ceil() as usize {
            m.step(0.05);
        }
    }
    fn act(m: &mut Match, input: Value) {
        m.input(1, &input);
        m.step(0.05);
        m.input(1, &json!({}));
        advance(m, 0.2);
    }
    fn shoot(m: &mut Match, yaw: f32, pitch: f32) {
        act(m, json!({"shoot":true,"yaw":yaw,"pitch":pitch}));
        advance(m, 0.15);
    }
    fn vehicle_lane(kind: &str, geometry: Value) -> Match {
        let level = serde_json::from_value(json!({
            "bounds":{"minX":-40,"maxX":40,"minZ":-85,"maxZ":36},"props":geometry,
            "teamSpawns":{"blue":[{"x":3.25,"z":20,"yaw":0}],"red":[{"x":15,"z":-55,"yaw":0}]},
            "vehicleSpawns":[{"kind":kind,"x":0,"z":20,"yaw":0,"team":"blue"}]
        }))
        .unwrap();
        let mut m = Match::from_level(level);
        m.add_player(1, "Driver");
        m.add_player(2, "Other");
        act(&mut m, json!({"vehicle":true}));
        assert_eq!(m.snapshot()["players"][0]["vehicleId"], 1);
        m
    }
    #[test]
    fn momentum_breaks_a_real_wooden_gap_but_an_intact_steel_wall_stops_the_hull() {
        let mut m = vehicle_lane(
            "mbt",
            json!([
                {"kind":"barricade","x":0,"z":3,"w":11.,"d":0.6,"h":1.55,"hp":140.,"solid":true},
                {"kind":"crate","style":"shippingContainer","x":0,"z":-13,"w":8.,"d":1.8,"h":3.,"hp":400.,"solid":true}
            ]),
        );
        assert!(m.props[0].intersects(0., 3., 1.45));
        let mut impacts = std::collections::BTreeSet::new();
        for _ in 0..200 {
            m.input(1, &json!({"forward":1}));
            m.step(0.05);
            for e in &m.events {
                if e["type"] == "vehicleImpact" {
                    impacts.insert(e["id"].as_u64().unwrap());
                }
            }
        }
        assert!(m.props[0].layout.is_some() && !m.props[0].fracture.broken.is_empty());
        assert!(!m.props[0].intersects(0., 3., 1.45) && m.props[0].intersects(5., 3., 0.4));
        assert_eq!(impacts.len(), 2);
        let state = m.snapshot();
        let z = state["vehicles"][0]["z"].as_f64().unwrap();
        assert!(z < -6. && z > -12.1, "stopped at {z}");
        let hp = m.props[1].hp.unwrap();
        assert!(hp > 365. && hp < 400.);
        for _ in 0..70 {
            m.input(1, &json!({"forward":1}));
            m.step(0.05);
        }
        assert_eq!(m.props[1].hp, Some(hp));
        assert!((m.snapshot()["vehicles"][0]["z"].as_f64().unwrap() - z).abs() < 0.05);
    }
    #[test]
    fn fast_heavy_vehicle_can_fell_a_tree_and_pole_and_roll_over_scattered_timber() {
        let mut m = vehicle_lane(
            "mbt",
            json!([
                {"kind":"tree","x":0,"z":5,"w":3.4,"h":6.0,"d":3.4,"collisionW":0.5,"collisionD":0.5,"solid":true},
                {"kind":"pole","x":0,"z":-18,"w":0.35,"h":4.,"d":0.35,"solid":true}
            ]),
        );
        for _ in 0..210 {
            m.input(1, &json!({"forward":1}));
            m.step(0.05);
        }
        assert!(m.props[0].dead && m.props[1].dead);
        assert!(m.players[&1].z < -24.);
        assert!(m
            .props
            .iter()
            .any(|o| o.debris_source == Some(0) && o.kind == "logs" && o.stock == 1));
    }
    #[test]
    fn shell_position_drops_during_flight_and_small_arms_leave_geometry_intact() {
        let mut m = vehicle_lane(
            "mbt",
            json!([{"kind":"fence","x":0,"z":-40,"w":7.,"h":4.,"d":0.3,"hp":110.,"solid":true}]),
        );
        for _ in 0..7 {
            m.input(1, &json!({"secondary":true,"yaw":0,"pitch":0}));
            m.step(0.05);
        }
        let st = m.snapshot();
        assert_eq!(st["vehicles"][0]["shells"], 8);
        assert!(st["vehicles"][0]["mgAmmo"].as_u64().unwrap() < 80);
        assert_eq!(m.props[0].hp, Some(110.));
        assert!(m
            .events
            .iter()
            .any(|e| e["type"] == "shot" && e["weapon"] == "vehicle_mg"));
        m.input(1, &json!({"shoot":true,"yaw":0,"pitch":0}));
        m.step(0.05);
        assert_eq!(m.snapshot()["shells"].as_array().unwrap().len(), 1);
        let start = m.shells[0].clone();
        m.input(1, &json!({}));
        advance(&mut m, 0.3);
        assert!(m.shells[0].y < start.y - 0.20 && m.shells[0].z < start.z - 24.);
        assert!(m.shells[0].vy < 0.);
        assert!(!m.props[0].dead && m.events.iter().all(|e| e["type"] != "shellImpact"));
        advance(&mut m, 0.40);
        assert!(m.props[0].dead && m.shells.is_empty());
        assert!(m.events.iter().any(|e| e["type"] == "shellImpact"
            && e["shellId"] == start.id
            && e["impactMaterial"] == "fence"));
        assert_eq!(m.players[&1].ammo, 8);
    }
    #[test]
    fn buggy_and_carrier_have_different_authoritative_roles_and_performance() {
        let mut speeds = Vec::new();
        for kind in ["scout_buggy", "apc"] {
            let mut m = vehicle_lane(kind, json!([]));
            for _ in 0..60 {
                m.input(1, &json!({"forward":1,"shoot":true}));
                m.step(0.05);
            }
            let st = m.snapshot();
            let v = &st["vehicles"][0];
            speeds.push(v["speed"].as_f64().unwrap());
            assert!(v["travel"].as_f64().unwrap() > 10.);
            if kind == "apc" {
                assert_eq!(v["cargoCapacity"], 24);
                assert!(v["mgAmmo"].as_u64().unwrap() < 100);
                assert_eq!(v["hp"], 390.);
            } else {
                assert_eq!(v["cargoCapacity"], 0);
                assert_eq!(v["mgAmmo"], 0);
                assert_eq!(v["hp"], 95.);
            }
            assert_eq!(v["shells"], 0);
        }
        assert!(speeds[0] > speeds[1] * 1.8);
    }
    #[test]
    fn utility_kits_expand_buggy_tank_and_apc_hold_and_reject_overloaded_swaps() {
        for vehicle in ["scout_buggy", "apc"] {
            let mut m = vehicle_lane(
                vehicle,
                json!([{"kind":"workshop","team":"blue","x":4,"z":20,"w":2.,"d":2.,"h":2.,"solid":true}]),
            );
            let pilot = m.players.get_mut(&1).unwrap();
            pilot.wood = 16;
            pilot.metal = 32;
            act(&mut m, json!({"upgradeSelect":true}));
            act(&mut m, json!({"upgrade":true}));
            let st = m.snapshot();
            let v = &st["vehicles"][0];
            assert_eq!(v["upgradeLevel"], 1);
            if vehicle == "scout_buggy" {
                assert!((v["maxFuel"].as_f64().unwrap() - 35.5).abs() < 0.01);
            } else {
                assert_eq!(v["cargoCapacity"], 36);
                act(
                    &mut m,
                    json!({"inventory":{"action":"load","resource":"metal","amount":26}}),
                );
                assert_eq!(m.snapshot()["vehicles"][0]["cargo"]["metal"], 26);
                act(&mut m, json!({"upgradeSelect":true}));
                let before = (m.players[&1].wood, m.players[&1].metal);
                act(&mut m, json!({"upgrade":true}));
                assert_eq!(m.snapshot()["vehicles"][0]["cargoCapacity"], 36);
                assert_eq!((m.players[&1].wood, m.players[&1].metal), before);
                act(
                    &mut m,
                    json!({"inventory":{"action":"unload","resource":"metal","amount":10}}),
                );
                act(&mut m, json!({"upgrade":true}));
                assert_eq!(m.snapshot()["vehicles"][0]["cargoCapacity"], 24);
            }
        }
    }
    #[test]
    fn workshop_fittings_track_the_weapon_and_bursts_finish_after_trigger_release() {
        let mut m = skirmish(
            json!([{"kind":"workshop","team":"blue","x":-2.3,"z":8,"w":1.5,"d":1.5,"h":2.,"solid":true}]),
        );
        act(
            &mut m,
            json!({"fitOptic":"reflex","fitGrip":"light","cycleMode":true}),
        );
        let player = &m.snapshot()["players"][0];
        assert_eq!(
            (&player["optic"], &player["grip"], &player["fireMode"]),
            (&json!("reflex"), &json!("light"), &json!("burst"))
        );
        m.input(2, &json!({"fitOptic":"reflex","fitGrip":"light"}));
        m.step(0.05);
        assert_eq!(m.snapshot()["players"][1]["optic"], "scope");
        act(&mut m, json!({"shoot":true,"yaw":0.8}));
        advance(&mut m, 1.);
        assert_eq!(m.players[&1].ammo, 6);
        act(&mut m, json!({"weapon":2}));
        assert_eq!(m.snapshot()["players"][0]["grip"], "standard");
        act(&mut m, json!({"weapon":1}));
        assert_eq!(m.snapshot()["players"][0]["grip"], "light");
        let mut p = m.players.remove(&1).unwrap();
        m.respawn(&mut p, false);
        m.players.insert(1, p);
        assert_eq!(m.snapshot()["players"][0]["fireMode"], "burst");
    }
    #[test]
    fn shipped_map_spawns_have_clearance_cover_and_supply() {
        let m = Match::new();
        let starts = &m.level.team_spawns;
        for team in ["blue", "red"] {
            for s in &starts[team] {
                assert!(!m.blocked(s.x, s.z, 0.52));
            }
            for resource in ["wood", "metal", "ammo"] {
                assert!(m
                    .props
                    .iter()
                    .any(|p| p.resource.as_deref() == Some(resource)
                        && p.stock > 0
                        && starts[team]
                            .iter()
                            .any(|s| (p.x - s.x).hypot(p.z - s.z) < 12.)));
            }
        }
        for a in &starts["blue"] {
            for b in &starts["red"] {
                let dx = b.x - a.x;
                let dz = b.z - a.z;
                let d = dx.hypot(dz);
                assert!(m.ray([a.x, EYE, a.z], [dx / d, 0., dz / d], d).0.is_some());
            }
        }
    }
    #[test]
    fn combat_authority_team_shield_hit_scoring_and_respawn() {
        let mut m = skirmish(json!([]));
        m.add_player(3, "Other Blue");
        m.add_player(4, "Other Red");
        assert_eq!(m.players[&3].team, "blue");
        assert_eq!(m.players[&4].team, "red");
        m.remove_player(4);
        shoot(&mut m, 0., 0.);
        assert_eq!(m.players[&2].hp, 100.);
        assert_eq!(m.players[&1].shield, 0.);
        advance(&mut m, 2.5);
        shoot(&mut m, PI / 2., 0.);
        assert_eq!(m.players[&3].hp, 100.);
        shoot(&mut m, 0., -0.045);
        assert_eq!(m.players[&2].hp, 45.);
        shoot(&mut m, 0., -0.045);
        assert!(!m.players[&2].alive);
        assert_eq!(m.blue, 1);
        assert_eq!(m.players[&1].kills, 1);
        assert_eq!(m.players[&2].deaths, 1);
        assert!(m.snapshot()["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "kill" && e["actor"] == 1 && e["target"] == 2));
        advance(&mut m, 4.1);
        assert!(m.players[&2].alive);
        assert!(m.players[&2].shield > 0.);
    }
    #[test]
    fn salvage_build_repair_restock_and_map_reset() {
        let mut m = skirmish(json!([
            {"kind":"tree","x":-2,"z":8,"w":3,"d":3,"h":6,"solid":true,"collisionW":0.45,"collisionD":0.45},
            {"kind":"patrol4x4","x":0,"z":4,"w":1.9,"d":3.8,"h":1.8,"solid":true,"hp":50},
            {"kind":"crate","x":0,"z":10,"w":1,"d":1,"h":1,"solid":true,"resource":"ammo","stock":1}
        ]));
        for _ in 0..3 {
            act(&mut m, json!({"interact":true,"yaw":-PI/2.}));
        }
        assert_eq!(m.players[&1].wood, 6);
        assert!(m.props[0].dead);
        shoot(&mut m, 0., 0.);
        assert_eq!(m.props[1].hp, Some(50.));
        let soldier = m.players[&1].clone();
        m.launch_shell(&soldier, [0., EYE, 8.], 0., 0., 0);
        advance(&mut m, 0.1);
        assert!(m.props[1].dead);
        assert_eq!(m.props[3].kind, "wreck");
        for _ in 0..3 {
            act(&mut m, json!({"interact":true,"yaw":0}));
        }
        assert_eq!(m.players[&1].metal, 6);
        act(&mut m, json!({"interact":true,"yaw":PI}));
        assert_eq!(m.props[2].stock, 0);
        assert_eq!(m.players[&1].reserve, 64);
        advance(&mut m, 46.);
        assert_eq!(m.props[2].stock, 1);
        act(&mut m, json!({"build":true,"yaw":-PI/2.}));
        assert_eq!(m.players[&1].preview.as_ref().unwrap()["valid"], true);
        act(&mut m, json!({"place":true,"yaw":-PI/2.}));
        let wall = m
            .props
            .iter()
            .position(|p| p.built && !p.dead)
            .expect("live barricade");
        assert!(m.props[wall].built);
        assert_eq!(m.players[&1].wood, 3);
        shoot(&mut m, -PI / 2., -0.21);
        assert_eq!(m.props[wall].hp, Some(140.));
        let (x, z) = (m.props[wall].x, m.props[wall].z);
        m.damage_prop(wall, [x, 0.7, z], Damage::Ram(14.), 25., &soldier);
        assert_eq!(m.props[wall].hp, Some(115.));
        act(&mut m, json!({"interact":true,"yaw":-PI/2.}));
        assert_eq!(m.props[wall].hp, Some(140.));
        assert_eq!(m.players[&1].wood, 2);
        let old_round = m.round;
        m.remaining = 1.0;
        let t = m.remaining;
        advance(&mut m, t as f32 + 0.2);
        assert_eq!(m.phase, "ended");
        advance(&mut m, 10.1);
        assert_eq!(m.round, old_round + 1);
        assert_eq!(m.props.len(), 3);
        assert_eq!(m.props[0].stock, 3);
        assert_eq!(m.props[1].hp, Some(50.));
        assert_eq!(m.players[&1].wood, 0);
    }
    #[test]
    fn explicit_teams_points_and_recent_assists_are_authoritative() {
        let mut m = skirmish(json!([]));
        m.add_player_with_team(3, "Blue helper", "blue");
        m.add_player_with_team(4, "Third Blue", "blue");
        assert_eq!(m.players[&4].team, "blue");
        m.remove_player(4);
        advance(&mut m, 2.5);
        shoot(&mut m, 0., -0.045);
        assert_eq!(m.players[&2].hp, 45.0);
        let shooter = &m.players[&3];
        let target = &m.players[&2];
        let dx = target.x - shooter.x;
        let dz = target.z - shooter.z;
        let yaw = dx.atan2(-dz);
        let pitch = (1.1 - EYE).atan2(dx.hypot(dz));
        m.input(3, &json!({"shoot":true,"yaw":yaw,"pitch":pitch}));
        m.step(0.05);
        m.input(3, &json!({}));
        assert_eq!((m.players[&1].points, m.players[&1].assists), (50, 1));
        assert_eq!((m.players[&3].points, m.players[&3].kills), (100, 1));
        assert_eq!(m.blue, 1);
        assert!(m
            .events
            .iter()
            .any(|e| e["type"] == "kill" && e["assistIds"] == json!([1])));
        advance(&mut m, 6.6);
        shoot(&mut m, 0., -0.045);
        advance(&mut m, 10.4);
        m.input(3, &json!({"shoot":true,"yaw":yaw,"pitch":pitch}));
        m.step(0.05);
        assert_eq!(m.players[&3].points, 200);
        assert_eq!(m.players[&1].points, 50);
    }
    #[test]
    fn timed_grenade_has_finite_charges_and_preblast_cover_protects_players() {
        fn arena(covered: bool) -> Match {
            let mut m = skirmish(if covered {
                json!([{"kind":"barricade","x":0,"z":-1.0,"w":6,"d":0.45,"h":2.3,"solid":true,"hp":180}])
            } else {
                json!([])
            });
            m.players.get_mut(&2).unwrap().z = -1.85;
            advance(&mut m, 2.5);
            m
        }
        for covered in [false, true] {
            let mut m = arena(covered);
            act(&mut m, json!({"grenade":true,"yaw":0,"pitch":0}));
            assert_eq!(m.players[&1].grenades, 0);
            let f = m.snapshot();
            assert_eq!(f["grenades"].as_array().unwrap().len(), 1);
            assert_eq!(f["grenades"][0]["actor"], 1);
            assert_eq!(m.players[&2].hp, 100.);
            act(&mut m, json!({"grenade":true}));
            assert_eq!(m.grenades.len(), 1);
            advance(&mut m, 1.50);
            assert!(m.grenades.is_empty());
            assert!(m.events.iter().any(|e| e["type"] == "explosion"));
            if covered {
                assert_eq!(m.players[&2].hp, 100.);
                assert!(!m.props[0].fracture.broken.is_empty());
                assert!(m.snapshot()["propChanges"][0]["revision"].as_u64().unwrap() > 0);
            } else {
                assert!(m.players[&2].hp < 100.);
            }
        }
    }
    #[test]
    fn menu_actions_select_structure_and_require_grenade_recipe_at_friendly_workshop() {
        let mut m = skirmish(
            json!([{"kind":"workshop","team":"blue","x":-2.5,"z":8,"w":1.5,"d":1.5,"h":2.0,"solid":true}]),
        );
        {
            let p = m.players.get_mut(&1).unwrap();
            p.powder = 2;
            p.metal = 1;
        }
        act(&mut m, json!({"craft":"grenade"}));
        assert_eq!(
            (
                m.players[&1].grenades,
                m.players[&1].powder,
                m.players[&1].metal
            ),
            (2, 0, 0)
        );
        act(&mut m, json!({"craft":"grenade"}));
        assert_eq!(m.players[&1].grenades, 2);
        act(&mut m, json!({"craft":"workshop"}));
        assert!(m.players[&1].build_mode);
        assert_eq!(m.players[&1].preview.as_ref().unwrap()["kind"], "workshop");
        act(&mut m, json!({"craft":"wall"}));
        assert_eq!(m.players[&1].preview.as_ref().unwrap()["kind"], "barricade");
        m.players.get_mut(&2).unwrap().powder = 2;
        m.players.get_mut(&2).unwrap().metal = 1;
        m.input(2, &json!({"craft":"grenade"}));
        m.step(0.05);
        assert_eq!((m.players[&2].grenades, m.players[&2].powder), (1, 2));
    }
    #[test]
    fn local_tank_breach_changes_actual_collision_and_late_join_snapshot() {
        let mut m = skirmish(
            json!([{"kind":"house","x":-4,"z":4,"w":8,"d":7,"h":6,"solid":true,"hp":420}]),
        );
        let o = [3.0, 1.1, 4.0];
        let d = [-1.0, 0.0, 0.0];
        assert_eq!(m.ray(o, d, 4.).0, Some(0));
        assert!(m.blocked(-0.1, 4., 0.36));
        let p = m.players.remove(&1).unwrap();
        m.launch_shell(&p, o, -PI * 0.5, 0., 0);
        m.players.insert(1, p);
        assert!(m.blocked(-0.1, 4., 0.36));
        advance(&mut m, 0.1);
        assert!(!m.props[0].dead);
        assert_eq!(m.ray(o, d, 4.).0, None);
        assert!(!m.blocked(-0.1, 4., 0.36));
        assert!(m.events.iter().any(|e| e["type"] == "fracture"
            && e["kind"] == "house"
            && !e["cells"].as_array().unwrap().is_empty()));
        let before = m.snapshot()["propChanges"][0].clone();
        advance(&mut m, 2.5);
        m.add_player_with_team(8, "Late", "red");
        assert_eq!(m.snapshot()["propChanges"][0], before);
        assert!(!before["broken"].as_array().unwrap().is_empty());
    }
    #[test]
    fn observer_controls_move_only_drone_and_opponents_can_destroy_it_without_a_frag() {
        let mut m = skirmish(json!([]));
        advance(&mut m, 2.5);
        let (bx, bz) = (m.players[&1].x, m.players[&1].z);
        m.input(1,&json!({"drone":true,"forward":1,"yaw":0.0,"pitch":-0.85,"shoot":true,"grenade":true,"ping":true}));
        m.step(0.05);
        let first = m.snapshot();
        assert_eq!(first["players"][0]["droneId"], 1);
        assert_eq!(first["markers"][0]["team"], "blue");
        for _ in 0..8 {
            m.input(
                1,
                &json!({"forward":1,"yaw":0,"pitch":-0.85,"shoot":true,"build":true}),
            );
            m.step(0.05);
        }
        m.input(1, &json!({}));
        let s = m.snapshot();
        assert_eq!((m.players[&1].x, m.players[&1].z), (bx, bz));
        assert_eq!(m.players[&1].ammo, 8);
        assert_eq!(m.players[&1].grenades, 1);
        assert!(!m.players[&1].build_mode);
        let drone = &s["drones"][0];
        let dx = drone["x"].as_f64().unwrap() as f32 - m.players[&2].x;
        let dz = drone["z"].as_f64().unwrap() as f32 - m.players[&2].z;
        let dy = drone["y"].as_f64().unwrap() as f32 - EYE;
        assert!(drone["z"].as_f64().unwrap() < bz as f64 - 2.);
        m.input(
            2,
            &json!({"shoot":true,"yaw":dx.atan2(-dz),"pitch":dy.atan2(dx.hypot(dz))}),
        );
        m.step(0.05);
        let s = m.snapshot();
        assert!(s["drones"].as_array().unwrap().is_empty());
        assert!(s["players"][0]["droneCooldown"].as_f64().unwrap() > 40.);
        assert!(m
            .events
            .iter()
            .any(|e| e["type"] == "droneDown" && e["droneId"] == 1));
        assert_eq!((m.players[&1].hp, m.players[&2].kills, m.red), (100., 0, 0));
    }
    fn campaign_build(team: &str, kind: &str, x: f32, z: f32) -> Prop {
        Prop {
            kind: kind.into(),
            team: Some(team.into()),
            built: true,
            solid: true,
            x,
            z,
            w: 2.,
            d: 2.,
            h: 2.,
            hp: Some(160.),
            max_hp: Some(160.),
            ..Default::default()
        }
    }
    #[test]
    fn team_and_debris_caps_preserve_materials_and_reuse_one_slot_with_fresh_identity() {
        let mut m = skirmish(json!([]));
        let start_count = m.props.len();
        for team in ["blue", "red"] {
            for _ in 0..TEAM_BUILD_LIMIT {
                assert!(m
                    .dynamic(campaign_build(team, "workshop", 12., 12.))
                    .is_some());
            }
            assert!(m
                .dynamic(campaign_build(team, "workshop", 12., 12.))
                .is_none());
        }
        for i in 0..DEBRIS_LIMIT * 3 {
            assert!(m
                .dynamic(Prop {
                    kind: "logs".into(),
                    debris_source: Some(1000 + i),
                    resource: Some("wood".into()),
                    x: -13.,
                    z: -12.,
                    w: 1.,
                    d: 1.,
                    h: 1.,
                    stock: 1,
                    solid: true,
                    ..Default::default()
                })
                .is_some());
        }
        assert_eq!(m.props.len() - start_count, DYNAMIC_LIMIT);
        let first = m.props[0].build_id;
        {
            let p = m.players.get_mut(&1).unwrap();
            p.wood = 4;
            p.metal = 2;
        }
        act(&mut m, json!({"craft":"wall","yaw":0}));
        act(&mut m, json!({"place":true,"yaw":0}));
        assert_eq!(m.players[&1].preview.as_ref().unwrap()["teamLimit"], true);
        assert_eq!((m.players[&1].wood, m.players[&1].metal), (4, 2));
        let p = m.players[&1].clone();
        m.break_prop(0, &p);
        let tomb = m.snapshot()["builtProps"][0].clone();
        assert_eq!(tomb["dead"], true);
        assert_eq!(tomb["buildId"], first.unwrap());
        act(&mut m, json!({"place":true,"yaw":0}));
        assert_eq!((m.players[&1].wood, m.players[&1].metal), (1, 1));
        assert_eq!(m.props[0].kind, "barricade");
        assert!(m.props[0].build_id > first);
        // Repeated destroy/rebuild cycles do not grow slot storage or event history.
        let old_identity = m.props[0].build_id;
        for _ in 0..300 {
            m.break_prop(0, &p);
            assert_eq!(
                m.dynamic(campaign_build("blue", "refinery", 12., 12.)),
                Some(0)
            );
        }
        assert_eq!(m.props.len() - start_count, DYNAMIC_LIMIT);
        assert!(m.events.len() <= 144);
        assert!(m.props[0].build_id > old_identity);
        // A tree's new harvest replaces its earlier pile, never adding stock to it.
        let log = Prop {
            kind: "logs".into(),
            debris_source: Some(99999),
            resource: Some("wood".into()),
            stock: 1,
            ..Default::default()
        };
        let slot = m.dynamic(log.clone()).unwrap();
        let id = m.props[slot].build_id;
        assert_eq!(m.dynamic(log), Some(slot));
        assert!(m.props[slot].build_id > id);
        assert_eq!(m.props[slot].stock, 1);
        assert_eq!(m.props.len() - start_count, DYNAMIC_LIMIT);
    }
    #[test]
    fn same_tick_refinery_replacement_discards_old_orders_and_unclaimed_fuel() {
        for delay in [0.0, 12.2] {
            let mut m = skirmish(json!([]));
            let source = campaign_build("blue", "refinery", -2.2, 8.);
            let i = m.dynamic(source.clone()).unwrap();
            m.players.get_mut(&1).unwrap().oil = 4;
            act(&mut m, json!({"craft":"refine"}));
            advance(&mut m, delay);
            let s = m.snapshot();
            assert!(s["factories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["pending"] == 6 || f["ready"] == 6));
            let soldier = m.players[&1].clone();
            let id = m.props[i].build_id;
            m.break_prop(i, &soldier);
            assert_eq!(m.dynamic(source.clone()), Some(i));
            assert_ne!(m.props[i].build_id, id);
            advance(&mut m, 12.2);
            act(&mut m, json!({"craft":"refine"}));
            assert_eq!((m.players[&1].oil, m.players[&1].fuel), (0, 0));
            assert!(m.snapshot()["factories"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["ready"] == 0 && f["pending"] == 0));
        }
    }
    #[test]
    fn hundred_infantry_hits_do_not_break_solid_metal_but_heavier_game_impacts_do() {
        for (kind, style) in [
            ("crate", "shippingContainer"),
            ("barrels", "tankBattery"),
            ("mbt", ""),
        ] {
            let mut m = skirmish(
                json!([{"kind":kind,"style":style,"x":0,"z":4,"w":2.,"d":2.,"h":2.5,"solid":true,"hp":200.}]),
            );
            let mut p = m.players.remove(&1).unwrap();
            for _ in 0..100 {
                m.infantry_ray(&mut p, [0., EYE, 8.], 0., 0.);
            }
            assert_eq!(m.props[0].hp, Some(200.));
            assert!(!m.props[0].dead);
            assert!(m
                .events
                .iter()
                .any(|e| e["type"] == "shot" && e["impactMaterial"] == "metal"));
            m.damage_prop(0, [0., 1., 5.], Damage::Grenade, 80., &p);
            assert_eq!(m.props[0].hp, Some(180.));
            m.launch_shell(&p, [0., EYE, 8.], 0., 0., 0);
            advance(&mut m, 0.1);
            assert_eq!(m.props[0].hp, Some(25.));
            m.launch_shell(&p, [0., EYE, 8.], 0., 0., 0);
            advance(&mut m, 0.1);
            assert!(m.props[0].dead);
        }
        let mut m = skirmish(json!([]));
        m.level.vehicle_spawns =
            serde_json::from_value(json!([{"kind":"mbt","team":"red","x":0,"z":0,"yaw":0}]))
                .unwrap();
        m.reset();
        let mut p = m.players.remove(&1).unwrap();
        let health = m.snapshot()["vehicles"][0]["hp"].as_f64().unwrap();
        for _ in 0..100 {
            m.infantry_ray(&mut p, [0., EYE, 8.], 0., 0.);
        }
        assert_eq!(m.snapshot()["vehicles"][0]["hp"].as_f64().unwrap(), health);
        m.launch_shell(&p, [0., EYE, 8.], 0., 0., 0);
        advance(&mut m, 0.1);
        assert_eq!(
            m.snapshot()["vehicles"][0]["hp"].as_f64().unwrap(),
            health - 135.
        );
    }
    #[test]
    fn all_sidegrades_preserve_their_magazines_and_reload_progress_is_cancelled_on_swap() {
        let mut m = skirmish(json!([]));
        shoot(&mut m, 0.8, 0.);
        let rifle_left = m.players[&1].ammo;
        act(&mut m, json!({"reload":true}));
        assert!(m.players[&1].reload > 0.);
        act(&mut m, json!({"weapon":2}));
        let p = &m.players[&1];
        assert_eq!(
            (p.weapon, p.ammo, p.reserve, p.mag_size),
            (Weapon::Vx9, 20, 80, 20)
        );
        assert_eq!(p.reload, 0.);
        advance(&mut m, 0.4);
        for _ in 0..12 {
            m.input(1, &json!({"shoot":true,"yaw":0.8,"pitch":0}));
            m.step(0.05);
        }
        let smg_left = m.players[&1].ammo;
        assert!((14..=17).contains(&smg_left));
        act(&mut m, json!({"weapon":"mr7"}));
        assert_eq!(
            (m.players[&1].ammo, m.players[&1].reserve),
            (rifle_left, 40)
        );
        act(&mut m, json!({"weapon":"ar4"}));
        assert_eq!(m.players[&1].mag_size, 15);
        act(&mut m, json!({"weapon":"vx9"}));
        assert_eq!(m.players[&1].ammo, smg_left);
        let rifle = Weapon::Mr7.stats();
        let smg = Weapon::Vx9.stats();
        let ar4 = Weapon::Ar4.stats();
        assert!(
            rifle.damage(85.0, false) > smg.damage(85.0, false) * 2.0
                && ar4.mobility > rifle.mobility
        );
        assert!(smg.cooldown < ar4.cooldown);
        assert!(rifle.range > ar4.range && ar4.range > smg.range);
        assert_eq!(m.snapshot()["roundSeconds"], 7200);
        assert_eq!(m.snapshot()["fragLimit"], 0);
    }
    #[test]
    fn inventory_cap_stock_and_destroyed_depot_reconstruction() {
        let mut m = skirmish(
            json!([{"kind":"crate","x":0,"z":6,"w":1.2,"d":1.2,"h":1.9,"solid":true,"resource":"gunpowder","stock":4,"hp":25}]),
        );
        m.players.get_mut(&1).unwrap().wood = 31;
        m.players.get_mut(&1).unwrap().powder = 31;
        act(&mut m, json!({"interact":true,"yaw":0}));
        assert_eq!(m.players[&1].powder, 32);
        assert_eq!(m.props[0].stock, 3);
        act(&mut m, json!({"interact":true,"yaw":0}));
        assert_eq!(m.props[0].stock, 3);
        shoot(&mut m, 0., 0.);
        assert!(!m.props[0].dead);
        let soldier = m.players[&1].clone();
        m.launch_shell(&soldier, [0., EYE, 8.], 0., 0., 0);
        advance(&mut m, 0.1);
        assert!(m.props[0].dead);
        assert!(m.events.iter().any(|e| e["type"] == "shot"
            && e["impactMaterial"] == "crate"
            && e["weapon"] == "mr7"));
        advance(&mut m, 118.);
        assert!(m.props[0].dead);
        advance(&mut m, 3.);
        assert!(!m.props[0].dead);
        assert_eq!(m.props[0].hp, Some(25.));
        assert_eq!(m.props[0].stock, 4);
        assert_eq!(m.snapshot()["propChanges"][0]["dead"], false);
    }
    #[test]
    fn facilities_charge_their_full_price_and_reject_overlapping_footprints() {
        let mut m =
            skirmish(json!([{"kind":"stone","x":1.57,"z":4,"w":0.6,"d":0.6,"h":1,"solid":true}]));
        {
            let p = m.players.get_mut(&1).unwrap();
            p.wood = 18;
            p.metal = 18;
        }
        act(&mut m, json!({"build":true,"cycleBuild":true,"yaw":0}));
        assert_eq!(m.players[&1].build_kind, "refinery");
        assert_eq!(m.players[&1].preview.as_ref().unwrap()["valid"], false);
        act(&mut m, json!({"place":true,"yaw":0}));
        assert_eq!(m.players[&1].wood, 18);
        act(&mut m, json!({"place":true,"yaw":-PI/2.}));
        assert_eq!(m.props.last().unwrap().kind, "refinery");
        assert_eq!((m.players[&1].wood, m.players[&1].metal), (10, 12));
        act(&mut m, json!({"build":true,"cycleBuild":true,"yaw":PI}));
        act(&mut m, json!({"place":true,"yaw":PI}));
        assert_eq!(m.props.last().unwrap().kind, "workshop");
        assert_eq!((m.players[&1].wood, m.players[&1].metal), (4, 4));
    }
    #[test]
    fn inputs_drive_fire_through_own_hull_and_eject_both_drivers_safely() {
        let level = serde_json::from_value(json!({
            "bounds":{"minX":-36,"maxX":36,"minZ":-36,"maxZ":36},"props":[],
            "teamSpawns":{"blue":[{"x":3.5,"z":10,"yaw":0}],"red":[{"x":3,"z":-10,"yaw":3.14159}]},
            "vehicleSpawns":[{"kind":"mbt","x":0,"z":10,"team":"blue","yaw":0},
                {"kind":"cargo6x6","x":0,"z":-10,"team":"red","yaw":3.14159}]
        }))
        .unwrap();
        let mut m = Match::from_level(level);
        m.add_player(1, "Driver");
        m.add_player(2, "Driver 2");
        m.input(2, &json!({"vehicle":true}));
        act(&mut m, json!({"vehicle":true}));
        assert_eq!(m.snapshot()["players"][0]["vehicleId"], 1);
        assert_eq!(m.snapshot()["players"][1]["vehicleId"], 2);
        for _ in 0..14 {
            m.input(1, &json!({"forward":1,"yaw":0.0}));
            m.step(0.05);
        }
        m.input(1, &json!({}));
        advance(&mut m, 0.6);
        let s = m.snapshot();
        assert!(s["vehicles"][0]["z"].as_f64().unwrap() < 9.5);
        let initial = s["vehicles"][0]["hp"].as_f64().unwrap();
        shoot(&mut m, 0., -0.02);
        let s = m.snapshot();
        assert_eq!(s["vehicles"][0]["hp"].as_f64().unwrap(), initial);
        assert_eq!(s["vehicles"][0]["shells"], 7);
        assert_eq!(m.players[&1].ammo, 8);
        assert_eq!(s["vehicles"][1]["hp"], 135.0);
        assert_eq!(m.players[&2].hp, 100.);
        assert!(s["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "shellImpact"
                && e["weapon"] == "tank"
                && e["impactMaterial"] == "metal"));
        advance(&mut m, 3.8);
        shoot(&mut m, 0., -0.02);
        let s = m.snapshot();
        assert_eq!(s["vehicles"][1]["destroyed"], true);
        assert!(s["players"][1]["vehicleId"].is_null());
        assert!(!m.blocked(m.players[&2].x, m.players[&2].z, RADIUS));
        act(&mut m, json!({"vehicle":true}));
        let s = m.snapshot();
        assert!(s["players"][0]["vehicleId"].is_null());
        assert!(!m.blocked(m.players[&1].x, m.players[&1].z, RADIUS));
    }
    #[test]
    fn walking_from_well_to_factory_produces_fuel_via_normal_inputs() {
        let mut m = skirmish(json!([
            {"kind":"oilwell","x":-2,"z":8,"w":1,"d":1,"h":1.8,"solid":true,"resource":"oil","stock":4},
            {"kind":"refinery","team":"blue","x":6,"z":8,"w":2,"d":2,"h":2,"solid":true}
        ]));
        act(&mut m, json!({"yaw":-PI/2.0,"interact":true}));
        assert_eq!(m.players[&1].oil, 4);
        for _ in 0..15 {
            m.input(1, &json!({"forward":1,"yaw":PI/2.0}));
            m.step(0.05);
        }
        act(&mut m, json!({"interact":true,"yaw":PI/2.0}));
        assert_eq!(m.players[&1].oil, 0);
        assert!(m.snapshot()["factories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["pending"] == 6));
        advance(&mut m, 12.2);
        act(&mut m, json!({"interact":true,"yaw":PI/2.0}));
        assert_eq!(m.players[&1].fuel, 6);
        assert_eq!(m.props[0].stock, 3);
    }
    #[test]
    fn headless_bot_count_changes_do_not_change_human_players() {
        let mut m = skirmish(json!([]));
        m.set_bots(10);
        assert_eq!(m.players.len(), 12);
        m.set_bots(4);
        assert_eq!(m.players.len(), 6);
        m.set_bots(0);
        assert!(m.brains.is_empty());
        assert!(m.players.contains_key(&1) && m.players.contains_key(&2));
    }
    #[test]
    fn frag_limit_and_reloading_end_round() {
        let mut m = skirmish(json!([]));
        m.set_frag_limit(20);
        for frag in 1..=m.frag_limit {
            advance(&mut m, 6.8);
            if m.players[&1].ammo == 0 {
                act(&mut m, json!({"reload":true}));
                advance(&mut m, 2.2);
            }
            shoot(&mut m, 0., 0.);
            assert_eq!(m.blue, frag);
        }
        assert_eq!(m.phase, "ended");
        assert_eq!(m.winner, Some("blue"));
        assert!(m.remaining > 0.);
    }
    #[test]
    fn configurable_three_hour_campaign_preserves_elapsed_and_carries_across_rounds() {
        let mut m = skirmish(json!([]));
        assert_eq!(m.snapshot()["roundSeconds"], 7200);
        advance(&mut m, 4.0);
        m.set_round_seconds(10800);
        assert!((m.remaining - 10796.0).abs() < 0.01);
        assert_eq!(m.snapshot()["roundSeconds"], 10800);
        m.remaining = 0.05;
        m.step(0.1);
        assert_eq!(m.phase, "ended");
        advance(&mut m, 10.1);
        assert!(m.remaining > 10799.8);
    }
    #[test]
    fn input_caps_speed_and_rejects_replayed_or_stale_movement() {
        let mut m = skirmish(json!([]));
        m.input(
            1,
            &json!({"seq":1,"forward":9999,"strafe":9999,"yaw":0,"pitch":200}),
        );
        m.step(0.1);
        assert!((m.players[&1].x.hypot(m.players[&1].z - 8.) - 0.425).abs() < 0.002);
        assert_eq!(m.players[&1].pitch, 1.28);
        m.input(1, &json!({"seq":1,"forward":-1,"yaw":PI}));
        assert_eq!(m.players[&1].yaw, 0.);
        advance(&mut m, 0.7);
        let last = m.players[&1].x;
        advance(&mut m, 0.5);
        assert_eq!(m.players[&1].x, last);
        let snap = m.snapshot();
        assert!(snap["players"][0].get("control").is_none());
    }
}

// Training is an explicit opt-in. Its three soldiers submit the same controls
// as connected peers: their weapons, spawn shields, inventory and scoring use
// no different rules. Ordinary matches allocate no AI navigation data.
#[derive(Default)]
struct Brain {
    rng: u32,
    target: u32,
    next_plan: f32,
    next_input: f32,
    next_fire: f32,
    path: VecDeque<(f32, f32)>,
    last_position: Option<(f32, f32)>,
}
impl Brain {
    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.rng >> 8) as f32 / 16777216.0
    }
}

struct NavGrid {
    bounds: Bounds,
    cols: i32,
    rows: i32,
    free: Vec<bool>,
}
impl NavGrid {
    const CELL: f32 = 2.0;
    fn bake(m: &Match) -> Self {
        let bounds = m.level.bounds.clone();
        let cols = ((bounds.max_x - bounds.min_x) / Self::CELL).floor() as i32;
        let rows = ((bounds.max_z - bounds.min_z) / Self::CELL).floor() as i32;
        let mut grid = Self {
            bounds,
            cols,
            rows,
            free: vec![true; (cols * rows) as usize],
        };
        for p in &m.props {
            if p.dead || !p.solid {
                continue;
            }
            let r = RADIUS + 0.12;
            let x0 = (((p.x - p.world_rx - r - grid.bounds.min_x) / Self::CELL).floor() as i32)
                .clamp(0, cols - 1);
            let x1 = (((p.x + p.world_rx + r - grid.bounds.min_x) / Self::CELL).ceil() as i32)
                .clamp(0, cols - 1);
            let z0 = (((p.z - p.world_rz - r - grid.bounds.min_z) / Self::CELL).floor() as i32)
                .clamp(0, rows - 1);
            let z1 = (((p.z + p.world_rz + r - grid.bounds.min_z) / Self::CELL).ceil() as i32)
                .clamp(0, rows - 1);
            for row in z0..=z1 {
                for col in x0..=x1 {
                    let id = (row * cols + col) as usize;
                    let (x, z) = grid.point(id);
                    if p.intersects(x, z, r) {
                        grid.free[id] = false;
                    }
                }
            }
        }
        grid
    }
    fn point(&self, i: usize) -> (f32, f32) {
        (
            self.bounds.min_x + ((i as i32 % self.cols) as f32 + 0.5) * Self::CELL,
            self.bounds.min_z + ((i as i32 / self.cols) as f32 + 0.5) * Self::CELL,
        )
    }
    fn free_index(&self, x: i32, z: i32) -> Option<usize> {
        if x < 0 || x >= self.cols || z < 0 || z >= self.rows {
            return None;
        }
        let i = (z * self.cols + x) as usize;
        self.free[i].then_some(i)
    }
    fn nearest(&self, px: f32, pz: f32) -> Option<usize> {
        let col = ((px - self.bounds.min_x) / Self::CELL).floor() as i32;
        let row = ((pz - self.bounds.min_z) / Self::CELL).floor() as i32;
        let mut best = None;
        let mut dist = f32::INFINITY;
        for z in row - 2..=row + 2 {
            for x in col - 2..=col + 2 {
                if let Some(i) = self.free_index(x, z) {
                    let (wx, wz) = self.point(i);
                    let d = (wx - px).hypot(wz - pz);
                    if d < dist {
                        dist = d;
                        best = Some(i);
                    }
                }
            }
        }
        best
    }
    fn route(&self, from: (f32, f32), to: (f32, f32)) -> VecDeque<(f32, f32)> {
        let mut route = VecDeque::new();
        let Some(start) = self.nearest(from.0, from.1) else {
            return route;
        };
        let Some(goal) = self.nearest(to.0, to.1) else {
            return route;
        };
        let (gx, gz) = (goal as i32 % self.cols, goal as i32 / self.cols);
        let mut parent = vec![usize::MAX; self.free.len()];
        let mut cost = vec![i32::MAX; self.free.len()];
        let mut frontier = BinaryHeap::new();
        cost[start] = 0;
        frontier.push(Reverse((0, 0, start)));
        while let Some(Reverse((_, g, id))) = frontier.pop() {
            if id == goal {
                let mut q = id;
                while q != start {
                    route.push_front(self.point(q));
                    q = parent[q];
                }
                return route;
            }
            if g > cost[id] {
                continue;
            }
            let col = id as i32 % self.cols;
            let row = id as i32 / self.cols;
            for (dx, dz) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (-1, 1),
                (1, -1),
                (-1, -1),
            ] {
                let Some(next) = self.free_index(col + dx, row + dz) else {
                    continue;
                };
                let diagonal = dx != 0 && dz != 0;
                if diagonal
                    && (self.free_index(col + dx, row).is_none()
                        || self.free_index(col, row + dz).is_none())
                {
                    continue;
                }
                let val = g + if diagonal { 14 } else { 10 };
                if val >= cost[next] {
                    continue;
                }
                let x = (gx - col - dx).abs();
                let z = (gz - row - dz).abs();
                cost[next] = val;
                parent[next] = id;
                frontier.push(Reverse((val + 10 * x.max(z) + 4 * x.min(z), val, next)));
            }
        }
        route
    }
}

impl Match {
    /// Keep the time already played when updating the configured round length.
    pub fn set_round_seconds(&mut self, seconds: u32) {
        let seconds = seconds.clamp(7200, 10800);
        let played = f64::from(self.round_seconds) - self.remaining;
        self.round_seconds = seconds;
        self.remaining = (f64::from(seconds) - played).max(0.0);
    }
    /// A limit of zero leaves the result to the campaign clock.
    pub fn set_frag_limit(&mut self, limit: u32) {
        self.frag_limit = limit;
    }
    pub fn enable_training(&mut self) {
        self.set_bots(3);
    }
    pub fn set_bots(&mut self, count: usize) {
        let count = count
            .min(10)
            .min(12 - self.players.values().filter(|p| !p.bot).count());
        let old: Vec<_> = self.brains.keys().copied().collect();
        for id in old.into_iter().skip(count) {
            self.remove_player(id);
        }
        const NAMES: [&str; 10] = [
            "Шульц",
            "Морель",
            "Фогель",
            "Бейкер",
            "Вебер",
            "Мартен",
            "Бек",
            "Леви",
            "Кляйн",
            "Рид",
        ];
        for (i, name) in NAMES.iter().enumerate().take(count) {
            let id = 1_000_000 + i as u32;
            if self.players.contains_key(&id) {
                continue;
            }
            self.add_player(id, name);
            if let Some(p) = self.players.get_mut(&id) {
                p.bot = true;
                p.switch_weapon([Weapon::Mr7, Weapon::Ar4, Weapon::Vx9][i % 3]);
            } else {
                continue;
            }
            self.brains.insert(
                id,
                Brain {
                    rng: 59567 + id * 3,
                    ..Default::default()
                },
            );
        }
    }
    fn practice_inputs(&mut self) {
        if self.brains.is_empty() {
            return;
        }
        if self.nav.is_none() {
            self.nav = Some(NavGrid::bake(self));
        }
        let ids: Vec<_> = self.brains.keys().copied().collect();
        for id in ids {
            let mut brain = self.brains.remove(&id).unwrap();
            if self.elapsed >= brain.next_input {
                if let Some(p) = self.players.get(&id) {
                    if p.alive {
                        let input = self.bot_command(p, &mut brain);
                        self.input(id, &input);
                    } else {
                        brain.target = 0;
                        brain.path.clear();
                        brain.last_position = None;
                    }
                }
                brain.next_input = self.elapsed + 0.10;
            }
            self.brains.insert(id, brain);
        }
    }
    fn visible(&self, p: &Player, q: &Player) -> bool {
        let dx = q.x - p.x;
        let dz = q.z - p.z;
        let dy = 1.2 - EYE;
        let len = dx.hypot(dz).hypot(dy);
        len > 0.001
            && self
                .ray([p.x, EYE, p.z], [dx / len, dy / len, dz / len], len)
                .0
                .is_none()
    }
    fn bot_command(&self, p: &Player, b: &mut Brain) -> Value {
        let mut chosen = None;
        let mut best = f32::INFINITY;
        for q in self
            .players
            .values()
            .filter(|q| q.team != p.team && q.alive)
        {
            let dist = (q.x - p.x).hypot(q.z - p.z);
            let visible = dist < 42.0 && self.visible(p, q);
            let priority =
                dist - if visible { 21.0 } else { 0.0 } - if q.id == b.target { 3.0 } else { 0.0 };
            if priority < best {
                chosen = Some((q, dist, visible));
                best = priority;
            }
        }
        let Some((q, dist, visible)) = chosen else {
            return json!({"yaw":p.yaw,"reload":true});
        };
        if q.id != b.target {
            b.target = q.id;
            b.next_plan = 0.0;
            b.next_fire = self.elapsed + 0.75 + b.random() * 0.45;
        }
        let reloading = p.ammo == 0 && p.reserve > 0;
        if visible && dist < 35.0 && p.ammo > 0 && p.reload <= 0.0 {
            let yaw = (q.x - p.x).atan2(p.z - q.z);
            let shoot = self.elapsed >= b.next_fire && q.shield <= 0.;
            // The first shot takes a moment to line up and aim drifts with range.
            let scatter = if shoot {
                (b.random() - 0.5) * 0.054
            } else {
                0.0
            };
            if shoot {
                b.next_fire = self.elapsed + 0.85 + b.random() * 0.9;
            }
            let pitch = (1.24 - EYE).atan2(dist)
                + if shoot {
                    (b.random() - 0.5) * 0.018
                } else {
                    0.0
                };
            b.path.clear();
            b.next_plan = 0.;
            return json!({"yaw":yaw+scatter,"pitch":pitch,"shoot":shoot,"aim":true});
        }
        // With an empty reserve, head for the nearest stocked ammunition crate.
        let depot = if p.ammo == 0 && p.reserve == 0 {
            self.props
                .iter()
                .filter(|o| !o.dead && o.resource.as_deref() == Some("ammo") && o.stock > 0)
                .min_by(|a, b| {
                    (a.x - p.x)
                        .hypot(a.z - p.z)
                        .total_cmp(&(b.x - p.x).hypot(b.z - p.z))
                })
        } else {
            None
        };
        let (tx, tz) = depot.map(|o| (o.x, o.z)).unwrap_or((q.x, q.z));
        if let Some(d) = depot {
            if d.footprint(p.x, p.z) < 2.1 * 2.1 {
                return json!({"yaw":(tx-p.x).atan2(p.z-tz),"interact":true});
            }
        }
        let stuck = b
            .last_position
            .is_some_and(|(x, z)| (x - p.x).hypot(z - p.z) < 0.012);
        b.last_position = Some((p.x, p.z));
        if self.elapsed >= b.next_plan || b.path.is_empty() || stuck {
            b.path = self.nav.as_ref().unwrap().route((p.x, p.z), (tx, tz));
            b.next_plan = self.elapsed + 1.5 + b.random() * 0.7;
        }
        while b
            .path
            .front()
            .is_some_and(|(x, z)| (x - p.x).hypot(z - p.z) < 0.27)
        {
            b.path.pop_front();
        }
        let (x, z) = b.path.front().copied().unwrap_or((tx, tz));
        json!({"yaw":(x-p.x).atan2(p.z-z),"pitch":0.0,"forward":1.0,"sprint":dist>23.0,"reload":reloading})
    }
}

#[cfg(test)]
mod practice_tests {
    use super::*;
    #[test]
    fn ordinary_servers_stay_human_only_training_squads_reach_and_fight_on_the_shipped_map() {
        let mut m = Match::new();
        m.add_player(1, "Volunteer");
        for _ in 0..20 {
            m.step(0.05);
        }
        assert_eq!(m.players.len(), 1);
        assert!(m.brains.is_empty());
        m.enable_training();
        m.enable_training();
        assert_eq!(m.players.len(), 4);
        assert_eq!(m.players.values().filter(|p| p.bot).count(), 3);
        assert_eq!(m.players.values().filter(|p| p.team == "blue").count(), 2);
        let start = (m.players[&1_000_001].x, m.players[&1_000_001].z);
        let mut distance = 0.0f32;
        let mut fired = false;
        let mut respawned = false;
        for _ in 0..2400 {
            m.step(0.05);
            let soldier = &m.players[&1_000_001];
            distance = distance.max((start.0 - soldier.x).hypot(start.1 - soldier.z));
            if m.events.iter().any(|e| e["type"] == "shot") {
                fired = true;
            }
            if m.events.iter().any(|e| e["type"] == "respawn") {
                respawned = true;
            }
            if distance > 8.0 && m.blue + m.red > 1 && respawned {
                break;
            }
        }
        assert!(
            distance > 8.0 && fired && respawned && m.blue + m.red > 0,
            "bot travel {distance}, shots {fired}, respawn {respawned}, score {}:{}",
            m.blue,
            m.red
        );
    }
}
