//! Server-side development commands; transport checks the operator before dispatch.
use crate::sim::Match;
use serde_json::Value;

impl Match {
    pub(crate) fn debug_command(&mut self, id: u32, data: &Value) {
        if !self.players.get(&id).is_some_and(|p| !p.bot) {
            return;
        }
        match data["op"].as_str() {
            Some("bots") => {
                let Some(n) = data["count"].as_u64().filter(|n| *n <= 10) else {
                    return;
                };
                self.set_bots(n as usize);
                let actual = self.players.values().filter(|p| p.bot).count();
                if let Some(p) = self.players.get_mut(&id) {
                    p.notice(&format!("Отладка · ботов в матче: {actual}"));
                }
            }
            Some("grant") => {
                let Some(amount) = data["amount"].as_u64().filter(|v| (1..=32).contains(v)) else {
                    return;
                };
                let p = self.players.get_mut(&id).unwrap();
                if !p.alive {
                    return;
                }
                let slot = match data["resource"].as_str() {
                    Some("wood") => &mut p.wood,
                    Some("metal") => &mut p.metal,
                    Some("oil") => &mut p.oil,
                    Some("fuel") => &mut p.fuel,
                    Some("powder") => &mut p.powder,
                    _ => return,
                };
                let was = *slot;
                *slot = slot.saturating_add(amount as u32).min(32);
                let gained = *slot - was;
                p.notice(&format!("Отладка · получено {gained} ед."));
            }
            Some("refill") => {
                let p = self.players.get_mut(&id).unwrap();
                if !p.alive {
                    return;
                }
                p.hp = 100.;
                p.ammo = p.mag_size;
                p.reserve = p.reserve.max(64);
                p.grenades = 4;
                p.notice("Отладка · здоровье и боезапас восстановлены");
            }
            _ => {}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn bounded_tools_change_only_the_requesting_human_and_respect_capacity() {
        let mut g = Match::new();
        g.add_player(1, "host");
        g.add_player(2, "visitor");
        let before = g.players[&2].wood;
        g.debug_command(1, &json!({"op":"grant","resource":"wood","amount":32}));
        g.debug_command(1, &json!({"op":"grant","resource":"wood","amount":32}));
        assert_eq!(g.players[&1].wood, 32);
        assert_eq!(g.players[&2].wood, before);
        g.debug_command(1, &json!({"op":"bots","count":10}));
        assert_eq!(g.players.len(), 12);
        g.debug_command(1, &json!({"op":"bots","count":0}));
        assert_eq!(g.players.len(), 2);
        g.debug_command(1, &json!({"op":"grant","resource":"metal","amount":65535}));
        assert_eq!(g.players[&1].metal, 0);
    }
}
