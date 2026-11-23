//! An arcade proximity ability. Charge and battery belong to the match.
use crate::sim::{Match, Player, Prop};
use serde_json::json;

const RADIUS: f32 = 6.8;
/// Sample the distance to the target and reject cover before applying a game effect.
fn falloff(origin: [f32; 3], target: [f32; 3], scene: &[Prop]) -> f32 {
    let d = [
        target[0] - origin[0],
        target[1] - origin[1],
        target[2] - origin[2],
    ];
    let distance = d.iter().map(|v| v * v).sum::<f32>().sqrt();
    if distance >= RADIUS {
        return 0.;
    }
    if distance > 0.05 {
        let direction = d.map(|v| v / distance);
        if scene
            .iter()
            .any(|p| p.ray(origin, direction, distance - 0.04).is_some())
        {
            return 0.;
        }
    }
    (1. - distance / RADIUS).sqrt()
}
impl Match {
    pub(crate) fn drone_pulse(&mut self, operator: &mut Player) {
        let Some(origin) = self.drones.consume_pulse(operator) else {
            return;
        };
        let infantry = self
            .players
            .values()
            .filter(|p| {
                p.alive && p.team != operator.team && self.logistics.driver_vehicle(p.id).is_none()
            })
            .map(|p| (p.id, falloff(origin, [p.x, 1.15, p.z], &self.props) * 43.))
            .filter(|(_, amt)| *amt > 0.)
            .collect::<Vec<_>>();
        for (id, amount) in infantry {
            self.harm(operator, id, amount, "drone_pulse", false, None);
        }
        for (i, x, y, z) in self.logistics.blast_targets(origin[0], origin[2], RADIUS) {
            let amount = falloff(origin, [x, y, z], &self.props) * 86.;
            if amount > 0. {
                let events = self.logistics.hit(i, amount, operator.id, &operator.team);
                self.ingest_logistics(events, operator);
            }
        }
        self.scene_event("dronePulse",json!({"actor":operator.id,"team":operator.team,"x":origin[0],"y":origin[1],"z":origin[2],"radius":RADIUS}));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pilot_input_spends_one_charge_and_only_scorches_exposed_enemies() {
        let mut game = Match::new();
        for (id, team) in [(1, "blue"), (2, "red"), (3, "blue")] {
            game.add_player_with_team(id, "fixture", team);
        }
        let vehicles = game.snapshot()["vehicles"].as_array().unwrap().clone();
        let hull = vehicles
            .iter()
            .find(|v| v["team"] == "red" && v["kind"] == "cargo6x6")
            .unwrap();
        let x = hull["x"].as_f64().unwrap() as f32;
        let z = hull["z"].as_f64().unwrap() as f32;
        // Clear nearby scenery, keeping the length of the authoring/static snapshot stable.
        for o in &mut game.props {
            if (x - o.x).hypot(z - o.z) < 15. {
                o.solid = false;
            }
        }
        for (id, offset) in [(1, 0.), (2, 1.), (3, -1.)] {
            let p = game.players.get_mut(&id).unwrap();
            p.x = x + offset;
            p.z = z;
            p.shield = 0.;
        }
        game.input(1, &json!({"seq":1,"drone":true}));
        game.step(0.02);
        game.input(1, &json!({"seq":2,"pulse":true}));
        game.step(0.02);
        let after = game.snapshot();
        assert_eq!(after["drones"][0]["charges"], 0);
        let hurt = game.players[&2].hp;
        assert!(hurt < 100. && hurt > 55.);
        assert_eq!(game.players[&1].hp, 100.);
        assert_eq!(game.players[&3].hp, 100.);
        let truck = after["vehicles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == hull["id"])
            .unwrap();
        assert!(truck["hp"].as_f64().unwrap() < hull["hp"].as_f64().unwrap());
        game.input(1, &json!({"seq":3,"pulse":true}));
        game.step(0.02);
        assert_eq!(game.players[&2].hp, hurt);
        assert_eq!(
            game.snapshot()["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["type"] == "dronePulse")
                .count(),
            1
        );
    }
    #[test]
    fn range_and_cover_limit_arcade_damage() {
        let mut p: Prop = serde_json::from_value(
            json!({"x":0,"z":2,"kind":"solid","w":5,"h":4,"d":1,"solid":true}),
        )
        .unwrap();
        p.prepare_geometry();
        let source = [0., 2., 0.];
        assert!(falloff(source, [0., 2., 3.], &[]) > 0.7);
        assert_eq!(falloff(source, [0., 2., 3.], &[p]), 0.);
        assert_eq!(falloff(source, [7., 2., 0.], &[]), 0.);
    }
}
