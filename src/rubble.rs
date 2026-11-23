//! Small traversable fragments on the ground. Rows use world space on a 1/32
//! unit grid; the same shape supplies the visible footprint and loose footing.
use crate::fracture::Material;

pub const LIMIT: usize = 24;
// [cell, x, z, from_x, from_y, from_z, appearance]
pub type Piece = [i32; 7];
pub fn hash(mut n: u32) -> u32 {
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb352d);
    n ^= n >> 15;
    n.wrapping_mul(0x846ca68b) ^ (n >> 16)
}
pub fn unit(seed: u32) -> f32 {
    (hash(seed) & 65535) as f32 / 65535.
}
pub fn encode(id: u16, end: [f32; 2], from: [f32; 3], seed: u32, mat: Material) -> Piece {
    let q = |v: f32| (v * 32.).round() as i32;
    let family = match mat {
        Material::Wood => 0,
        Material::Brick => 1,
        Material::Stone => 2,
        Material::Metal => 3,
        Material::Tile => 4,
    };
    [
        i32::from(id),
        q(end[0]),
        q(end[1]),
        q(from[0]),
        q(from[1]),
        q(from[2]),
        (((seed & 0x00ff_ffff) << 3) | family) as i32,
    ]
}
pub fn center(piece: &Piece) -> [f32; 2] {
    [piece[1] as f32 / 32., piece[2] as f32 / 32.]
}
pub fn material(piece: &Piece) -> Material {
    match piece[6] & 7 {
        0 => Material::Wood,
        1 => Material::Brick,
        2 => Material::Stone,
        3 => Material::Metal,
        _ => Material::Tile,
    }
}
pub fn size(piece: &Piece) -> [f32; 3] {
    let k = piece[6] as u32 >> 3;
    let scale = 0.65 + unit(k) * 0.70;
    let base = match material(piece) {
        Material::Wood => [0.78, 0.11, 0.19],
        Material::Brick => [0.47, 0.22, 0.30],
        Material::Stone => [0.60, 0.31, 0.49],
        Material::Metal => [0.61, 0.08, 0.38],
        Material::Tile => [0.41, 0.08, 0.31],
    };
    [base[0] * scale, base[1] * scale, base[2] * scale]
}
pub fn yaw(piece: &Piece) -> f32 {
    unit(piece[6] as u32 >> 3) * std::f32::consts::TAU
}
pub fn underfoot(piece: &Piece, x: f32, z: f32) -> bool {
    let [cx, cz] = center(piece);
    let dx = x - cx;
    let dz = z - cz;
    if dx.abs() > 0.70 || dz.abs() > 0.70 {
        return false;
    }
    let [w, _, d] = size(piece);
    let (s, c) = yaw(piece).sin_cos();
    (dx * c - dz * s).abs() <= w * 0.5 + 0.10 && (dx * s + dz * c).abs() <= d * 0.5 + 0.10
}
