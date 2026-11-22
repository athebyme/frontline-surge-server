//! Reliable map changes accompany the high-frequency disposable actor snapshots.
//! Replacing complete changed lists also handles recycled building slots.
#[cfg(feature = "server")]
use serde_json::json;
use serde_json::Value;

#[derive(Default)]
pub(crate) struct MapState {
    round: Option<u64>,
    props: Value,
    structures: Value,
    drops: Value,
}

impl MapState {
    #[cfg(feature = "server")]
    pub(crate) fn extract_update(&mut self, state: &mut Value) -> Option<Value> {
        let round = state["round"].as_u64().expect("authoritative round number");
        let new_round = self.round != Some(round);
        let object = state
            .as_object_mut()
            .expect("authoritative snapshot object");
        let mut update = json!({"type":"world","round":round});
        let mut changed = false;
        for (key, saved) in [
            ("propChanges", &mut self.props),
            ("builtProps", &mut self.structures),
            ("groundDrops", &mut self.drops),
        ] {
            let list = object.remove(key).expect("authoritative world list");
            if new_round || *saved != list {
                update[key] = list.clone();
                *saved = list;
                changed = true;
            }
        }
        self.round = Some(round);
        changed.then_some(update)
    }

    #[cfg(feature = "client")]
    pub(crate) fn accept(&mut self, packet: &mut Value) -> Result<(), String> {
        let round = packet["round"]
            .as_u64()
            .ok_or("В пакете карты отсутствует номер раунда.")?;
        let new_round = self.round != Some(round);
        for (key, saved) in [
            ("propChanges", &mut self.props),
            ("builtProps", &mut self.structures),
            ("groundDrops", &mut self.drops),
        ] {
            match packet.get_mut(key) {
                Some(value) if value.is_array() => *saved = value.take(),
                None if !new_round => {}
                _ => return Err("От сервера получена неполная карта.".into()),
            }
        }
        self.round = Some(round);
        Ok(())
    }

    #[cfg(feature = "client")]
    pub(crate) fn fill(&self, frame: &mut Value) -> Result<(), String> {
        if self.round.is_none() || frame["round"].as_u64() != self.round {
            return Err("Сервер прислал кадр до загрузки карты.".into());
        }
        frame["propChanges"] = self.props.clone();
        frame["builtProps"] = self.structures.clone();
        frame["groundDrops"] = self.drops.clone();
        Ok(())
    }
}
