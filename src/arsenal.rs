//! Server-owned, per-weapon workshop loadouts and trigger scheduling.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Optic {
    Irons,
    Reflex,
    Scope,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Standard,
    Light,
    Stable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireMode {
    Semi,
    Burst2,
    Burst3,
    Auto,
    SlowAuto,
}

impl Optic {
    pub fn key(self) -> &'static str {
        match self {
            Self::Irons => "irons",
            Self::Reflex => "reflex",
            Self::Scope => "scope",
        }
    }
}
impl Grip {
    pub fn key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Light => "light",
            Self::Stable => "stable",
        }
    }
}
impl FireMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Semi => "semi",
            Self::Burst2 | Self::Burst3 => "burst",
            Self::Auto | Self::SlowAuto => "auto",
        }
    }
    fn burst(self) -> u8 {
        match self {
            Self::Burst2 => 2,
            Self::Burst3 => 3,
            _ => 1,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Kit {
    pub optic: Optic,
    pub grip: Grip,
    pub mode: FireMode,
}
impl Kit {
    pub fn ads_fov(self) -> f32 {
        match self.optic {
            Optic::Irons => 61.,
            Optic::Reflex => 53.,
            Optic::Scope => 35.,
        }
    }
    /// Multiplier for the existing shoulder raise/lower speed.
    pub fn aim_speed(self) -> f32 {
        let sight = match self.optic {
            Optic::Irons => 1.12,
            Optic::Reflex => 1.0,
            Optic::Scope => 0.70,
        };
        sight
            * match self.grip {
                Grip::Standard => 1.,
                Grip::Light => 1.15,
                Grip::Stable => 0.91,
            }
    }
    pub fn mobility(self) -> f32 {
        match self.grip {
            Grip::Standard => 1.,
            Grip::Light => 1.065,
            Grip::Stable => 0.96,
        }
    }
    pub fn spread_multiplier(self) -> f32 {
        match self.grip {
            Grip::Standard => 1.,
            Grip::Light => 1.17,
            Grip::Stable => 0.76,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Arsenal {
    slots: [Kit; 3],
}
impl Default for Arsenal {
    fn default() -> Self {
        use FireMode::*;
        use Optic::*;
        Self {
            slots: [(Scope, Semi), (Irons, Auto), (Reflex, Semi)].map(|(optic, mode)| Kit {
                optic,
                mode,
                grip: Grip::Standard,
            }),
        }
    }
}
impl Arsenal {
    pub fn equipped(&self, slot: usize) -> Kit {
        self.slots.get(slot).copied().unwrap_or(self.slots[0])
    }
    pub fn publish(&self, slot: usize, p: &mut Value) {
        let k = self.equipped(slot);
        p["optic"] = k.optic.key().into();
        p["grip"] = k.grip.key().into();
        p["fireMode"] = k.mode.key().into();
        p["adsFov"] = k.ads_fov().into();
    }
    pub fn cycle_mode(&mut self, slot: usize) {
        use FireMode::*;
        let Some(k) = self.slots.get_mut(slot) else {
            return;
        };
        k.mode = match (slot, k.mode) {
            (0, Semi) => Burst2,
            (1, Auto) => Burst3,
            (1, Burst3) => Semi,
            (1, _) => Auto,
            (2, Semi) => Burst3,
            (2, Burst3) => SlowAuto,
            _ => Semi,
        };
    }
    /// An accepted setting is idempotent. Workshop proximity is enforced by the caller.
    pub fn fit(&mut self, slot: usize, category: &str, key: &str) -> bool {
        let Some(k) = self.slots.get_mut(slot) else {
            return false;
        };
        match category {
            "optic" | "fitOptic" => {
                k.optic = match key {
                    "irons" => Optic::Irons,
                    "reflex" => Optic::Reflex,
                    "scope" => Optic::Scope,
                    _ => return false,
                }
            }
            "grip" | "fitGrip" => {
                k.grip = match key {
                    "standard" => Grip::Standard,
                    "light" => Grip::Light,
                    "stable" => Grip::Stable,
                    _ => return false,
                }
            }
            _ => return false,
        }
        true
    }
}
#[derive(Clone, Debug, Default)]
pub struct FireControl {
    burst_left: u8,
}
impl FireControl {
    /// A burst continues after release. Inputs in a locked recovery interval are discarded.
    pub fn ready(&mut self, held: bool, pressed: bool, ready: bool, mode: FireMode) -> bool {
        if !ready {
            return false;
        }
        if self.burst_left > 0 {
            self.burst_left -= 1;
            return true;
        }
        if pressed || (held && matches!(mode, FireMode::Auto | FireMode::SlowAuto)) {
            self.burst_left = mode.burst() - 1;
            return true;
        }
        false
    }
    /// Query after ready() grants the shot. A full burst has a distinct recovery period.
    pub fn cooldown(&self, base: f32, mode: FireMode) -> f32 {
        match mode {
            FireMode::Burst2 | FireMode::Burst3 if self.burst_left > 0 => base * 0.82,
            FireMode::Burst2 | FireMode::Burst3 => base * 1.65,
            FireMode::SlowAuto => base * 1.24,
            _ => base,
        }
    }
    pub fn reset(&mut self) {
        self.burst_left = 0;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kit_choices_and_modes_stay_with_the_weapon() {
        let mut a = Arsenal::default();
        assert_eq!(a.equipped(0).optic, Optic::Scope);
        assert!(a.fit(1, "optic", "reflex"));
        assert!(a.fit(1, "grip", "stable"));
        assert!(!a.fit(1, "grip", "bogus"));
        assert_eq!(a.equipped(2).grip, Grip::Standard);
        assert!(a.equipped(1).spread_multiplier() < 1.);
        a.cycle_mode(1);
        assert_eq!(a.equipped(1).mode, FireMode::Burst3);
        a.cycle_mode(0);
        assert_eq!(a.equipped(0).mode, FireMode::Burst2);
        let mut v = serde_json::json!({});
        a.publish(1, &mut v);
        assert_eq!(v["adsFov"], 53.);
        assert_eq!(v["fireMode"], "burst");
    }
    #[test]
    fn released_burst_finishes_once_and_switch_cancels() {
        let mut gate = FireControl::default();
        let m = FireMode::Burst3;
        assert!(gate.ready(true, true, true, m));
        assert!(gate.cooldown(1., m) < 1.);
        assert!(!gate.ready(false, false, false, m));
        assert!(gate.ready(false, false, true, m));
        assert!(gate.ready(false, false, true, m));
        assert!(gate.cooldown(1., m) > 1.);
        assert!(!gate.ready(false, false, true, m));
        assert!(gate.ready(true, true, true, m));
        gate.reset();
        assert!(!gate.ready(true, false, true, m));
        assert!(gate.ready(true, false, true, FireMode::SlowAuto));
        assert!(!gate.ready(true, false, true, FireMode::Semi));
    }
}
