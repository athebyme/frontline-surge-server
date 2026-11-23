//! Server-owned observation drone with one arcade pulse per flight. Its operator remains where they deployed.
use crate::sim::{Bounds, Player, Prop};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const BATTERY: f32 = 60.;
const LINK: f32 = 65.;
const LAUNCH: f32 = 0.92;
const RELEASE: f32 = 0.32;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Drone {
    id: u32,
    owner: u32,
    team: String,
    x: f32,
    y: f32,
    z: f32,
    yaw: f32,
    battery: f32,
    charges: u8,
    speed: f32,
    v_speed: f32,
    roll: f32,
    signal: f32,
    pitch: f32,
    piloted: bool,
    launch_remaining: f32,
    #[serde(skip)]
    launch_from: [f32; 3],
    #[serde(skip)]
    launch_to: [f32; 3],
    #[serde(skip)]
    velocity: [f32; 3],
    #[serde(skip)]
    ping_delay: f32,
}
#[derive(Serialize)]
struct Marker {
    owner: u32,
    team: String,
    x: f32,
    z: f32,
    remaining: f32,
}
#[derive(Default)]
pub(crate) struct DroneFleet {
    active: BTreeMap<u32, Drone>,
    next_id: u32,
    cooldowns: BTreeMap<u32, f32>,
    markers: BTreeMap<u32, Marker>,
}
fn ray_scene(scene: &[Prop], o: [f32; 3], v: [f32; 3], range: f32) -> f32 {
    scene.iter().fold(range, |limit, p| {
        p.ray(o, v, limit).unwrap_or(limit).min(limit)
    })
}
impl DroneFleet {
    pub(crate) fn aircraft(&self, owner: u32) -> Option<(u32, [f32; 3])> {
        self.active.get(&owner).map(|d| (d.id, [d.x, d.y, d.z]))
    }
    fn recall(&mut self, id: u32, delay: f32) {
        self.active.remove(&id);
        self.cooldowns.insert(id, delay);
    }
    pub(crate) fn remove(&mut self, id: u32) {
        self.active.remove(&id);
        self.cooldowns.remove(&id);
        self.markers.remove(&id);
    }
    pub(crate) fn control(
        &mut self,
        p: &mut Player,
        input: &Value,
        dt: f32,
        bounds: &Bounds,
        scene: &[Prop],
    ) -> bool {
        let id = p.id;
        let key = |k: &str| input[k].as_bool() == Some(true);
        if key("droneRecall") {
            if self.active.contains_key(&id) {
                self.recall(id, 18.);
                p.notice("Дрон отозван");
            } else {
                p.notice("В воздухе нет дрона");
            }
            return true;
        }
        if key("drone") {
            if let Some(v) = self.active.get_mut(&id) {
                if key("droneLink") {
                    if v.launch_remaining > 0. {
                        return true;
                    }
                    if v.piloted {
                        v.piloted = false;
                        v.velocity = [0.; 3];
                        v.speed = 0.;
                        v.v_speed = 0.;
                        v.roll = 0.;
                        p.notice("Дрон завис · J вернуться к камере");
                    } else if (v.x - p.x).hypot(v.z - p.z) <= LINK {
                        v.piloted = true;
                        p.yaw = v.yaw;
                        p.pitch = v.pitch;
                        p.notice("Камера дрона подключена");
                    } else {
                        p.notice("Дрон вне связи · подойди ближе");
                    }
                    return true;
                }
                self.recall(id, 18.);
                p.notice("Дрон отозван");
                return true;
            }
            if self.cooldowns.get(&id).copied().unwrap_or(0.) > 0. {
                p.notice("Дрон на перезарядке");
                return true;
            }
            let clearance = ray_scene(scene, [p.x, 1.72, p.z], [0., 1., 0.], 5.);
            if clearance < 2.0 {
                p.notice("Для запуска выйди под открытое небо");
                return true;
            }
            let modern = key("droneLink");
            let (sin, cos) = p.yaw.sin_cos();
            let hand = [
                p.x + 0.24 * cos + 0.38 * sin,
                1.32,
                p.z + 0.24 * sin - 0.38 * cos,
            ];
            let eye = [p.x, 1.68, p.z];
            // Check both the reach of the hand and the short visible toss. The
            // first airborne position must be reachable through the same map.
            let reach = [hand[0] - eye[0], hand[1] - eye[1], hand[2] - eye[2]];
            let reach_len = reach[0].hypot(reach[1]).hypot(reach[2]);
            let throw = [
                sin * 1.15,
                (1.72 + clearance - 0.3).min(4.1) - hand[1],
                -cos * 1.15,
            ];
            let length = throw[0].hypot(throw[1]).hypot(throw[2]);
            let direction = throw.map(|d| d / length);
            let allowed = (ray_scene(scene, hand, direction, length + 0.3) - 0.3).clamp(0., length);
            if modern
                && (ray_scene(scene, eye, reach.map(|d| d / reach_len), reach_len + 0.18)
                    < reach_len + 0.17
                    || allowed < 1.0)
            {
                p.notice("Перед рукой препятствие · отойди от укрытия");
                return true;
            }
            let to = std::array::from_fn(|i| hand[i] + direction[i] * allowed);
            let from = if modern {
                hand
            } else {
                [p.x, (1.72 + clearance - 0.3).min(5.5), p.z]
            };
            self.next_id = self.next_id.wrapping_add(1).max(1);
            self.active.insert(
                id,
                Drone {
                    id: self.next_id,
                    owner: id,
                    team: p.team.clone(),
                    x: from[0],
                    y: from[1],
                    z: from[2],
                    yaw: p.yaw,
                    battery: BATTERY,
                    charges: 1,
                    speed: 0.,
                    v_speed: 0.,
                    roll: 0.,
                    signal: 1.,
                    pitch: if modern { -0.34 } else { p.pitch },
                    velocity: [0.; 3],
                    ping_delay: 0.,
                    piloted: true,
                    launch_remaining: if modern { LAUNCH } else { 0. },
                    launch_from: from,
                    launch_to: to,
                },
            );
            p.build_mode = false;
            p.preview = None;
        }
        let Some(v) = self.active.get_mut(&id) else {
            return false;
        };
        if v.launch_remaining > 0. {
            v.launch_remaining = (v.launch_remaining - dt).max(0.);
            let phase =
                ((LAUNCH - v.launch_remaining - RELEASE) / (LAUNCH - RELEASE)).clamp(0., 1.);
            let rise = phase * phase * (3. - 2. * phase);
            let next: [f32; 3] = std::array::from_fn(|i| {
                v.launch_from[i] + (v.launch_to[i] - v.launch_from[i]) * rise
            });
            v.speed = (next[0] - v.x).hypot(next[2] - v.z) / dt.max(0.0001);
            v.v_speed = (next[1] - v.y) / dt.max(0.0001);
            [v.x, v.y, v.z] = next;
            if v.launch_remaining == 0. {
                p.yaw = v.yaw;
                p.pitch = v.pitch;
            }
            return true;
        }
        if !v.piloted {
            return false;
        }
        let axis = |k: &str| input[k].as_f64().unwrap_or(0.).clamp(-1., 1.) as f32;
        let (s, c) = p.yaw.sin_cos();
        v.yaw = p.yaw;
        let (f, a) = (axis("forward"), axis("strafe"));
        let norm = f.hypot(a).max(1.);
        let desired = [
            (s * f + c * a) * 13. / norm,
            (key("up") as i8 - key("down") as i8) as f32 * 6.,
            (-c * f + s * a) * 13. / norm,
        ];
        let alpha = 1. - (-dt * 7.5).exp();
        for (velocity, target) in v.velocity.iter_mut().zip(desired) {
            *velocity += (target - *velocity) * alpha;
        }
        v.roll += (-a * 0.19 - v.roll) * alpha;
        v.pitch = p.pitch;
        let xx = v.x + v.velocity[0] * dt;
        let zz = v.z + v.velocity[2] * dt;
        let (dx, dz) = (xx - p.x, zz - p.z);
        let scale = LINK / dx.hypot(dz).max(LINK);
        let mut goal = [
            (p.x + dx * scale).clamp(bounds.min_x + 0.4, bounds.max_x - 0.4),
            (v.y + v.velocity[1] * dt).clamp(2.2, 25.),
            (p.z + dz * scale).clamp(bounds.min_z + 0.4, bounds.max_z - 0.4),
        ];
        let start = [v.x, v.y, v.z];
        let displacement = [goal[0] - v.x, goal[1] - v.y, goal[2] - v.z];
        let length = displacement.iter().map(|t| t * t).sum::<f32>().sqrt();
        if length > 0.00001 {
            let direction = displacement.map(|d| d / length);
            let allowed =
                (ray_scene(scene, start, direction, length + 0.3) - 0.3).clamp(0., length);
            for i in 0..3 {
                goal[i] = start[i] + direction[i] * allowed;
            }
            (v.x, v.y, v.z) = (goal[0], goal[1], goal[2]);
        }
        v.speed = (v.x - start[0]).hypot(v.z - start[2]) / dt.max(1e-5);
        v.v_speed = (v.y - start[1]) / dt.max(1e-5);
        if key("ping") && v.ping_delay <= 0. {
            v.ping_delay = 1.;
            let (sy, cy) = p.pitch.sin_cos();
            let d = [s * cy, sy, -c * cy];
            let floor = if sy < -0.001 { -v.y / sy } else { 100. };
            let t = ray_scene(scene, [v.x, v.y, v.z], d, floor.min(100.));
            if v.y + d[1] * t <= 0.1 || t < floor.min(100.) - 0.01 {
                self.markers.insert(
                    id,
                    Marker {
                        owner: id,
                        team: p.team.clone(),
                        x: (v.x + d[0] * t).clamp(bounds.min_x, bounds.max_x),
                        z: (v.z + d[2] * t).clamp(bounds.min_z, bounds.max_z),
                        remaining: 12.,
                    },
                );
                p.notice("Метка передана команде");
            } else {
                p.notice("Наведи камеру на землю или строение");
            }
        }
        true
    }
    pub(crate) fn consume_pulse(&mut self, p: &mut Player) -> Option<[f32; 3]> {
        let v = self.active.get_mut(&p.id)?;
        if !v.piloted || v.launch_remaining > 0. {
            return None;
        }
        if v.charges == 0 {
            p.notice("Импульс уже израсходован");
            return None;
        }
        if v.battery <= 14. {
            p.notice("Для импульса нужно больше 14 с заряда");
            return None;
        }
        v.charges = 0;
        v.battery -= 14.;
        p.notice("Импульс активирован");
        Some([v.x, v.y, v.z])
    }
    pub(crate) fn tick(&mut self, dt: f32, players: &BTreeMap<u32, Player>) -> Vec<u32> {
        self.cooldowns.retain(|id, t| {
            *t -= dt;
            *t > 0. && players.contains_key(id)
        });
        self.markers.retain(|_, m| {
            m.remaining -= dt;
            m.remaining > 0.
        });
        let mut ended = Vec::new();
        for (&id, v) in &mut self.active {
            if let Some(p) = players.get(&id) {
                let distance = (v.x - p.x).hypot(v.z - p.z);
                v.signal = if distance > LINK {
                    0.
                } else {
                    (1. - 0.94 * (distance / LINK).powf(1.35)).clamp(0.06, 1.)
                };
            }
            v.battery = (v.battery - dt).max(0.);
            v.ping_delay = (v.ping_delay - dt).max(0.);
            if v.battery <= 0. || !players.get(&id).is_some_and(|p| p.alive) {
                ended.push(id);
            }
        }
        for &id in &ended {
            self.recall(id, 45.);
        }
        ended
    }
    pub(crate) fn ray(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        limit: f32,
        team: &str,
    ) -> Option<(u32, f32)> {
        self.active
            .values()
            .filter(|v| v.team != team)
            .filter_map(|v| {
                let q = [v.x - o[0], v.y - o[1], v.z - o[2]];
                let t = q[0] * d[0] + q[1] * d[1] + q[2] * d[2];
                let dist2 = q
                    .iter()
                    .zip(d)
                    .map(|(n, u)| (n - t * u).powi(2))
                    .sum::<f32>();
                if dist2 > 0.33 * 0.33 {
                    return None;
                }
                let near = t - (0.33 * 0.33 - dist2).sqrt();
                (near >= 0. && near <= limit).then_some((v.id, near))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
    pub(crate) fn hit(&mut self, id: u32, damage: f32) -> Option<u32> {
        if damage <= 0. {
            return None;
        }
        let owner = self.active.values().find(|v| v.id == id)?.owner;
        self.recall(owner, 45.);
        Some(owner)
    }
    pub(crate) fn snapshot(&self, s: &mut Value) {
        s["drones"] = json!(self.active.values().collect::<Vec<_>>());
        s["markers"] = json!(self.markers.values().collect::<Vec<_>>());
        if let Some(arr) = s["players"].as_array_mut() {
            for p in arr {
                let id = p["id"].as_u64().unwrap_or(0) as u32;
                let v = self.active.get(&id);
                p["droneId"] = json!(v
                    .filter(|v| v.piloted && v.launch_remaining <= 0.)
                    .map(|v| v.id));
                p["deployedDroneId"] = json!(v.map(|v| v.id));
                if let Some(v) = v.filter(|v| v.launch_remaining > 0.) {
                    p["droneLaunch"] = json!(v.launch_remaining);
                }
                p["droneCooldown"] = json!(self.cooldowns.get(&id).copied().unwrap_or(0.));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn operator() -> Player {
        let mut p = Player::default();
        p.id = 8;
        p.team = "blue".into();
        p.alive = true;
        p
    }
    fn field() -> Bounds {
        Bounds {
            min_x: -200.,
            max_x: 200.,
            min_z: -200.,
            max_z: 200.,
        }
    }
    #[test]
    fn flight_limits_recall_and_expiry_are_authoritative() {
        let mut f = DroneFleet::default();
        let mut p = operator();
        let b = field();
        let players = BTreeMap::from([(p.id, p.clone())]);
        assert!(f.control(&mut p, &json!({"drone":true}), 0.05, &b, &[]));
        for _ in 0..150 {
            f.control(&mut p, &json!({"forward":1,"up":true}), 0.05, &b, &[]);
            f.tick(0.05, &players);
        }
        assert!((f.active[&8].z + LINK).abs() < 0.05);
        assert_eq!(f.active[&8].y, 25.);
        f.control(&mut p, &json!({"drone":true}), 0.05, &b, &[]);
        f.control(&mut p, &json!({"drone":true}), 0.05, &b, &[]);
        assert!(f.active.is_empty());
        f.tick(18.1, &players);
        f.control(&mut p, &json!({"drone":true}), 0.01, &b, &[]);
        assert!(f.active.contains_key(&8));
        f.tick(60.1, &players);
        assert!(f.active.is_empty());
        assert!((f.cooldowns[&8] - 45.).abs() < 0.01);
    }
    #[test]
    fn one_pulse_per_flight_and_no_negative_charge() {
        let mut f = DroneFleet::default();
        let mut p = operator();
        assert!(f.consume_pulse(&mut p).is_none());
        f.control(&mut p, &json!({"drone":true}), 0.05, &field(), &[]);
        assert!(f.consume_pulse(&mut p).is_some());
        assert_eq!(f.active[&8].battery, 46.);
        assert!(f.consume_pulse(&mut p).is_none());
        assert_eq!(f.active[&8].battery, 46.);
        f.active.get_mut(&8).unwrap().charges = 1;
        f.active.get_mut(&8).unwrap().battery = 9.;
        assert!(f.consume_pulse(&mut p).is_none());
        assert_eq!(f.active[&8].charges, 1);
    }
    #[test]
    fn hovering_link_uses_operator_position_and_keeps_its_battery_running() {
        let mut fleet = DroneFleet::default();
        let mut p = operator();
        let field = field();
        let key = json!({"drone":true,"droneLink":true});
        fleet.control(&mut p, &key, 0.05, &field, &[]);
        for _ in 0..20 {
            fleet.control(&mut p, &Value::Null, 0.05, &field, &[]);
        }
        let (aircraft, pose) = fleet.aircraft(p.id).unwrap();
        fleet.control(&mut p, &key, 0.05, &field, &[]);
        p.x += 80.;
        let people = BTreeMap::from([(p.id, p.clone())]);
        fleet.tick(1., &people);
        assert_eq!(fleet.active[&p.id].signal, 0.);
        fleet.control(&mut p, &key, 0.05, &field, &[]);
        assert!(!fleet.active[&p.id].piloted);
        assert!(fleet.consume_pulse(&mut p).is_none());
        assert_eq!(fleet.aircraft(p.id), Some((aircraft, pose)));
        p.x -= 80.;
        fleet.control(&mut p, &key, 0.05, &field, &[]);
        assert!(fleet.active[&p.id].piloted);
        assert_eq!(fleet.active[&p.id].battery, 59.);
        fleet.control(&mut p, &key, 0.05, &field, &[]);
        fleet.tick(59.1, &people);
        assert!(fleet.aircraft(p.id).is_none());
        assert_eq!(fleet.cooldowns[&p.id], 45.);
        fleet.tick(45.1, &people);
        fleet.control(&mut p, &key, 0.05, &field, &[]);
        assert_ne!(fleet.aircraft(p.id).unwrap().0, aircraft);
        fleet.control(&mut p, &json!({"droneRecall":true}), 0.05, &field, &[]);
        assert!(fleet.aircraft(p.id).is_none());
        assert_eq!(fleet.cooldowns[&p.id], 18.);
    }
    #[test]
    fn drone_cannot_cross_a_solid_facade_and_marker_expires() {
        let mut f = DroneFleet::default();
        let mut p = operator();
        let b = field();
        let mut wall: Prop = serde_json::from_value(
            json!({"kind":"solid","solid":true,"x":0,"z":-4,"w":9,"h":8,"d":1}),
        )
        .unwrap();
        wall.prepare_geometry();
        let scene = [wall];
        f.control(&mut p, &json!({"drone":true}), 0.05, &b, &scene);
        for _ in 0..60 {
            f.control(&mut p, &json!({"forward":1}), 0.05, &b, &scene);
        }
        assert!(f.active[&8].z < -2.8 && f.active[&8].z > -3.5);
        f.control(&mut p, &json!({"ping":true}), 0.05, &b, &scene);
        assert_eq!(f.markers[&8].team, "blue");
        f.tick(12.1, &BTreeMap::from([(8, p)]));
        assert!(f.markers.is_empty());
    }
}
