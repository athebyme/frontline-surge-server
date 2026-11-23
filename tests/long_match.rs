//! Exercise persistent fracture snapshots through the public game interface.
//! Run with cargo test --release --no-default-features --features server --test long_match -- --nocapture.
#![cfg(feature = "server")]
use frontline_surge::sim::Match;
use serde_json::{json, Value};

fn broken_cells(snapshot: &Value) -> usize {
    snapshot["propChanges"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|prop| prop["broken"].as_array())
        .map(Vec::len)
        .sum()
}
fn advance(game: &mut Match, steps: usize) {
    for _ in 0..steps {
        game.step(0.1);
    }
}
fn encoded_frame_bytes(snapshot: &Value) -> usize {
    4 + serde_json::to_vec(&json!({"type":"state","state":snapshot}))
        .unwrap()
        .len()
}

#[test]
fn structural_changes_outlive_events_and_survive_late_join_and_round_reset() {
    let mut game = Match::new();
    let fresh = game.snapshot();
    assert_eq!(fresh["roundSeconds"], 7200);
    let mut peak_bytes = 0;
    // Cycle a full lobby through real spawns and inputs. Each new volunteer
    // brings their normal starting inventory. No wall, cell or HP is injected.
    for wave in 0..16 {
        for id in 1..=12 {
            game.add_player(id, &format!("QA {id}"));
        }
        let before = game.snapshot();
        for unit in before["players"].as_array().unwrap() {
            let id = unit["id"].as_u64().unwrap() as u32;
            let side = if unit["team"] == "blue" { 1. } else { -1. };
            let (cx, cz) = if (id / 2 + wave) % 2 == 0 {
                (-244., 148.)
            } else {
                (-218.5, 139.)
            };
            let span = ((id / 2 + wave) % 5) as f64 - 2.;
            let dx = side * (cx + span) - unit["x"].as_f64().unwrap();
            let dz = side * (cz - span * 0.65) - unit["z"].as_f64().unwrap();
            let pitch = 0.18 + ((id + wave) % 3) as f64 * 0.16;
            game.input(
                id,
                &json!({"grenade":true,"yaw":dx.atan2(-dz),"pitch":pitch}),
            );
        }
        advance(&mut game, 20);
        peak_bytes = peak_bytes.max(encoded_frame_bytes(&game.snapshot()));
        if wave < 15 {
            for id in 1..=12 {
                game.remove_player(id);
            }
        }
    }
    let damaged = game.snapshot();
    let count = broken_cells(&damaged);
    assert!(
        count >= 200,
        "expected hundreds of damaged cells, got {count}"
    );
    advance(&mut game, 90);
    game.remove_player(11);
    game.add_player(77, "Late joiner");
    let joined = game.snapshot();
    assert_eq!(
        broken_cells(&joined),
        count,
        "a late join must receive the settled breach"
    );
    assert!(!joined["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["type"] == "fracture"));
    // The wire supports larger map baselines. This representative case should
    // still leave ample room for larger maps and simultaneous effect events.
    assert!(
        peak_bytes < 128 * 1024,
        "unexpected growth in the fracture fixture"
    );
    let frame = encoded_frame_bytes(&joined);
    eprintln!("{count} persistent cells, peak frame {peak_bytes} B, settled frame {frame} B, repeated full snapshots at 20 Hz would use {:.2} Mbit/s per client", frame as f64*160./1e6);

    // Test from the shipped two-hour start through the actual round transition.
    let remain = joined["remaining"].as_f64().unwrap();
    advance(&mut game, (remain * 10.).ceil() as usize);
    assert_eq!(game.snapshot()["phase"], "ended");
    advance(&mut game, 102);
    let reset = game.snapshot();
    assert_eq!(reset["round"], 2);
    assert_eq!(reset["phase"], "playing");
    assert_eq!(reset["players"].as_array().unwrap().len(), 12);
    assert_eq!(reset["vehicles"], fresh["vehicles"]);
    assert_eq!(reset["groundDrops"], fresh["groundDrops"]);
    assert_eq!(broken_cells(&reset), 0);
}
