//! Server-owned resource stacks, marked supply sites and physical inventory actions.
use super::{Match, Player};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(crate) const STACK_LIMIT: u32 = 32;
const MAX_LOOSE: usize = 64;
const LIFETIME: f32 = 600.;
const REACH: f32 = 2.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Material {
    Wood,
    Metal,
    Oil,
    Fuel,
    #[serde(alias = "gunpowder")]
    Powder,
}
impl Material {
    pub(crate) const ALL: [Self; 5] =
        [Self::Oil, Self::Fuel, Self::Metal, Self::Wood, Self::Powder];
    fn parse(key: &str) -> Option<Self> {
        Some(match key {
            "wood" => Self::Wood,
            "metal" => Self::Metal,
            "oil" => Self::Oil,
            "fuel" => Self::Fuel,
            "powder" | "gunpowder" => Self::Powder,
            _ => return None,
        })
    }
    fn label(self) -> &'static str {
        match self {
            Self::Wood => "Дерево",
            Self::Metal => "Металл",
            Self::Oil => "Нефть",
            Self::Fuel => "Топливо",
            Self::Powder => "Порох",
        }
    }
}
impl Player {
    pub(crate) fn material(&self, kind: Material) -> u32 {
        match kind {
            Material::Wood => self.wood,
            Material::Metal => self.metal,
            Material::Oil => self.oil,
            Material::Fuel => self.fuel,
            Material::Powder => self.powder,
        }
    }
    pub(crate) fn material_mut(&mut self, kind: Material) -> &mut u32 {
        match kind {
            Material::Wood => &mut self.wood,
            Material::Metal => &mut self.metal,
            Material::Oil => &mut self.oil,
            Material::Fuel => &mut self.fuel,
            Material::Powder => &mut self.powder,
        }
    }
    pub(crate) fn inventory_room(&self, key: &str) -> u32 {
        Material::parse(key).map_or(0, |kind| STACK_LIMIT.saturating_sub(self.material(kind)))
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ResourcePoint {
    id: String,
    resource: Material,
    x: f32,
    z: f32,
    initial: u32,
    capacity: u32,
    interval: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Action {
    Drop,
    Pickup,
    Load,
    Unload,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Command {
    action: Action,
    resource: Material,
    amount: u32,
    drop_id: Option<u32>,
}
impl Command {
    pub(super) fn parse(value: &Value) -> Option<Self> {
        let command = Self::deserialize(value).ok()?;
        ((1..=STACK_LIMIT).contains(&command.amount)
            && command
                .drop_id
                .is_none_or(|id| id > 0 && command.action == Action::Pickup))
        .then_some(command)
    }
}

#[derive(Debug, Serialize)]
struct Pile {
    id: u32,
    x: f32,
    z: f32,
    resource: Material,
    amount: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip)]
    generation: Option<Generation>,
    #[serde(skip)]
    expires: f32,
}
#[derive(Debug)]
struct Generation {
    capacity: u32,
    interval: f32,
    due: f32,
}

#[derive(Default)]
pub(super) struct Ground {
    piles: Vec<Pile>,
    next_id: u32,
    depot_ready_at: HashMap<usize, f32>,
}
impl Ground {
    pub(super) fn reset(&mut self, points: &[ResourcePoint], now: f32) {
        self.piles.clear();
        self.depot_ready_at.clear();
        self.next_id = 1;
        for point in points {
            let capacity = point.capacity.clamp(1, STACK_LIMIT);
            let interval = point.interval.clamp(5., 300.);
            self.piles.push(Pile {
                id: self.next_id,
                x: point.x,
                z: point.z,
                resource: point.resource,
                amount: point.initial.min(capacity),
                source: Some(point.id.clone()),
                generation: Some(Generation {
                    capacity,
                    interval,
                    due: now + interval,
                }),
                expires: f32::INFINITY,
            });
            self.next_id += 1;
        }
    }
    pub(super) fn tick(&mut self, now: f32) {
        self.piles.retain_mut(|p| {
            if let Some(g) = &mut p.generation {
                if now >= g.due {
                    let pulses = ((now - g.due) / g.interval).floor() as u32 + 1;
                    p.amount = p.amount.saturating_add(pulses).min(g.capacity);
                    g.due += pulses as f32 * g.interval;
                }
                true
            } else {
                p.expires > now
            }
        });
    }
    pub(super) fn snapshot(&self) -> Value {
        json!(self
            .piles
            .iter()
            .filter(|p| p.amount > 0)
            .collect::<Vec<_>>())
    }
    pub(super) fn depot_ready(&self, index: usize, now: f32) -> bool {
        self.depot_ready_at
            .get(&index)
            .is_none_or(|due| now >= *due)
    }
    pub(super) fn record_harvest(&mut self, index: usize, now: f32) {
        self.depot_ready_at.insert(index, now + 3.);
    }
    fn put(&mut self, x: f32, z: f32, resource: Material, requested: u32, now: f32) -> u32 {
        if requested == 0 {
            return 0;
        }
        if let Some(p) = self.piles.iter_mut().find(|p| {
            p.generation.is_none()
                && p.resource == resource
                && p.amount < STACK_LIMIT
                && (p.x - x).hypot(p.z - z) < 0.9
        }) {
            let n = requested.min(STACK_LIMIT - p.amount);
            p.amount += n;
            p.expires = now + LIFETIME;
            return n;
        }
        if self.piles.iter().filter(|p| p.generation.is_none()).count() >= MAX_LOOSE {
            return 0;
        }
        let amount = requested.min(STACK_LIMIT);
        self.piles.push(Pile {
            id: self.next_id,
            x,
            z,
            resource,
            amount,
            source: None,
            generation: None,
            expires: now + LIFETIME,
        });
        self.next_id = self.next_id.checked_add(1).expect("round pile identifiers");
        amount
    }
    fn take(&mut self, index: usize, requested: u32, now: f32) -> u32 {
        let p = &mut self.piles[index];
        let n = p.amount.min(requested);
        p.amount -= n;
        if p.amount == 0 {
            if let Some(g) = &mut p.generation {
                g.due = now + g.interval;
            } else {
                self.piles.swap_remove(index);
            }
        }
        n
    }
}

impl Match {
    fn nearest_pile(&self, p: &Player, filter: Option<&Command>) -> Option<usize> {
        self.ground
            .piles
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                if item.amount == 0
                    || filter.is_some_and(|cmd| {
                        cmd.resource != item.resource || cmd.drop_id.is_some_and(|id| id != item.id)
                    })
                {
                    return None;
                }
                let (dx, dz) = (item.x - p.x, item.z - p.z);
                let distance = dx.hypot(dz);
                if distance > REACH {
                    return None;
                }
                let length = distance.hypot(0.65);
                if self
                    .ray(
                        [p.x, 0.92, p.z],
                        [dx / length, -0.65 / length, dz / length],
                        length,
                    )
                    .0
                    .is_some()
                {
                    return None;
                }
                Some((i, distance, p.material(item.resource) >= STACK_LIMIT))
            })
            .min_by(|a, b| a.2.cmp(&b.2).then_with(|| a.1.total_cmp(&b.1)))
            .map(|(i, _, _)| i)
    }
    pub(super) fn ground_hint(&self, p: &Player) -> Option<String> {
        if self.logistics.driver_vehicle(p.id).is_some() {
            return None;
        }
        let item = &self.ground.piles[self.nearest_pile(p, None)?];
        Some(format!(
            "[E] {} ×{} · подобрать",
            item.resource.label(),
            item.amount
        ))
    }
    fn take_pile(&mut self, p: &mut Player, i: usize, max: u32) {
        let kind = self.ground.piles[i].resource;
        let room = STACK_LIMIT.saturating_sub(p.material(kind));
        if room == 0 {
            p.notice("Эта стопка уже полна · 32 ед.");
            return;
        }
        let amount = self.ground.take(i, room.min(max), self.elapsed);
        *p.material_mut(kind) += amount;
        p.notice(&format!("{} +{amount}", kind.label()));
        self.event("pickup", p, json!({"resource":kind, "amount":amount}));
    }
    pub(super) fn pickup_ground(&mut self, p: &mut Player) -> bool {
        if !p.alive || self.logistics.driver_vehicle(p.id).is_some() {
            return false;
        }
        let Some(i) = self.nearest_pile(p, None) else {
            return false;
        };
        self.take_pile(p, i, STACK_LIMIT);
        true
    }
    pub(super) fn apply_inventory(&mut self, p: &mut Player, command: Command) {
        if !p.alive || self.elapsed - p.last_use < 0.15 {
            return;
        }
        p.last_use = self.elapsed;
        let resource = command.resource;
        let amount = match command.action {
            Action::Load | Action::Unload => {
                let amount = self.logistics.transfer_material(
                    p,
                    resource,
                    command.amount,
                    command.action == Action::Load,
                );
                if amount == 0 {
                    p.notice("Нужен свой грузовик рядом, свободное место и груз");
                    return;
                }
                amount
            }
            Action::Drop | Action::Pickup if self.logistics.driver_vehicle(p.id).is_some() => {
                p.notice("Для работы на земле выйди из машины");
                return;
            }
            Action::Pickup => {
                if let Some(i) = self.nearest_pile(p, Some(&command)) {
                    self.take_pile(p, i, command.amount);
                } else {
                    p.notice("Этого груза рядом нет");
                }
                return;
            }
            Action::Drop => {
                let requested = p.material(resource).min(command.amount);
                if requested == 0 {
                    p.notice("Этого ресурса в рюкзаке нет");
                    return;
                }
                // Drop at the player's accessible feet, including in narrow alleys.
                let accepted = self.ground.put(p.x, p.z, resource, requested, self.elapsed);
                if accepted == 0 {
                    p.notice("На земле уже слишком много отдельных стопок");
                    return;
                }
                *p.material_mut(resource) -= accepted;
                accepted
            }
        };
        let verb = match command.action {
            Action::Drop => "На землю",
            Action::Load => "В кузов",
            Action::Unload => "В рюкзак",
            Action::Pickup => unreachable!(),
        };
        p.notice(&format!("{verb} · {} ×{amount}", resource.label()));
        self.event(
            "inventoryMove",
            p,
            json!({"action":command.action,"resource":resource,"amount":amount}),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(props: Value, points: Value, vehicles: Value) -> Match {
        let mut m = Match::from_level(
            serde_json::from_value(json!({
                "bounds":{"minX":-80,"maxX":80,"minZ":-80,"maxZ":80},
                "teamSpawns":{"blue":[{"x":0,"z":0,"yaw":0}],"red":[{"x":10,"z":0,"yaw":0}]},
                "props":props,"resourcePoints":points,"vehicleSpawns":vehicles
            }))
            .unwrap(),
        );
        m.add_player_with_team(1, "Один", "blue");
        m.add_player_with_team(2, "Два", "red");
        m
    }

    fn wait(m: &mut Match, seconds: f32) {
        for _ in 0..(seconds * 20.).ceil() as u32 {
            m.step(0.05);
        }
    }

    fn action(m: &mut Match, id: u32, cmd: Value) {
        m.input(id, &json!({"inventory":cmd}));
        m.step(0.05);
    }

    #[test]
    fn partial_pickup_is_conserved_visible_and_does_not_accelerate_generation() {
        let points = json!([{"id":"scrap","resource":"metal","x":0.9,"z":0,
                             "initial":4,"capacity":6,"interval":8}]);
        let mut game = scene(json!([]), points, json!([]));
        game.players.get_mut(&1).unwrap().wood = 32;
        game.players.get_mut(&1).unwrap().metal = 31;
        let take = json!({"action":"pickup","resource":"metal","amount":32,"dropId":1});
        action(&mut game, 1, take.clone());
        assert_eq!(game.players[&1].metal, 32);
        assert_eq!(game.snapshot()["groundDrops"][0]["amount"], 3);
        game.players.get_mut(&2).unwrap().x = 1.8;
        action(&mut game, 2, take);
        assert_eq!(game.players[&2].metal, 3);
        assert_eq!(game.snapshot()["groundDrops"], json!([]));
        game.add_player_with_team(3, "Зритель", "blue");
        for _ in 0..100 {
            game.input(2, &json!({"interact":true}));
            game.step(0.05);
        }
        assert_eq!(game.players[&2].metal, 3);
        assert_eq!(game.snapshot()["groundDrops"], json!([]));
        wait(&mut game, 3.2);
        assert_eq!(game.snapshot()["groundDrops"][0]["amount"], 1);
        wait(&mut game, 50.);
        assert_eq!(game.snapshot()["groundDrops"][0]["amount"], 6);
        game.reset();
        assert_eq!(game.snapshot()["groundDrops"][0]["amount"], 4);
        assert_eq!(game.players[&2].metal, 0);
    }

    #[test]
    fn player_drops_require_reach_and_visibility_and_can_be_partially_taken() {
        let mut game = scene(
            json!([{"kind":"wall","x":1.1,"z":0,"w":0.4,"d":4,
                                     "h":2.2,"solid":true}]),
            json!([]),
            json!([]),
        );
        game.players.get_mut(&1).unwrap().metal = 22;
        action(
            &mut game,
            1,
            json!({"action":"drop","resource":"metal","amount":9}),
        );
        let pile = game.snapshot()["groundDrops"][0].clone();
        assert_eq!(
            (game.players[&1].metal, pile["amount"].as_u64()),
            (13, Some(9))
        );
        let pickup = json!({"action":"pickup","resource":"metal","amount":4,"dropId":pile["id"]});
        action(&mut game, 2, pickup.clone());
        assert_eq!(game.players[&2].metal, 0, "cannot retrieve a distant pile");
        game.players.get_mut(&2).unwrap().x = 2.1;
        wait(&mut game, 0.2);
        action(&mut game, 2, pickup.clone());
        assert_eq!(
            game.players[&2].metal, 0,
            "solid wall separates these players"
        );
        game.players.get_mut(&2).unwrap().x = 0.2;
        wait(&mut game, 0.2);
        action(&mut game, 2, pickup);
        assert_eq!(
            (
                game.players[&2].metal,
                game.snapshot()["groundDrops"][0]["amount"].as_u64()
            ),
            (4, Some(5))
        );
        game.input(2, &json!({"interact":true}));
        game.step(0.05);
        assert_eq!(
            game.players[&2].metal, 4,
            "resubmitting a second action immediately is rate-limited"
        );
        wait(&mut game, 0.2);
        game.input(2, &json!({"interact":true}));
        game.step(0.05);
        assert_eq!(game.players[&2].metal, 9);
        assert_eq!(game.snapshot()["groundDrops"], json!([]));
        assert_eq!(game.players[&1].metal + game.players[&2].metal, 22);
    }

    #[test]
    fn selective_freight_only_moves_the_chosen_stack_on_a_friendly_nearby_hull() {
        let mut game = scene(
            json!([]),
            json!([]),
            json!([
            {"kind":"cargo6x6","team":"blue","x":3,"z":0,"yaw":0},
            {"kind":"apc","team":"red","x":9,"z":5,"yaw":0}]),
        );
        game.players.get_mut(&1).unwrap().wood = 32;
        game.players.get_mut(&1).unwrap().metal = 25;
        let load = json!({"action":"load","resource":"metal","amount":7});
        action(&mut game, 1, load.clone());
        assert_eq!((game.players[&1].wood, game.players[&1].metal), (32, 18));
        assert_eq!(game.snapshot()["vehicles"][0]["cargo"]["metal"], 7);
        game.players.get_mut(&2).unwrap().x = 2.;
        action(
            &mut game,
            2,
            json!({"action":"unload","resource":"metal","amount":7}),
        );
        assert_eq!(game.players[&2].metal, 0);
        wait(&mut game, 0.2);
        action(
            &mut game,
            1,
            json!({"action":"unload","resource":"metal","amount":4}),
        );
        assert_eq!(game.players[&1].metal, 22);
        assert_eq!(game.snapshot()["vehicles"][0]["cargo"]["metal"], 3);
        game.players.get_mut(&1).unwrap().z = 20.;
        wait(&mut game, 0.2);
        action(&mut game, 1, load);
        assert_eq!(game.players[&1].metal, 22);
        let red = game.players.get_mut(&2).unwrap();
        red.x = 9.;
        red.z = 7.5;
        red.fuel = 32;
        wait(&mut game, 0.2);
        action(
            &mut game,
            2,
            json!({"action":"load","resource":"fuel","amount":32}),
        );
        assert_eq!(game.players[&2].fuel, 8);
        assert_eq!(game.snapshot()["vehicles"][1]["cargo"]["fuel"], 24);
    }

    #[test]
    fn nearby_truck_materials_can_be_withdrawn_then_spent_exactly_once_at_a_workshop() {
        let mut game = scene(
            json!([{"kind":"workshop","team":"blue","x":-2,"z":0,"w":2,"d":2,"h":2,"solid":true}]),
            json!([]),
            json!([{"kind":"cargo6x6","team":"blue","x":3.1,"z":0,"yaw":0}]),
        );
        let builder = game.players.get_mut(&1).unwrap();
        builder.metal = 1;
        builder.powder = 2;
        let initial_ammo = builder.reserve;
        for (op, kind, n) in [("load", "metal", 1), ("load", "powder", 2)] {
            action(
                &mut game,
                1,
                json!({"action":op,"resource":kind,"amount":n}),
            );
            wait(&mut game, 0.25);
        }
        game.input(1, &json!({"craft":"ammo"}));
        game.step(0.05);
        assert_eq!(game.players[&1].reserve, initial_ammo);
        for (kind, n) in [("metal", 1), ("powder", 2)] {
            action(
                &mut game,
                1,
                json!({"action":"unload","resource":kind,"amount":n}),
            );
            wait(&mut game, 0.25);
        }
        game.input(1, &json!({"craft":"ammo"}));
        game.step(0.05);
        assert_eq!(game.players[&1].reserve, initial_ammo + 24);
        assert_eq!((game.players[&1].metal, game.players[&1].powder), (0, 0));
        let truck = game.snapshot()["vehicles"][0].clone();
        assert_eq!(
            (
                truck["cargo"]["metal"].as_u64(),
                truck["cargo"]["powder"].as_u64()
            ),
            (Some(0), Some(0))
        );
        wait(&mut game, 0.25);
        game.input(1, &json!({"craft":"ammo"}));
        game.step(0.05);
        assert_eq!(game.players[&1].reserve, initial_ammo + 24);
    }

    #[test]
    fn deposit_cooldown_is_shared_and_full_stacks_do_not_consume_stock() {
        let mut game = scene(
            json!([{"kind":"barrels","x":0,"z":-1.6,"w":1,"d":1,"h":1,
                                   "solid":true,"resource":"metal","stock":4}]),
            json!([]),
            json!([]),
        );
        game.players.get_mut(&2).unwrap().x = 0.35;
        game.input(1, &json!({"interact":true}));
        game.step(0.05);
        game.input(2, &json!({"interact":true}));
        game.step(0.05);
        assert_eq!(
            (
                game.players[&1].metal,
                game.players[&2].metal,
                game.props[0].stock
            ),
            (2, 0, 3)
        );
        wait(&mut game, 3.);
        game.players.get_mut(&2).unwrap().metal = 32;
        game.input(2, &json!({"interact":true}));
        game.step(0.05);
        assert_eq!(game.props[0].stock, 3);
        game.input(1, &json!({"interact":true}));
        game.step(0.05);
        assert_eq!((game.players[&1].metal, game.props[0].stock), (4, 2));
    }

    #[test]
    fn temporary_piles_are_bounded_merge_without_loss_and_expire() {
        let mut ground = Ground::default();
        ground.reset(&[], 0.);
        for i in 0..MAX_LOOSE {
            assert_eq!(ground.put(i as f32 * 2., 0., Material::Metal, 30, 5.), 30);
        }
        assert_eq!(ground.put(400., 0., Material::Metal, 2, 5.), 0);
        assert_eq!(ground.put(0., 0., Material::Metal, 5, 6.), 2);
        assert_eq!(ground.piles[0].amount, 32);
        ground.tick(605.);
        assert_eq!(ground.piles.len(), 1);
        ground.tick(606.);
        assert!(ground.piles.is_empty());
        for cmd in [
            json!({"action":"drop","resource":"metal","amount":33}),
            json!({"action":"drop","resource":"metal","amount":-1}),
            json!({"action":"pickup","resource":"metal","amount":2,"dropId":0}),
            json!({"action":"load","resource":"invalid","amount":1}),
        ] {
            assert!(Command::parse(&cmd).is_none());
        }
    }
}
