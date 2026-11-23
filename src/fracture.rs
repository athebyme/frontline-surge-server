//! Shared building geometry. The same occupied cells drive drawing, shots and passage on both peers.
//! Coordinates use metres, +Y up; the centre of the ground footprint is the local origin.
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Wood,
    Brick,
    Stone,
    Metal,
    Tile,
}
impl Material {
    fn strength(self) -> f32 {
        match self {
            Self::Wood => 34.,
            Self::Brick => 73.,
            Self::Stone => 108.,
            Self::Metal => 140.,
            Self::Tile => 27.,
        }
    }
    fn received(self, hit: Damage) -> f32 {
        match hit {
            Damage::Bullet(_) => 0.0, // Surface feedback only: small arms never remove map geometry.
            Damage::Ram(force) => {
                force
                    * match self {
                        Self::Wood => 1.35,
                        Self::Tile => 1.10,
                        Self::Brick => 0.72,
                        Self::Stone => 0.45,
                        Self::Metal => 0.14,
                    }
            }
            Damage::Grenade => match self {
                Self::Wood => 115.,
                Self::Tile => 128.,
                Self::Brick => 92.,
                Self::Stone => 57.,
                Self::Metal => 22.,
            },
            Damage::Tank => match self {
                Self::Wood => 250.,
                Self::Tile => 250.,
                Self::Brick => 235.,
                Self::Stone => 230.,
                Self::Metal => 215.,
            },
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Damage {
    Bullet(f32),
    Ram(f32),
    Grenade,
    Tank,
}
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub center: [f32; 3],
    pub size: [f32; 3],
    pub material: Material,
}
// Small surface scars are sent on the reliable map channel. Keep only the most
// recent scars per building so sustained fire cannot inflate late-join snapshots.
const MAX_SCARS: usize = 48;
#[derive(Clone, Copy, Debug)]
struct Scar {
    id: u16,
    stage: u8,
    faces: u8,
}
#[derive(Clone, Debug, Default)]
pub struct FractureState {
    pub broken: BTreeSet<u16>,
    cuts: BTreeMap<u16, u8>,
    pub revision: u32,
    pub scar_revision: u32,
    scars: VecDeque<Scar>,
    // Only touched, surviving cells need HP storage.
    wear: BTreeMap<u16, f32>,
}
impl FractureState {
    pub fn missing_material(&self) -> bool {
        !self.broken.is_empty() || !self.cuts.is_empty()
    }
    pub fn repair(&mut self, limit: usize) {
        let repaired: Vec<_> = self.broken.iter().chain(self.cuts.keys()).copied().take(limit).collect();
        for id in repaired {
            self.broken.remove(&id);
            self.cuts.remove(&id);
        }
        self.wear.retain(|id, _| self.cuts.contains_key(id));
        self.scars.clear();
        self.scar_revision = self.scar_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
    }
    /// Four low bits encode an exposed face and its two possible cut depths.
    pub fn packed_cuts(&self) -> impl Iterator<Item = u32> + '_ {
        self.cuts.iter().map(|(&id, &cut)| (u32::from(id) << 4) | u32::from(cut))
    }
    pub fn replace_cuts(&mut self, codes: impl Iterator<Item = u32>) {
        self.cuts.clear();
        self.cuts.extend(codes.filter_map(|n| {
            let cut = (n & 15) as u8;
            (cut < 12).then_some((u16::try_from(n >> 4).ok()?, cut))
        }));
    }
    pub fn clipped(&self, index: u16, mut cell: Cell) -> Cell {
        if let Some(&cut) = self.cuts.get(&index) {
            let axis = usize::from(cut / 4);
            let sign = if cut & 2 == 0 { 1. } else { -1. };
            let depth = cell.size[axis] * if cut & 1 == 0 { 0.28 } else { 0.56 };
            cell.center[axis] -= sign * depth * 0.5;
            cell.size[axis] -= depth;
        }
        cell
    }
    // Bits 0..1 = stage, bits 2..7 = affected faces, high bits = cell.
    pub fn packed_scars(&self) -> Vec<u32> {
        self.scars
            .iter()
            .map(|s| ((s.id as u32) << 8) | ((s.faces as u32) << 2) | s.stage as u32)
            .collect()
    }
    fn scar(&mut self, id: u16, faces: u8, stage: u8) -> bool {
        if let Some(scar) = self.scars.iter_mut().find(|s| s.id == id) {
            let next = (scar.stage.max(stage), scar.faces | faces);
            if next == (scar.stage, scar.faces) {
                return false;
            }
            (scar.stage, scar.faces) = next;
        } else {
            if self.scars.len() == MAX_SCARS {
                self.scars.pop_front();
            }
            self.scars.push_back(Scar { id, stage, faces });
        }
        true
    }
}
fn masonry(mat: Material) -> bool {
    matches!(mat, Material::Brick | Material::Stone | Material::Tile)
}
#[derive(Default, Debug)]
pub struct Change {
    pub removed: Vec<u16>,
    pub collapsed: Vec<u16>,
    pub chipped: Vec<u32>,
    pub touched: bool,
}
#[derive(Clone, Debug)]
pub struct Layout {
    pub cells: Vec<Cell>,
    neighbors: Vec<Vec<(u16, u8)>>,
    grounded: Vec<u16>,
    span: u16,
}
impl Cell {
    fn distance2(&self, p: [f32; 3]) -> f32 {
        (0..3)
            .map(|i| {
                ((p[i] - self.center[i]).abs() - self.size[i] * 0.5)
                    .max(0.)
                    .powi(2)
            })
            .sum()
    }
    fn ray(&self, o: [f32; 3], d: [f32; 3], limit: f32) -> Option<f32> {
        box_ray(self.center, self.size, o, d, limit)
    }
}

pub(crate) fn box_ray(center: [f32; 3], size: [f32; 3], o: [f32; 3], d: [f32; 3], limit: f32) -> Option<f32> {
    let (mut lo, mut hi) = (0.0_f32, limit);
    for i in 0..3 {
        let c = center[i] - o[i];
        let half = size[i] * 0.5;
        if d[i].abs() < 0.000001 {
            // Adjacent cell edges can differ by a few ulps at a panel seam.
            if c.abs() > half + 1e-5 {
                return None;
            }
        } else {
            let a = (c - half) / d[i];
            let b = (c + half) / d[i];
            lo = lo.max(a.min(b));
            hi = hi.min(a.max(b));
            if lo > hi {
                return None;
            }
        }
    }
    Some(lo)
}
impl Layout {
    pub fn new(kind: &str, w: f32, h: f32, d: f32, variant: u32) -> Option<Self> {
        let barrier = matches!(kind, "wall" | "barricade");
        if !barrier && !matches!(kind, "house" | "barn" | "ruin") {
            return None;
        }
        if [w, h, d].iter().any(|v| !v.is_finite() || *v <= 0.) {
            return None;
        }
        let nx = (w / 0.82).ceil().clamp(if barrier { 2. } else { 5. }, 48.) as i32;
        let nz = if barrier {
            1
        } else {
            (d / 0.86).ceil().clamp(5., 44.) as i32
        };
        let ny = (h / 0.77).ceil().clamp(if barrier { 1. } else { 5. }, 32.) as i32;
        let (dx, dy, dz) = (w / nx as f32, h / ny as f32, d / nz as f32);
        let eaves = (ny * 7 / 10).max(3);
        let mut cells = Vec::new();
        let mut keys = BTreeMap::new();
        let mut grounded = Vec::new();
        let ridge = |i: i32| {
            let t = (2. * (i as f32 + 0.5) / nx as f32 - 1.).abs();
            eaves + ((ny - 1 - eaves) as f32 * (1. - t)).round() as i32
        };
        for y in 0..ny {
            for z in 0..nz {
                for x in 0..nx {
                    let edge_x = x == 0 || x == nx - 1;
                    let end = z == 0 || z == nz - 1;
                    let roof = if barrier {
                        false
                    } else {
                        let high = ridge(x);
                        let low = if x <= nx / 2 {
                            ridge((x - 1).max(0))
                        } else {
                            ridge((x + 1).min(nx - 1))
                        };
                        y >= low.min(high) && y <= high
                    };
                    let material = if barrier {
                        if kind == "barricade" {
                            Material::Wood
                        } else {
                            Material::Stone
                        }
                    } else {
                        if y > ridge(x) || !(roof || (edge_x && y <= eaves) || end) {
                            continue;
                        }
                        if kind == "ruin"
                            && (roof
                                || y >= 2 + ((x * 3 + z * 5 + variant as i32 % 11).rem_euclid(5)))
                        {
                            continue;
                        }
                        // The entry clearance is independent of the physical size of the facade.
                        let cx = (x as f32 + 0.5) * dx - w * 0.5;
                        let cy = y as f32 * dy;
                        let door_width = if kind == "barn" { 3.4 } else { 1.95 };
                        let door = end
                            && (z == 0 || kind == "barn")
                            && cx.abs() < door_width * 0.5
                            && cy < if kind == "barn" { 2.55 } else { 2.05 };
                        let window = cy >= 0.90
                            && cy + dy <= 3.2
                            && y < eaves
                            && ((edge_x && (z == nz / 3 || z == 2 * nz / 3))
                                || (end && (x == nx / 4 || x == nx - 1 - nx / 4)));
                        if kind != "ruin" && (door || window) {
                            continue;
                        }
                        if roof {
                            if kind == "barn" {
                                Material::Metal
                            } else {
                                Material::Tile
                            }
                        } else if kind == "barn" {
                            Material::Wood
                        } else if y == 0 || kind == "ruin" {
                            Material::Stone
                        } else if end && cx.abs() < door_width && cy < 2.05 + 2. * dy && cy >= 2.05
                        {
                            Material::Wood
                        } else {
                            Material::Brick
                        }
                    };
                    let index = cells.len() as u16;
                    keys.insert((x, y, z), index);
                    if y == 0 {
                        grounded.push(index);
                    }
                    cells.push(Cell {
                        center: [
                            (x as f32 + 0.5) * dx - w * 0.5,
                            (y as f32 + 0.5) * dy,
                            (z as f32 + 0.5) * dz - d * 0.5,
                        ],
                        size: [dx, dy, dz],
                        material,
                    });
                }
            }
        }
        let mut neighbors = vec![Vec::new(); cells.len()];
        for (&(x, y, z), &id) in &keys {
            for (ax, ay, az) in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
                if let Some(&j) = keys.get(&(x + ax, y + ay, z + az)) {
                    let cost = if ay == 1 { 0 } else { 1 };
                    neighbors[id as usize].push((j, cost));
                    neighbors[j as usize].push((id, cost));
                }
            }
        }
        Some(Self {
            cells,
            neighbors,
            grounded,
            span: (nx.max(nz) / 2 + 2) as u16,
        })
    }
    pub fn ray(&self, state: &FractureState, o: [f32; 3], d: [f32; 3], limit: f32) -> Option<f32> {
        let mut hit = None;
        let mut best = limit;
        for (i, c) in self.cells.iter().enumerate() {
            let Some(mut t) = c.ray(o, d, best) else { continue };
            let id = i as u16;
            if state.broken.contains(&id) { continue; }
            if state.cuts.contains_key(&id) {
                let Some(face) = state.clipped(id, *c).ray(o, d, best) else { continue };
                t = face;
            }
            best = t;
            hit = Some(t);
        }
        hit
    }
    pub fn intersects(&self, state: &FractureState, x: f32, z: f32, r: f32) -> bool {
        self.cells.iter().enumerate().any(|(i, c)| {
            let c = state.clipped(i as u16, *c);
            if c.center[1] - c.size[1] * 0.5 >= 1.82 {
                return false;
            }
            let a = ((x - c.center[0]).abs() - c.size[0] * 0.5).max(0.);
            let b = ((z - c.center[2]).abs() - c.size[2] * 0.5).max(0.);
            a * a + b * b <= r * r && !state.broken.contains(&(i as u16))
        })
    }
    fn unsupported(&self, state: &FractureState) -> Vec<u16> {
        let mut cost = vec![u16::MAX; self.cells.len()];
        let mut frontier = BinaryHeap::new();
        for &id in &self.grounded {
            if !state.broken.contains(&id) {
                cost[id as usize] = 0;
                frontier.push(Reverse((0u16, id)));
            }
        }
        while let Some(Reverse((distance, id))) = frontier.pop() {
            if cost[id as usize] != distance {
                continue;
            }
            for &(next, extra) in &self.neighbors[id as usize] {
                let total = distance + u16::from(extra);
                if total <= self.span
                    && total < cost[next as usize]
                    && !state.broken.contains(&next)
                {
                    cost[next as usize] = total;
                    frontier.push(Reverse((total, next)));
                }
            }
        }
        cost.iter()
            .enumerate()
            .filter(|(i, c)| **c == u16::MAX && !state.broken.contains(&(*i as u16)))
            .map(|(i, _)| i as u16)
            .collect()
    }
    pub fn surface_material(&self, state: &FractureState, point: [f32; 3]) -> Option<Material> {
        self.cells
            .iter()
            .enumerate()
            .filter(|(i, _)| !state.broken.contains(&(*i as u16)))
            .map(|(i,c)| (i, state.clipped(i as u16,*c)))
            .min_by(|a, b| a.1.distance2(point).total_cmp(&b.1.distance2(point)))
            .map(|(_, cell)| cell.material)
    }
    fn surface_impact(&self, state: &mut FractureState, point: [f32; 3]) -> Change {
        let Some((index, cell)) = self
            .cells
            .iter()
            .enumerate()
            .filter(|(i, _)| !state.broken.contains(&(*i as u16)))
            .map(|(i,c)| (i, state.clipped(i as u16,*c)))
            .min_by(|a, b| a.1.distance2(point).total_cmp(&b.1.distance2(point)))
        else {
            return Change::default();
        };
        if cell.distance2(point) > 0.025 || !masonry(cell.material) {
            return Change::default();
        }
        let offset: [f32; 3] = std::array::from_fn(|k| (point[k] - cell.center[k]) / cell.size[k]);
        let axis = (0..3)
            .max_by(|&a, &b| offset[a].abs().total_cmp(&offset[b].abs()))
            .unwrap();
        let face = 1 << (2 * axis + usize::from(offset[axis] < 0.));
        let id = index as u16;
        let old = state
            .scars
            .iter()
            .find(|s| s.id == id && s.faces & face != 0)
            .map_or(0, |s| s.stage);
        let stage = (old + 1).clamp(2, 3);
        let mut changed = state.scar(id, face, stage);
        // Trace only through shared solid edges on this face of the facade.
        let mut neighbors: Vec<_> = self.neighbors[index]
            .iter()
            .map(|n| n.0)
            .filter(|&j| {
                let n = &self.cells[j as usize];
                !state.broken.contains(&j)
                    && masonry(n.material)
                    && (n.center[axis] - cell.center[axis]).abs() < 0.01
            })
            .collect();
        neighbors.sort_by(|&a, &b| {
            self.cells[a as usize]
                .distance2(point)
                .total_cmp(&self.cells[b as usize].distance2(point))
        });
        for neighbor in neighbors.into_iter().take(2) {
            changed |= state.scar(neighbor, face, stage - 1);
        }
        if changed {
            state.scar_revision = state.scar_revision.wrapping_add(1);
        }
        Change {
            touched: changed,
            ..Default::default()
        }
    }
    /// Pick an exposed face toward the impact. A surviving block keeps contact
    /// with its supports; material can only be chipped away from an open side.
    fn cut_face(&self, state: &FractureState, id: u16, impact: [f32; 3]) -> Option<u8> {
        let cell = &self.cells[usize::from(id)];
        let mut covered = [false; 6];
        for &(neighbor, _) in &self.neighbors[usize::from(id)] {
            if state.broken.contains(&neighbor) { continue; }
            let other = &self.cells[usize::from(neighbor)];
            for axis in 0..3 {
                let delta = other.center[axis] - cell.center[axis];
                if delta.abs() > 0.001 {
                    covered[axis * 2 + usize::from(delta < 0.)] = true;
                    break;
                }
            }
        }
        // The ground supports the bottom face even without an adjacent wall cell.
        covered[3] |= cell.center[1] - cell.size[1] * 0.5 < 0.001;
        (0..6).filter(|&face| !covered[face]).filter_map(|face| {
            let axis = face / 2;
            let direction = if face % 2 == 0 { 1. } else { -1. };
            let score = direction * (impact[axis] - cell.center[axis]) / cell.size[axis];
            (score > 0.001).then_some((face as u8, score))
        }).max_by(|a,b| a.1.total_cmp(&b.1)).map(|v| v.0)
    }
    pub fn damage(&self, state: &mut FractureState, p: [f32; 3], source: Damage) -> Change {
        let mut out = Change::default();
        let radius: f32 = match source {
            Damage::Bullet(_) => return self.surface_impact(state, p),
            Damage::Ram(force) => (0.9 + force * 0.010).clamp(1.1, 3.1),
            Damage::Grenade => 2.55,
            Damage::Tank => 2.85,
        };
        let candidates = self
            .cells
            .iter()
            .enumerate()
            .filter(|(i, _)| !state.broken.contains(&(*i as u16)))
            .map(|(i, c)| (i as u16, state.clipped(i as u16,*c).distance2(p)))
            .filter(|(_, d)| *d <= radius * radius);
        let hits: Vec<_> = candidates.collect();
        let mut scarred = false;
        for (id, d2) in hits {
            let mat = self.cells[id as usize].material;
            let amount = mat.received(source) * (1. - d2.sqrt() / radius).max(0.);
            if amount <= 0. {
                continue;
            }
            out.touched = true;
            let value = state.wear.entry(id).or_default();
            *value += amount;
            let wear = *value / mat.strength();
            if wear >= 1. {
                state.wear.remove(&id);
                state.cuts.remove(&id);
                state.broken.insert(id);
                out.removed.push(id);
            } else {
                if masonry(mat) && wear > 0.14 {
                    scarred |= state.scar(id, 63, (1. + 3. * wear).min(3.) as u8);
                }
                if wear >= 0.38 && mat != Material::Metal {
                    let previous = state.cuts.get(&id).copied();
                    if let Some(face) = previous.map(|c| c / 2).or_else(|| self.cut_face(state,id,p)) {
                        let cut = face * 2 + u8::from(wear >= 0.72);
                        if previous != Some(cut) {
                            state.cuts.insert(id,cut);
                            out.chipped.push((u32::from(id) << 4) | u32::from(cut));
                        }
                    }
                }
            }
        }
        if !out.removed.is_empty() {
            out.collapsed = self.unsupported(state);
            for &id in &out.collapsed {
                state.broken.insert(id);
                state.wear.remove(&id);
                state.cuts.remove(&id);
            }
            let before = state.scars.len();
            state.scars.retain(|s| !state.broken.contains(&s.id));
            scarred |= before != state.scars.len();
            out.removed.extend_from_slice(&out.collapsed);
            out.chipped.retain(|code| !state.broken.contains(&((code >> 4) as u16)));
        }
        if !out.chipped.is_empty() || !out.removed.is_empty() {
            state.revision = state.revision.wrapping_add(1);
        }
        if scarred {
            state.scar_revision = state.scar_revision.wrapping_add(1);
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gouge_has_depth_and_a_back_face_for_movement_shots_and_late_join() {
        let l = Layout::new("wall", 6., 1.6, 1., 0).unwrap();
        let mut s = FractureState::default();
        let ray = |s: &FractureState| l.ray(s, [0.3,0.8,2.], [0.,0.,-1.], 3.);
        assert_eq!(ray(&s), Some(1.5));
        let hit = l.damage(&mut s,[0.3,0.8,0.5], Damage::Grenade);
        assert!(!hit.chipped.is_empty() && hit.removed.is_empty());
        assert!((ray(&s).unwrap() - 1.78).abs() < 0.001);
        assert!(!l.intersects(&s,0.3,0.49,0.02));
        assert!(l.intersects(&s,0.3,0.,0.02));
        let mut late = FractureState::default();
        late.replace_cuts(s.packed_cuts());
        assert_eq!(ray(&s), ray(&late));
        let version = s.revision;
        l.damage(&mut s,[0.3,0.8,0.22], Damage::Bullet(25.));
        assert_eq!(s.revision,version);
        l.damage(&mut s,[0.3,0.8,0.22], Damage::Tank);
        assert_eq!(ray(&s),None);
        assert!(!l.intersects(&s,0.3,0.,0.02));
        assert!(s.packed_cuts().all(|n| !s.broken.contains(&((n >> 4) as u16))));
    }
    #[test]
    fn masonry_impacts_spread_on_one_face_without_removing_solid_cells() {
        let l = Layout::new("wall", 8., 3., 0.8, 0).unwrap();
        let mut state = FractureState::default();
        let p = [0.33, 1.15, 0.4];
        assert!(l.damage(&mut state, p, Damage::Bullet(10.)).touched);
        assert_eq!(state.scars.len(), 3);
        assert!(state.scars.iter().all(|s| s.faces == 1 << 4));
        assert_eq!(state.scars.iter().filter(|s| s.stage == 2).count(), 1);
        l.damage(&mut state, p, Damage::Bullet(10.));
        let revision = state.scar_revision;
        for _ in 0..200 {
            l.damage(&mut state, p, Damage::Bullet(10.));
        }
        assert_eq!((state.scar_revision, state.revision), (revision, 0));
        assert!(l.ray(&state, [0.33, 1.15, 3.], [0., 0., -1.], 5.).is_some());
        l.damage(&mut state, p, Damage::Tank);
        assert!(!state.broken.is_empty());
        assert!(state.scars.iter().all(|s| !state.broken.contains(&s.id)));
        assert!(!state.packed_scars().is_empty());
    }
    #[test]
    fn metal_resists_small_arms_and_an_isolated_grenade_but_a_shell_opens_it() {
        let l = Layout::new("barn", 9., 5., 8., 0).unwrap();
        let (i, c) = l
            .cells
            .iter()
            .enumerate()
            .find(|(_, c)| c.material == Material::Metal)
            .unwrap();
        let mut s = FractureState::default();
        for _ in 0..300 {
            l.damage(&mut s, c.center, Damage::Bullet(40.));
        }
        assert!(!s.broken.contains(&(i as u16)));
        l.damage(&mut s, c.center, Damage::Grenade);
        assert!(!s.broken.contains(&(i as u16)));
        l.damage(&mut s, c.center, Damage::Tank);
        assert!(s.broken.contains(&(i as u16)));
    }
    #[test]
    fn doorway_and_local_breach_are_shared_by_rays_and_avatars() {
        let l = Layout::new("house", 8., 6., 7., 0).unwrap();
        let mut s = FractureState::default();
        assert!(!l.intersects(&s, 0., -3.2, 0.35));
        assert!(l.intersects(&s, 3.8, 0., 0.36));
        assert!(l.ray(&s, [6., 1.1, 0.], [-1., 0., 0.], 3.).is_some());
        l.damage(&mut s, [3.8, 1.1, 0.], Damage::Tank);
        assert!(!l.intersects(&s, 3.8, 0., 0.36));
        assert!(l.ray(&s, [6., 1.1, 0.], [-1., 0., 0.], 3.).is_none());
        assert!(l.intersects(&s, -3.8, 0., 0.36));
        assert!(s.broken.len() < l.cells.len());
    }
    #[test]
    fn unsupported_upper_section_falls_and_intact_structures_are_stable() {
        for k in ["house", "barn", "ruin", "wall", "barricade"] {
            let l = Layout::new(k, 8., 6., 7., 2).unwrap();
            let mut s = FractureState::default();
            assert!(l.unsupported(&s).is_empty(), "floating cells in {k}");
            s.broken.extend(&l.grounded);
            assert_eq!(l.unsupported(&s).len() + l.grounded.len(), l.cells.len());
        }
    }
}
