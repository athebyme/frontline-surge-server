//! Authoritative 60 Hz simulation and TCP service. Never links a rendering backend.
use crate::world_wire::MapState;
use crate::{
    sim::Match,
    wire::{Wire, INPUT_PACKET, PROTOCOL},
};
use serde_json::{json, Value};
use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub const PLAYER_LIMIT: usize = 12;
struct Peer {
    wire: Wire,
    id: Option<u32>,
    team: String,
    developer: bool,
    seen: Instant,
    rate_at: Instant,
    count: usize,
    dead: bool,
}

fn local_developer(enabled: bool, peer: IpAddr) -> bool {
    enabled && peer.to_canonical().is_loopback()
}

fn retain_friendly_markers(state: &mut Value, team: &str) {
    if let Some(marks) = state["markers"].as_array_mut() {
        marks.retain(|mark| mark["team"].as_str() == Some(team));
    }
}

pub fn run(
    listener: TcpListener,
    stop: &AtomicBool,
    bots: usize,
    round_seconds: u32,
    dev_local: bool,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let mut bots_started = false;
    let mut game = Match::new();
    let mut world = MapState::default();
    game.set_round_seconds(round_seconds);
    let mut peers: Vec<Peer> = Vec::new();
    let mut next_id = 1u32;
    let fixed = Duration::from_secs_f64(1.0 / 60.0);
    let mut last = Instant::now();
    let mut snapshot_at = last;
    let mut debt = Duration::ZERO;
    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        for _ in 0..16 {
            let (stream, address) = match listener.accept() {
                Ok(peer) => peer,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if peers.len() >= PLAYER_LIMIT + 5 {
                continue;
            }
            if let Ok(wire) = Wire::new(stream) {
                peers.push(Peer {
                    wire,
                    id: None,
                    team: String::new(),
                    developer: local_developer(dev_local, address.ip()),
                    seen: now,
                    rate_at: now,
                    count: 0,
                    dead: false,
                });
            }
        }
        for peer in &mut peers {
            let Ok(packets) = peer.wire.frames(INPUT_PACKET) else {
                peer.dead = true;
                continue;
            };
            if now.duration_since(peer.rate_at) >= Duration::from_secs(1) {
                peer.rate_at = now;
                peer.count = 0;
            }
            peer.count += packets.len();
            if peer.count > 160 {
                peer.dead = true;
                continue;
            }
            for data in packets {
                peer.seen = now;
                if let Some(id) = peer.id {
                    if data["type"] == "input" {
                        game.input(id, &data);
                    } else if peer.developer && data["type"] == "debug" {
                        game.debug_command(id, &data);
                    }
                } else if data["type"] == "join"
                    && data["version"].as_u64() == Some(PROTOCOL)
                    && game.players.len() < PLAYER_LIMIT
                {
                    let name: String = data["name"]
                        .as_str()
                        .unwrap_or("Боец")
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(18)
                        .collect();
                    let id = next_id;
                    next_id = next_id.wrapping_add(1).max(1);
                    peer.id = Some(id);
                    let team = match data["preferredTeam"].as_str() {
                        Some("blue") => "blue",
                        Some("red") => "red",
                        _ => "auto",
                    };
                    game.add_player_with_team(id, &name, team);
                    println!("Join #{id} {name}");
                    if bots > 0 && !bots_started {
                        game.set_bots(bots);
                        bots_started = true;
                    }
                    let mut state = game.snapshot();
                    peer.team = state["players"]
                        .as_array()
                        .and_then(|players| players.iter().find(|p| p["id"] == id))
                        .and_then(|p| p["team"].as_str())
                        .unwrap_or_default()
                        .into();
                    retain_friendly_markers(&mut state, &peer.team);
                    let pack = Wire::packet(
                        &json!({"type":"welcome","version":PROTOCOL,"id":id,"developer":peer.developer,"state":state}),
                    );
                    if peer.wire.enqueue(pack).is_err() {
                        peer.dead = true;
                        break;
                    }
                } else {
                    let reason = if game.players.len() >= PLAYER_LIMIT {
                        "На сервере заняты все места для игроков."
                    } else {
                        "Версия клиента и сервера различается."
                    };
                    let _ = peer
                        .wire
                        .enqueue(Wire::packet(&json!({"type":"error","reason":reason})));
                    peer.dead = true;
                    break;
                }
            }
            let timeout = if peer.id.is_some() { 45 } else { 8 };
            if now.duration_since(peer.seen) > Duration::from_secs(timeout)
                || peer.wire.flush().is_err()
            {
                peer.dead = true;
            }
        }
        peers.retain(|p| {
            if p.dead {
                if let Some(id) = p.id {
                    game.remove_player(id);
                    println!("Leave #{id}");
                }
                false
            } else {
                true
            }
        });
        let joined = peers.iter().filter(|peer| peer.id.is_some()).count();
        debt += (now - last).min(Duration::from_millis(160));
        last = now;
        while debt >= fixed {
            if joined > 0 {
                game.step(1.0 / 60.0);
            }
            debt -= fixed;
        }
        if joined > 0 && now.duration_since(snapshot_at) >= Duration::from_millis(50) {
            snapshot_at = now;
            let mut state = game.snapshot();
            let update = world
                .extract_update(&mut state)
                .map(|packet| Wire::packet(&packet));
            // Most frames have no active reconnaissance markers. Serialize these
            // once. With active markers, share one encoded view per team.
            let (blue, red) = if state["markers"]
                .as_array()
                .is_some_and(|marks| !marks.is_empty())
            {
                let mut blue_state = state.clone();
                retain_friendly_markers(&mut blue_state, "blue");
                let mut red_state = state;
                retain_friendly_markers(&mut red_state, "red");
                (
                    Wire::packet(&json!({"type":"state","state":blue_state})),
                    Wire::packet(&json!({"type":"state","state":red_state})),
                )
            } else {
                let packet = Wire::packet(&json!({"type":"state","state":state}));
                (packet.clone(), packet)
            };
            for p in peers.iter_mut().filter(|p| p.id.is_some()) {
                if let Some(patch) = &update {
                    if p.wire.enqueue(patch.clone()).is_err() {
                        p.dead = true;
                        continue;
                    }
                }
                let packet = match p.team.as_str() {
                    "blue" => &blue,
                    "red" => &red,
                    _ => {
                        p.dead = true;
                        continue;
                    }
                };
                if p.wire.snapshot(packet.clone()).is_err() {
                    p.dead = true;
                }
            }
        }
        thread::sleep(Duration::from_millis(if joined == 0 { 12 } else { 2 }));
    }
    let end = Wire::packet(&json!({"type":"error","reason":"Сервер остановлен."}));
    for peer in &mut peers {
        let _ = peer.wire.enqueue(end.clone());
    }
    Ok(())
}

// This UDP connect only asks the local routing table for the active LAN address.
pub fn lan_endpoint(port: u16) -> Option<SocketAddr> {
    use std::net::UdpSocket;
    let outgoing = UdpSocket::bind("0.0.0.0:0").ok()?;
    outgoing
        .connect(SocketAddr::from(([192, 0, 2, 1], 9)))
        .ok()?;
    let ip = outgoing.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return None;
    }
    Some(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_loopback_peers_receive_developer_permissions() {
        for address in ["127.0.0.1", "127.0.0.2", "::1", "::ffff:127.0.0.1"] {
            assert!(local_developer(true, address.parse().unwrap()));
            assert!(!local_developer(false, address.parse().unwrap()));
        }
        for address in [
            "0.0.0.0",
            "192.168.1.2",
            "192.0.2.1",
            "::ffff:192.168.1.2",
            "fe80::2",
            "2001:db8::1",
        ] {
            assert!(!local_developer(true, address.parse().unwrap()));
        }
    }
}
