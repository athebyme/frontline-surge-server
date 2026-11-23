//! Server-owned motor pool, refinery orders, vehicle kits and freight.
use crate::sim::inventory::{Material, STACK_LIMIT};
use crate::sim::{Bounds, Player, Prop};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{PI, TAU};

const MAX_TANK_SHELLS: u32 = 12;

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct VehicleSpawn {
    pub(crate) kind: String,
    pub(crate) team: String,
    pub(crate) x: f32,
    pub(crate) z: f32,
    #[serde(default)]
    pub(crate) yaw: f32,
}

#[derive(Clone, Copy)]
struct Spec {
    speed: f32,
    accel: f32,
    turn: f32,
    hp: f32,
    fuel: f32,
    burn: f32,
    w: f32,
    d: f32,
    h: f32,
    respawn: f32,
}
fn spec(kind: &str) -> Spec {
    let (speed, accel, turn, hp, fuel, burn, w, d, h, respawn) = match kind {
        "scout_buggy" => (28., 10.1, 1.85, 95., 25., 0.011, 1.75, 3.3, 1.45, 43.),
        "apc" => (12.7, 4.1, 0.98, 390., 70., 0.033, 2.8, 5.45, 2.35, 90.),
        "patrol4x4" => (22., 7.8, 1.5, 145., 34., 0.013, 1.65, 3.5, 1.6, 55.),
        "cargo6x6" => (15., 4.2, 0.94, 270., 58., 0.024, 2.36, 6.0, 2.7, 72.),
        _ => (9.2, 3.4, 0.77, 610., 82., 0.044, 2.9, 5.9, 2.85, 105.),
    };
    Spec {
        speed,
        accel,
        turn,
        hp,
        fuel,
        burn,
        w,
        d,
        h,
        respawn,
    }
}
fn hold_capacity(kind: &str, utility: u32) -> u32 {
    match kind {
        "cargo6x6" => 96 + 32 * utility,
        "apc" => 24 + 12 * utility,
        _ => 0,
    }
}
fn bag(p: &Player) -> u32 {
    p.wood + p.metal + p.oil + p.fuel + p.powder
}
fn val(input: &Value, name: &str) -> f32 {
    input[name].as_f64().unwrap_or(0.0) as f32
}
fn pressed(input: &Value, name: &str) -> bool {
    input[name].as_bool() == Some(true)
}
fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}
fn distance(x: f32, z: f32, cx: f32, cz: f32) -> f32 {
    (x - cx).hypot(z - cz)
}

#[derive(Clone, Default, Serialize)]
struct Cargo {
    wood: u32,
    metal: u32,
    oil: u32,
    fuel: u32,
    powder: u32,
}
impl Cargo {
    fn total(&self) -> u32 {
        self.wood + self.metal + self.oil + self.fuel + self.powder
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Vehicle {
    id: u32,
    kind: String,
    team: String,
    x: f32,
    z: f32,
    yaw: f32,
    turret_yaw: f32,
    speed: f32,
    #[serde(skip)]
    course: f32,
    #[serde(skip)]
    last_driver: Option<u32>,
    #[serde(skip)]
    advanced_this_tick: bool,
    hp: f32,
    max_hp: f32,
    fuel: f32,
    max_fuel: f32,
    driver: Option<u32>,
    destroyed: bool,
    respawn_in: f32,
    cargo: Cargo,
    cargo_capacity: u32,
    upgrade: Option<String>,
    upgrade_level: u32,
    shells: u32,
    mg_ammo: u32,
    mg_capacity: u32,
    travel: f32,
    steering: f32,
    #[serde(skip)]
    mg_delay: f32,
    #[serde(skip)]
    impact_delay: f32,
    #[serde(skip)]
    spawn: VehicleSpawn,
    #[serde(skip)]
    cooldown: f32,
    #[serde(skip)]
    salvage: u32,
}
impl Vehicle {
    fn new(id: u32, spawn: &VehicleSpawn) -> Self {
        let b = spec(&spawn.kind);
        Self {
            id,
            kind: spawn.kind.clone(),
            team: spawn.team.clone(),
            x: spawn.x,
            z: spawn.z,
            yaw: spawn.yaw,
            turret_yaw: spawn.yaw,
            speed: 0.,
            course: spawn.yaw,
            last_driver: None,
            advanced_this_tick: false,
            hp: b.hp,
            max_hp: b.hp,
            fuel: b.fuel * 0.52,
            max_fuel: b.fuel,
            driver: None,
            destroyed: false,
            respawn_in: 0.,
            cargo: Cargo::default(),
            cargo_capacity: hold_capacity(&spawn.kind, 0),
            upgrade: None,
            upgrade_level: 0,
            mg_ammo: match spawn.kind.as_str() {
                "mbt" => 80,
                "apc" => 100,
                _ => 0,
            },
            mg_capacity: match spawn.kind.as_str() {
                "mbt" => 80,
                "apc" => 100,
                _ => 0,
            },
            mg_delay: 0.,
            impact_delay: 0.,
            travel: 0.,
            steering: 0.,
            shells: if spawn.kind == "mbt" { 8 } else { 0 },
            spawn: spawn.clone(),
            cooldown: 0.,
            salvage: 0,
        }
    }
    fn kit(&self, name: &str) -> f32 {
        if self.upgrade.as_deref() == Some(name) {
            self.upgrade_level as f32
        } else {
            0.
        }
    }
    fn shape(&self) -> Prop {
        let b = spec(&self.kind);
        let mut p = Prop {
            x: self.x,
            z: self.z,
            yaw: self.yaw,
            w: b.w,
            d: b.d,
            h: if self.destroyed { b.h * 0.58 } else { b.h },
            solid: true,
            ..Default::default()
        };
        p.prepare_geometry();
        p
    }
}
#[derive(Clone, Default)]
struct Run {
    remaining: f32,
    ready: u32,
    pending: u32,
}
#[derive(Default)]
pub(crate) struct Logistics {
    vehicles: Vec<Vehicle>,
    runs: BTreeMap<usize, Run>,
    roster: Vec<(u32, f32, f32)>,
    selected: HashMap<u32, bool>,
    eject: HashMap<u32, (f32, f32, f32)>,
    clock: f32,
    used: HashMap<u32, f32>,
    factories: Vec<(usize, String, String, f32, f32, f32, f32, f32)>,
}
/// The same server-owned mount supplies both the HUD sight and each MG shot.
pub(crate) struct MachinegunRay {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub vehicle_kind: &'static str,
}
impl Logistics {
    pub(crate) fn machinegun_ray(&self, driver: u32, pitch: f32) -> Option<MachinegunRay> {
        let v = &self.vehicles[self.driver_vehicle(driver)?];
        let vehicle_kind = match v.kind.as_str() {
            "mbt" => "mbt",
            "apc" => "apc",
            _ => return None,
        };
        let (sp, cp) = pitch.clamp(-0.7, 0.7).sin_cos();
        let (sy, cy) = v.turret_yaw.sin_cos();
        Some(MachinegunRay {
            origin: [v.x, 2.08, v.z],
            direction: [sy * cp, sp, -cy * cp],
            vehicle_kind,
        })
    }
    pub(crate) fn hull_kind(&self, index: usize) -> Option<&str> {
        self.vehicles.get(index).map(|v| v.kind.as_str())
    }
    /// A facility's dynamic slot can be rebuilt before the following tick.
    /// Its replacement never inherits a pending batch or unclaimed product.
    pub(crate) fn forget_site(&mut self, index: usize) {
        self.runs.remove(&index);
        self.factories.retain(|(i, ..)| *i != index);
    }

    pub(crate) fn reset(&mut self, spawns: &[VehicleSpawn]) {
        *self = Self::default();
        self.vehicles = spawns
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                matches!(
                    s.kind.as_str(),
                    "scout_buggy" | "apc" | "patrol4x4" | "cargo6x6" | "mbt"
                )
            })
            .map(|(i, s)| Vehicle::new((i + 1) as u32, s))
            .collect();
    }
    pub(crate) fn driver_vehicle(&self, id: u32) -> Option<usize> {
        self.vehicles
            .iter()
            .position(|v| v.driver == Some(id) && !v.destroyed)
    }
    pub(crate) fn remove_player(&mut self, id: u32) {
        if let Some(i) = self.driver_vehicle(id) {
            self.vehicles[i].driver = None;
        }
        self.selected.remove(&id);
        self.eject.remove(&id);
        self.used.remove(&id);
    }
    pub(crate) fn blocked(&self, x: f32, z: f32, r: f32, ignore_driver: Option<u32>) -> bool {
        self.vehicles.iter().any(|v| {
            !(ignore_driver.is_some() && v.driver == ignore_driver)
                && (v.x - x).abs() < 6. + r
                && (v.z - z).abs() < 6. + r
                && v.shape().intersects(x, z, r)
        })
    }
    pub(crate) fn ray(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        limit: f32,
        ignore_driver: Option<u32>,
    ) -> Option<(usize, f32)> {
        self.ray_except(o, d, limit, ignore_driver, None)
    }
    pub(crate) fn ray_except(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        limit: f32,
        ignore_driver: Option<u32>,
        ignore_vehicle: Option<u32>,
    ) -> Option<(usize, f32)> {
        self.vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| {
                !(ignore_driver.is_some() && v.driver == ignore_driver)
                    && Some(v.id) != ignore_vehicle
            })
            .filter_map(|(i, v)| v.shape().ray(o, d, limit).map(|t| (i, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
    pub(crate) fn blast_targets(&self, x: f32, z: f32, r: f32) -> Vec<(usize, f32, f32, f32)> {
        self.vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.destroyed)
            .filter_map(|(i, v)| {
                let hull = v.shape();
                if hull.footprint(x, z) > r * r {
                    return None;
                }
                let (a, b) = (x - v.x, z - v.z);
                let dx = (a * hull.cs + b * hull.sn).clamp(-hull.w / 2., hull.w / 2.);
                let dz = (b * hull.cs - a * hull.sn).clamp(-hull.d / 2., hull.d / 2.);
                Some((
                    i,
                    v.x + dx * hull.cs - dz * hull.sn,
                    hull.h * 0.5,
                    v.z + dx * hull.sn + dz * hull.cs,
                ))
            })
            .collect()
    }
    pub(crate) fn hit(&mut self, i: usize, amount: f32, actor: u32, team: &str) -> Vec<Value> {
        let Some(v) = self.vehicles.get_mut(i) else {
            return vec![];
        };
        if v.destroyed || v.team == team {
            return vec![];
        }
        v.hp = (v.hp - amount.max(0.)).max(0.);
        if v.hp > 0. {
            return vec![];
        }
        if let Some(id) = v.driver.take() {
            self.eject.insert(id, (v.x, v.z, v.yaw));
        }
        v.speed = 0.;
        v.destroyed = true;
        v.respawn_in = spec(&v.kind).respawn;
        v.salvage = 6;
        v.fuel = 0.;
        v.cargo = Cargo::default();
        vec![
            json!({"type":"destroy","actor":actor,"kind":v.kind,"vehicleId":v.id,"x":v.x,"z":v.z,"ex":v.x,"ez":v.z}),
        ]
    }
    pub(crate) fn tick(&mut self, dt: f32, scene: &[Prop], players: &BTreeMap<u32, Player>) {
        self.clock += dt;
        self.roster.clear();
        self.roster.extend(
            players
                .values()
                .filter(|p| p.alive)
                .map(|p| (p.id, p.x, p.z)),
        );
        for i in 0..self.vehicles.len() {
            let v = &mut self.vehicles[i];
            v.advanced_this_tick = false;
            v.cooldown = (v.cooldown - dt).max(0.);
            v.mg_delay = (v.mg_delay - dt).max(0.);
            v.impact_delay = (v.impact_delay - dt).max(0.);
            if let Some(id) = v.driver {
                if !players.get(&id).is_some_and(|p| p.alive) {
                    v.driver = None;
                }
            }
            if !v.destroyed {
                continue;
            }
            v.respawn_in = (v.respawn_in - dt).max(0.);
            if v.respawn_in > 0. {
                continue;
            }
            let pad = Vehicle::new(v.id, &v.spawn);
            let hull = pad.shape();
            if self
                .roster
                .iter()
                .any(|(_, x, z)| hull.intersects(*x, *z, 0.4))
                || scene
                    .iter()
                    .any(|p| p.built && !p.dead && p.solid && p.touches(&hull))
                || self
                    .vehicles
                    .iter()
                    .enumerate()
                    .any(|(j, q)| i != j && hull.touches(&q.shape()))
            {
                continue;
            }
            self.vehicles[i] = pad;
        }
        self.factories.clear();
        for (i, prop) in scene.iter().enumerate() {
            if !prop.dead && matches!(prop.kind.as_str(), "refinery" | "workshop") {
                self.factories.push((
                    i,
                    prop.kind.clone(),
                    prop.team.clone().unwrap_or_default(),
                    prop.x,
                    prop.z,
                    prop.w,
                    prop.d,
                    prop.yaw,
                ));
            }
        }
        self.runs
            .retain(|i, _| scene.get(*i).is_some_and(|p| !p.dead));
        for run in self.runs.values_mut() {
            if run.pending > 0 {
                run.remaining = (run.remaining - dt).max(0.);
                if run.remaining <= 0.0001 {
                    run.remaining = 0.;
                    run.ready += run.pending;
                    run.pending = 0;
                }
            }
        }
    }
    fn vehicle_near(&self, p: &Player) -> Option<usize> {
        self.vehicle_near_matching(p, |_| true)
    }
    fn vehicle_near_matching(
        &self,
        p: &Player,
        eligible: impl Fn(&Vehicle) -> bool,
    ) -> Option<usize> {
        if let Some(i) = self
            .driver_vehicle(p.id)
            .filter(|&i| eligible(&self.vehicles[i]))
        {
            return Some(i);
        }
        self.vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| (v.x - p.x).abs() < 7. && (v.z - p.z).abs() < 7.)
            .filter(|(_, v)| eligible(v))
            .map(|(i, v)| (i, v.shape().footprint(p.x, p.z)))
            .filter(|(_, d)| *d <= 2.65f32.powi(2))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|e| e.0)
    }
    fn facility(p: &Player, scene: &[Prop], kind: &str, reach: f32) -> Option<usize> {
        scene
            .iter()
            .enumerate()
            .filter(|(_, o)| !o.dead && o.kind == kind && o.team.as_ref() == Some(&p.team))
            .map(|(i, o)| (i, o.footprint(p.x, p.z)))
            .filter(|(_, d)| *d <= reach * reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|e| e.0)
    }
    pub(crate) fn station_action(&self, p: &Player, scene: &[Prop]) -> bool {
        self.driver_vehicle(p.id).is_none()
            && (Self::facility(p, scene, "refinery", 2.6).is_some()
                || self
                    .vehicle_near(p)
                    .is_some_and(|i| self.vehicles[i].destroyed))
    }
    pub(crate) fn hint(&self, p: &Player, scene: &[Prop]) -> Option<String> {
        if p.build_mode {
            return None;
        }
        if let Some(i) = self.driver_vehicle(p.id) {
            let v = &self.vehicles[i];
            let mut hint = "[F] Выйти".to_string();
            if v.kind == "mbt" {
                hint.push_str(" · [ЛКМ] Пушка · [ПКМ] Пулемёт");
            } else if v.kind == "apc" {
                hint.push_str(" · [ЛКМ] Пулемёт");
            }
            if v.cargo_capacity > 0 {
                hint.push_str("  ·  [T] Груз");
            }
            if Self::facility(p, scene, "workshop", 8.).is_some() {
                hint.push_str("  ·  [V/U] Комплект  ·  [G] Сервис");
            } else if v.fuel < v.max_fuel - 1. && p.fuel > 0 {
                hint.push_str("  ·  [G] Заправить");
            }
            return Some(hint);
        }
        if let Some(i) = self.vehicle_near(p) {
            let v = &self.vehicles[i];
            if v.destroyed && v.salvage > 0 {
                return Some("[E] Разобрать остов".into());
            }
            if !v.destroyed && v.team == p.team && v.driver.is_none() {
                return Some(
                    if v.cargo_capacity > 0 {
                        "[F] За руль  ·  [T] Переложить груз  ·  [G] Сервис"
                    } else {
                        "[F] За руль  ·  [G] Сервис"
                    }
                    .into(),
                );
            }
        }
        if let Some(i) = Self::facility(p, scene, "refinery", 2.6) {
            if let Some(r) = self.runs.get(&i) {
                if r.ready > 0 {
                    return Some(format!("[E] Забрать топливо · {} ед.", r.ready));
                }
                if r.pending > 0 {
                    return Some(format!("Переработка · {} с", r.remaining.ceil() as u32));
                }
            }
            return Some("[E] Переработать 4 нефти в 6 топлива · 12 с".into());
        }
        if Self::facility(p, scene, "workshop", 3.).is_some() {
            return Some("[C] Снаряжение · 2 пороха + 1 металл".into());
        }
        None
    }

    fn inside(x: f32, z: f32, r: f32, b: &Bounds) -> bool {
        x - r >= b.min_x && x + r <= b.max_x && z - r >= b.min_z && z + r <= b.max_z
    }
    fn safe_exit(
        &self,
        x: f32,
        z: f32,
        yaw: f32,
        id: u32,
        scene: &[Prop],
        bounds: &Bounds,
    ) -> Option<(f32, f32)> {
        for reach in [2.5, 3.7, 5., 7., 10.] {
            for angle in [
                PI / 2.,
                -PI / 2.,
                PI,
                0.,
                PI * 0.75,
                -PI * 0.75,
                PI * 0.25,
                -PI * 0.25,
            ] {
                let a = yaw + angle;
                let (xx, zz) = (x + a.sin() * reach, z - a.cos() * reach);
                if Self::inside(xx, zz, 0.37, bounds)
                    && !scene.iter().any(|o| o.intersects(xx, zz, 0.37))
                    && !self.blocked(xx, zz, 0.37, None)
                    && !self
                        .roster
                        .iter()
                        .any(|(j, px, pz)| *j != id && distance(xx, zz, *px, *pz) < 0.72)
                {
                    return Some((xx, zz));
                }
            }
        }
        None
    }
    /// Candidate hull volume is tested against surviving structure cells.
    /// On contact, expose the actual point on the blocking surface for damage.
    fn obstruction(
        &self,
        i: usize,
        x: f32,
        z: f32,
        yaw: f32,
        scene: &[Prop],
        bounds: &Bounds,
    ) -> Option<(Option<usize>, [f32; 3])> {
        let cfg = spec(&self.vehicles[i].kind);
        let r = cfg.w * 0.5;
        let half = (cfg.d - cfg.w) * 0.5;
        for along in [half, -half, 0.] {
            let (cx, cz) = (x + yaw.sin() * along, z - yaw.cos() * along);
            if !Self::inside(cx, cz, r, bounds) {
                return Some((None, [cx, 1., cz]));
            }
            for (index, p) in scene.iter().enumerate() {
                if let Some(point) = p.contact(cx, cz, r, cfg.h) {
                    return Some((Some(index), point));
                }
            }
            if self.vehicles.iter().enumerate().any(|(j, q)| {
                i != j && distance(cx, cz, q.x, q.z) < 7.0 && q.shape().intersects(cx, cz, r)
            }) {
                return Some((None, [cx, 1., cz]));
            }
        }
        None
    }
    fn fit(&self, i: usize, x: f32, z: f32, yaw: f32, scene: &[Prop], b: &Bounds) -> bool {
        self.obstruction(i, x, z, yaw, scene, b).is_none()
    }
    fn advance_vehicle(
        &mut self,
        i: usize,
        acceleration: f32,
        turn: f32,
        dt: f32,
        scene: &[Prop],
        b: &Bounds,
    ) -> Vec<Value> {
        let v = &self.vehicles[i];
        let cfg = spec(&v.kind);
        let mut events = Vec::new();
        let load = v.cargo.total() as f32 / (v.cargo_capacity.max(1) as f32);
        let factor = (1. + 0.14 * v.kit("mobility"))
            * (1. - 0.14 * load)
            * (if v.kind == "mbt" {
                1. - 0.075 * v.kit("utility")
            } else {
                1.
            });
        let top = cfg.speed * factor;
        let target = if v.fuel <= 0. {
            0.
        } else {
            top * acceleration * (if acceleration < 0. { 0.4 } else { 1. })
        };
        let rate = if target == 0. {
            cfg.accel * 0.48
        } else if target.signum() != v.speed.signum() && v.speed != 0. {
            cfg.accel * 1.8
        } else {
            cfg.accel
        };
        let mut speed = v.speed + (target - v.speed).clamp(-rate * dt, rate * dt);
        let steering = if v.kind == "mbt" && v.fuel > 0. {
            0.85
        } else {
            (speed.abs() / 3.).min(1.) * speed.signum()
        };
        let mut yaw = wrap(v.yaw + turn * steering * cfg.turn * dt);
        if !self.fit(i, v.x, v.z, yaw, scene, b) {
            yaw = v.yaw;
        }
        let grip = if v.kind == "mbt" { 10.0_f32 } else { 5.0 };
        let course = if speed.abs() < 1.0 {
            yaw
        } else {
            wrap(v.course + wrap(yaw - v.course) * (1. - (-grip * dt).exp()))
        };
        let (mut x, mut z) = (v.x + course.sin() * speed * dt, v.z - course.cos() * speed * dt);
        let mut impact = false;
        if let Some((index, point)) = self.obstruction(i, x, z, yaw, scene, b) {
            let approach = speed.abs();
            if approach >= 3.4 && v.impact_delay <= 0.0 {
                let weight = match v.kind.as_str() {
                    "scout_buggy" => 3.,
                    "patrol4x4" => 6.,
                    "cargo6x6" => 12.,
                    "apc" => 18.,
                    _ => 25.,
                };
                let power = (approach * weight).min(235.);
                let material = index.map(|n| scene[n].impact_material()).unwrap_or("metal");
                events.push(json!({"type":"vehicleImpact","actor":v.last_driver,"vehicleId":v.id,"kind":v.kind,"index":index,"material":material,
                    "strength":(power/180.).clamp(0.12,1.0),"power":power,"x":point[0],"y":point[1],"z":point[2],"ex":point[0],"ey":point[1],"ez":point[2]}));
                impact = true;
            }
            x = v.x;
            z = v.z;
            // Re-test the hull next tick after the match applies impact damage.
            // Only a newly opened passage can carry the remaining momentum.
            speed = if impact && index.is_some() { speed * 0.55 } else { 0. };
        }
        let v = &mut self.vehicles[i];
        if impact {
            v.impact_delay = 0.7;
        }
        let moved = distance(v.x, v.z, x, z);
        v.advanced_this_tick = true;
        v.travel += moved * speed.signum();
        v.steering = turn;
        if acceleration != 0. {
            v.fuel = (v.fuel - moved * cfg.burn * (1. + 0.09 * v.kit("mobility"))).max(0.);
        }
        v.x = x;
        v.z = z;
        v.speed = speed;
        v.yaw = yaw;
        v.course = course;
        if moved > 0. && speed.abs() >= 6. {
            let hull = v.shape();
            for &(id, px, pz) in &self.roster {
                if Some(id) != v.last_driver && hull.intersects(px, pz, 0.34) {
                    events.push(json!({"type":"runOver","target":id,"vehicleKind":v.kind}));
                }
            }
        }
        events
    }

    pub(crate) fn coast(
        &mut self,
        dt: f32,
        scene: &[Prop],
        bounds: &Bounds,
    ) -> Vec<(u32, String, Vec<Value>)> {
        let mut impacts = Vec::new();
        for i in 0..self.vehicles.len() {
            let v = &self.vehicles[i];
            if v.destroyed || v.driver.is_some() || v.advanced_this_tick || v.speed == 0. {
                continue;
            }
            let Some(actor) = v.last_driver else { continue };
            let events = self.advance_vehicle(i, 0., 0., dt, scene, bounds);
            if !events.is_empty() {
                impacts.push((actor, self.vehicles[i].team.clone(), events));
            }
        }
        impacts
    }

    fn drive(
        &mut self,
        i: usize,
        p: &mut Player,
        input: &Value,
        dt: f32,
        scene: &[Prop],
        b: &Bounds,
    ) -> Vec<Value> {
        let mut events = self.advance_vehicle(
            i,
            val(input, "forward").clamp(-1., 1.),
            val(input, "strafe").clamp(-1., 1.),
            dt,
            scene,
            b,
        );
        let v = &mut self.vehicles[i];
        let (x, z, yaw, speed) = (v.x, v.z, v.yaw, v.speed);
        let turret_step = 1.28 * dt;
        v.turret_yaw = if matches!(v.kind.as_str(), "mbt" | "apc") {
            wrap(v.turret_yaw + wrap(p.yaw - v.turret_yaw).clamp(-turret_step, turret_step))
        } else {
            yaw
        };
        p.x = x;
        p.z = z;
        p.moving = speed.abs() > 0.3;
        p.sprinting = false;
        p.build_mode = false;
        p.preview = None;
        p.recoil = 0.;
        if v.kind == "mbt" && pressed(input, "shoot") && v.cooldown <= 0. && v.shells > 0 {
            v.shells -= 1;
            v.cooldown = 3.6;
            p.shield = 0.;
            events.push(json!({"type":"tankFire","actor":p.id,"x":x,"z":z,"y":2.25,"yaw":v.turret_yaw,"pitch":p.pitch.clamp(-0.6,0.6),"vehicleId":v.id}));
        }
        if v.mg_capacity > 0
            && (pressed(input, "secondary") || (v.kind == "apc" && pressed(input, "shoot")))
            && v.mg_delay <= 0.0
            && v.mg_ammo > 0
        {
            v.mg_ammo -= 1;
            v.mg_delay = 0.12;
            p.shield = 0.;
            events.push(json!({"type":"mgFire"}));
        }
        events
    }
    fn operation(&mut self, p: &mut Player, scene: &[Prop]) {
        if let Some(i) = Self::facility(p, scene, "refinery", 2.6) {
            let run = self.runs.entry(i).or_default();
            if run.ready > 0 {
                let n = run.ready.min(p.inventory_room("fuel"));
                if n == 0 {
                    p.notice("Рюкзак полон");
                    return;
                }
                run.ready -= n;
                p.fuel += n;
                p.notice(&format!("Получено топлива: {n}"));
            } else if run.pending > 0 {
                p.notice("Переработка ещё идёт");
            } else if p.oil >= 4 {
                p.oil -= 4;
                run.pending = 6;
                run.remaining = 12.;
                p.notice("Нефть в переработке · 12 с");
            } else {
                p.notice("Нужно 4 нефти");
            }
            return;
        }
        if let Some(i) = self.vehicle_near(p) {
            let v = &mut self.vehicles[i];
            if v.destroyed && v.salvage > 0 {
                let n = 2.min(v.salvage).min(p.inventory_room("metal"));
                if n > 0 {
                    v.salvage -= n;
                    p.metal += n;
                    p.notice("С остова снят металл");
                }
            }
        }
    }
    fn service(&mut self, p: &mut Player, scene: &[Prop], i: usize) {
        let v = &mut self.vehicles[i];
        if v.destroyed || v.team != p.team {
            return;
        }
        let amount = p.fuel.min((v.max_fuel - v.fuel).ceil().max(0.) as u32);
        v.fuel = (v.fuel + amount as f32).min(v.max_fuel);
        p.fuel -= amount;
        let repair =
            Self::facility(p, scene, "workshop", 8.).is_some() && p.metal >= 2 && v.hp < v.max_hp;
        if repair {
            p.metal -= 2;
            v.hp = (v.hp + 80.).min(v.max_hp);
        }
        p.notice(if repair && amount > 0 {
            "Машина заправлена и отремонтирована"
        } else if repair {
            "Ремонт +80"
        } else if amount > 0 {
            "Топливо залито"
        } else {
            "Для заправки возьми топливо. Для ремонта нужны мастерская и 2 металла"
        });
    }
    pub(crate) fn debug_vehicle_service(&mut self, p: &mut Player) {
        if !p.alive {
            return;
        }
        let Some(i) = self.vehicle_near_matching(p, |v| !v.destroyed && v.team == p.team) else {
            p.notice("Подойди к союзной машине");
            return;
        };
        let v = &mut self.vehicles[i];
        v.hp = v.max_hp;
        v.fuel = v.max_fuel;
        v.mg_ammo = v.mg_capacity;
        if v.kind == "mbt" {
            v.shells = MAX_TANK_SHELLS;
        }
        p.notice("Машина отремонтирована, топливо и боезапас пополнены");
    }
    fn upgrade(&mut self, p: &mut Player, scene: &[Prop], i: usize) {
        let v = &mut self.vehicles[i];
        if v.destroyed || v.team != p.team {
            return;
        }
        if Self::facility(p, scene, "workshop", 8.).is_none() {
            p.notice("Подведи машину к своей мастерской");
            return;
        }
        let kind = if self.selected.get(&p.id) == Some(&true) {
            "utility"
        } else {
            "mobility"
        };
        let next = if v.upgrade.as_deref() == Some(kind) {
            v.upgrade_level + 1
        } else {
            1
        };
        if next > 2 {
            p.notice("Уже установлен комплект II");
            return;
        }
        if kind == "mobility" && v.cargo.total() > hold_capacity(&v.kind, 0) {
            p.notice("Выгрузи груз перед заменой кузова");
            return;
        }
        let (wood, metal) = (2 * next, 4 * next);
        if p.wood < wood || p.metal < metal {
            p.notice(&format!("Для комплекта: {wood} дерева + {metal} металла"));
            return;
        }
        p.wood -= wood;
        p.metal -= metal;
        v.upgrade = Some(kind.into());
        v.upgrade_level = next;
        let base = spec(&v.kind);
        let utility = v.kit("utility");
        v.max_fuel = base.fuel
            * if matches!(v.kind.as_str(), "patrol4x4" | "scout_buggy") {
                1. + 0.42 * utility
            } else {
                1.
            };
        v.fuel = v.fuel.min(v.max_fuel);
        v.max_hp = base.hp
            * if v.kind == "mbt" {
                1. + 0.22 * utility
            } else {
                1.
            };
        v.hp = v.hp.min(v.max_hp);
        v.cargo_capacity = hold_capacity(&v.kind, utility as u32);
        p.notice(&format!("Комплект установлен · уровень {next}"));
    }
    fn move_material(
        p: &mut Player,
        v: &mut Vehicle,
        kind: Material,
        requested: u32,
        load: bool,
    ) -> u32 {
        let bay_room = v.cargo_capacity.saturating_sub(v.cargo.total());
        let cargo = match kind {
            Material::Wood => &mut v.cargo.wood,
            Material::Metal => &mut v.cargo.metal,
            Material::Oil => &mut v.cargo.oil,
            Material::Fuel => &mut v.cargo.fuel,
            Material::Powder => &mut v.cargo.powder,
        };
        let slot = p.material_mut(kind);
        if load {
            let n = requested.min(*slot).min(bay_room);
            *cargo += n;
            *slot -= n;
            n
        } else {
            let n = requested.min(*cargo).min(STACK_LIMIT.saturating_sub(*slot));
            *slot += n;
            *cargo -= n;
            n
        }
    }
    pub(crate) fn transfer_material(
        &mut self,
        p: &mut Player,
        kind: Material,
        requested: u32,
        load: bool,
    ) -> u32 {
        let driving = self.driver_vehicle(p.id);
        let nearest = self
            .vehicles
            .iter()
            .enumerate()
            .filter(|(i, v)| {
                v.cargo_capacity > 0
                    && !v.destroyed
                    && v.team == p.team
                    && driving.is_none_or(|index| index == *i)
            })
            .map(|(i, v)| (i, v.shape().footprint(p.x, p.z)))
            .filter(|(_, distance)| *distance <= 2.65 * 2.65)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((i, _)) = nearest else { return 0 };
        Self::move_material(p, &mut self.vehicles[i], kind, requested, load)
    }
    fn transfer(p: &mut Player, v: &mut Vehicle) {
        if v.cargo_capacity == 0 || v.destroyed || v.team != p.team {
            return;
        }
        let loading = bag(p) > 0;
        let mut moved = 0;
        for kind in Material::ALL {
            moved += Self::move_material(p, v, kind, STACK_LIMIT, loading);
        }
        p.notice(&format!(
            "{} · {} ед.",
            if loading {
                "Погружено"
            } else {
                "Выгружено в рюкзак"
            },
            moved
        ));
    }
    fn ammo(p: &mut Player, vehicle: Option<&mut Vehicle>) {
        if p.powder < 2 || p.metal < 1 {
            p.notice("Нужно 2 пороха и 1 металл");
            return;
        }
        if let Some(v) = vehicle.filter(|v| v.mg_capacity > 0 && !v.destroyed) {
            let shells_full = v.kind != "mbt" || v.shells >= MAX_TANK_SHELLS;
            if shells_full && v.mg_ammo >= v.mg_capacity {
                p.notice("Боекомплект полный");
                return;
            }
            if v.kind == "mbt" {
                v.shells = (v.shells + 3).min(MAX_TANK_SHELLS);
            }
            v.mg_ammo = (v.mg_ammo + 28).min(v.mg_capacity);
            p.notice("Боекомплект машины пополнен");
        } else {
            p.reserve = p.reserve.saturating_add(24);
            p.notice("+24 патрона в запас");
        }
        p.powder -= 2;
        p.metal -= 1;
    }
    pub(crate) fn update(
        &mut self,
        p: &mut Player,
        input: &Value,
        dt: f32,
        scene: &[Prop],
        bounds: &Bounds,
    ) -> Vec<Value> {
        if let Some((x, z, y)) = self.eject.remove(&p.id) {
            if let Some((a, b)) = self.safe_exit(x, z, y, p.id, scene, bounds) {
                p.x = a;
                p.z = b;
            }
            p.notice("Машина потеряна");
        }
        let was = self.driver_vehicle(p.id);
        let actions = [
            "vehicle",
            "service",
            "upgrade",
            "ammoCraft",
            "interact",
            "cargo",
            "upgradeSelect",
        ];
        if actions.iter().any(|k| pressed(input, k))
            && self.clock >= *self.used.get(&p.id).unwrap_or(&0.0)
        {
            self.used.insert(p.id, self.clock + 0.12);
            if pressed(input, "vehicle") {
                if let Some(i) = was {
                    let v = &self.vehicles[i];
                    if let Some((x, z)) = self.safe_exit(v.x, v.z, v.yaw, p.id, scene, bounds) {
                        self.vehicles[i].driver = None;
                        p.x = x;
                        p.z = z;
                        p.moving = false;
                        p.notice("Пешком");
                    } else {
                        p.notice("Выход перекрыт");
                    }
                } else if let Some(i) = self.vehicle_near(p) {
                    let v = &mut self.vehicles[i];
                    if !v.destroyed && v.driver.is_none() && v.team == p.team {
                        v.driver = Some(p.id);
                        v.last_driver = Some(p.id);
                        p.x = v.x;
                        p.z = v.z;
                        p.build_mode = false;
                        p.preview = None;
                        p.notice("За рулём");
                    } else {
                        p.notice("Машина недоступна");
                    }
                }
            } else if pressed(input, "upgradeSelect") {
                let choice = self.selected.entry(p.id).or_default();
                *choice = !*choice;
                p.notice(if *choice {
                    "Комплект: специализация"
                } else {
                    "Комплект: ходовая"
                });
            } else if pressed(input, "interact") && was.is_none() && !p.build_mode {
                self.operation(p, scene);
            } else if pressed(input, "ammoCraft")
                && Self::facility(p, scene, "workshop", if was.is_some() { 8. } else { 3. })
                    .is_some()
            {
                Self::ammo(p, was.map(|i| &mut self.vehicles[i]));
            } else if let Some(i) = self.vehicle_near(p) {
                if pressed(input, "service") {
                    self.service(p, scene, i);
                } else if pressed(input, "upgrade") {
                    self.upgrade(p, scene, i);
                } else if pressed(input, "cargo") {
                    Self::transfer(p, &mut self.vehicles[i]);
                }
            }
        }
        if let Some(i) = self.driver_vehicle(p.id) {
            return self.drive(i, p, input, dt, scene, bounds);
        }
        vec![]
    }
    pub(crate) fn write_snapshot(&self, state: &mut Value) {
        state["vehicles"] = json!(self.vehicles);
        state["factories"]=Value::Array(self.factories.iter().map(|(i,k,t,x,z,w,d,yaw)|{
            let r=self.runs.get(i);
            json!({"kind":k,"team":t,"x":x,"z":z,"w":w,"d":d,"yaw":yaw,"remaining":r.map_or(0.,|v|v.remaining),"pending":r.map_or(0,|v|v.pending),"ready":r.map_or(0,|v|v.ready)})
        }).collect());
        if let Some(players) = state["players"].as_array_mut() {
            for p in players {
                let id = p["id"].as_u64().unwrap_or_default() as u32;
                p["vehicleId"] = self
                    .driver_vehicle(id)
                    .map(|i| json!(self.vehicles[i].id))
                    .unwrap_or(Value::Null);
                p["upgradeChoice"] = json!(if self.selected.get(&id) == Some(&true) {
                    "utility"
                } else {
                    "mobility"
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "logistics_tests.rs"]
mod tests;
