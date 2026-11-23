#![cfg(feature = "server")]
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Process(Child);
impl Process {
    fn quit(&mut self) {
        writeln!(self.0.stdin.as_mut().unwrap(), "quit").unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "server did not shut down within four seconds"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn launch(bots: usize, minutes: u32) -> (Process, SocketAddr) {
    launch_configured(bots, minutes, "127.0.0.1", false)
}
fn launch_configured(
    bots: usize,
    minutes: u32,
    bind: &str,
    dev_local: bool,
) -> (Process, SocketAddr) {
    let mut process = Process(
        Command::new(env!("CARGO_BIN_EXE_frontline-server"))
            .args([
                "--bind",
                bind,
                "--port=0",
                "--bots",
                &bots.to_string(),
                "--round-minutes",
                &minutes.to_string(),
            ])
            .args(dev_local.then_some("--dev-local"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut lines = BufReader::new(process.0.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(
            lines.read_line(&mut line).unwrap() > 0,
            "server exited before accepting clients"
        );
        if let Some(address) = line
            .strip_prefix("Listening on ")
            .and_then(|x| x.split_whitespace().next())
        {
            let address = address.parse().unwrap();
            // Keep the console reader alive until the process exits.
            std::thread::spawn(move || {
                let mut tail = String::new();
                let _ = lines.read_to_string(&mut tail);
            });
            return (process, address);
        }
    }
}
fn send(stream: &mut TcpStream, value: Value) {
    let data = serde_json::to_vec(&value).unwrap();
    stream
        .write_all(&(data.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&data).unwrap();
}
fn read(stream: &mut TcpStream) -> Value {
    let mut header = [0; 4];
    stream.read_exact(&mut header).unwrap();
    let n = u32::from_be_bytes(header) as usize;
    assert!(n <= 4 * 1024 * 1024);
    let mut data = vec![0; n];
    stream.read_exact(&mut data).unwrap();
    serde_json::from_slice(&data).unwrap()
}
fn join(address: SocketAddr, name: &str, team: &str) -> (TcpStream, Value) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    send(
        &mut stream,
        json!({"type":"join","version":frontline_surge::PROTOCOL,"name":name,"preferredTeam":team}),
    );
    let value = read(&mut stream);
    assert_eq!(value["type"], "welcome");
    assert_eq!(value["version"], frontline_surge::PROTOCOL);
    (stream, value)
}
fn until(stream: &mut TcpStream, predicate: impl Fn(&Value) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let frame = read(stream);
        if frame["type"] == "state" && predicate(&frame["state"]) {
            return;
        }
    }
    panic!("expected state not received");
}
#[test]
fn two_real_clients_share_one_dedicated_authority() {
    let (mut server, address) = launch(0, 120);
    let (mut alice, welcome) = join(address, "Alice", "auto");
    let (mut bob, other) = join(address, "Bob", "auto");
    let id = welcome["id"].as_u64().unwrap();
    let initial = other["state"]["players"].as_array().unwrap();
    assert_eq!(initial.len(), 2);
    assert_ne!(initial[0]["team"], initial[1]["team"]);
    let player = initial.iter().find(|p| p["id"] == id).unwrap();
    let (x, z) = (player["x"].as_f64().unwrap(), player["z"].as_f64().unwrap());
    send(
        &mut alice,
        json!({"type":"input","seq":1,"forward":1.0,"yaw":player["yaw"]}),
    );
    until(&mut bob, |state| {
        state["players"].as_array().unwrap().iter().any(|p| {
            p["id"] == id
                && (p["x"].as_f64().unwrap() - x).hypot(p["z"].as_f64().unwrap() - z) > 0.1
        })
    });
    drop(bob);
    until(&mut alice, |state| {
        state["players"].as_array().unwrap().len() == 1
    });
    let mut old = TcpStream::connect(address).unwrap();
    old.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    send(
        &mut old,
        json!({"type":"join","version":frontline_surge::PROTOCOL - 1,"name":"outdated"}),
    );
    assert_eq!(read(&mut old)["type"], "error");
    server.quit();
}

#[test]
fn bots_share_the_slot_limit_and_three_hour_setting_reaches_clients() {
    let (mut server, address) = launch(10, 180);
    let (_alice, _) = join(address, "Alice", "auto");
    let (_bob, welcome) = join(address, "Bob", "auto");
    assert_eq!(welcome["state"]["players"].as_array().unwrap().len(), 12);
    assert_eq!(welcome["state"]["roundSeconds"].as_f64(), Some(10800.0));
    let mut extra = TcpStream::connect(address).unwrap();
    extra
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    send(
        &mut extra,
        json!({"type":"join","version":frontline_surge::PROTOCOL,"name":"Spectator"}),
    );
    assert_eq!(read(&mut extra)["type"], "error");
    server.quit();
}

#[test]
fn friends_can_pick_one_team_and_auto_fills_the_other() {
    let (mut server, address) = launch(0, 120);
    let (_first, _) = join(address, "First", "red");
    let (_second, _) = join(address, "Second", "red");
    let (_third, welcome) = join(address, "Third", "auto");
    let roster = welcome["state"]["players"].as_array().unwrap();
    for (name, expected) in [("First", "red"), ("Second", "red"), ("Third", "blue")] {
        let player = roster.iter().find(|p| p["name"] == name).unwrap();
        assert_eq!(player["team"], expected);
    }
    server.quit();
}

#[test]
fn recon_markers_are_private_to_allies_in_live_and_late_join_packets() {
    let (mut server, address) = launch(0, 120);
    let (mut pilot, first) = join(address, "Scout", "blue");
    let (mut rival, _) = join(address, "Rival", "red");
    let id = first["id"].as_u64().unwrap();
    let owned = |list: &Value| list.as_array().unwrap().iter().any(|p| p["owner"] == id);
    send(&mut pilot, json!({"type":"input","seq":1,"drone":true}));
    until(&mut rival, |s| owned(&s["drones"]));
    send(
        &mut pilot,
        json!({"type":"input","seq":2,"pitch":-1.1,"ping":true}),
    );
    until(&mut pilot, |s| owned(&s["markers"]));
    let (_ally, ally) = join(address, "Late ally", "blue");
    let (_enemy, enemy) = join(address, "Late enemy", "red");
    assert!(owned(&ally["state"]["markers"]));
    assert!(!owned(&enemy["state"]["markers"]));
    assert_eq!(ally["state"]["scores"], enemy["state"]["scores"]);
    for _ in 0..4 {
        until(&mut rival, |s| {
            assert!(!owned(&s["markers"]));
            true
        });
    }
    send(&mut pilot, json!({"type":"input","seq":3,"drone":true}));
    until(&mut pilot, |s| !owned(&s["drones"]));
    server.quit();
}

#[test]
fn harvested_supply_is_sent_once_and_in_the_next_join_baseline() {
    let (mut server, address) = launch(0, 120);
    let (mut worker, first) = join(address, "Quartermaster", "blue");
    assert!(first["state"]["propChanges"].is_array() && first["state"]["builtProps"].is_array());
    until(&mut worker, |s| {
        !s.as_object().unwrap().contains_key("propChanges")
    });
    send(&mut worker, json!({"type":"input","seq":1,"interact":true}));
    let mut patch = None;
    for _ in 0..20 {
        let frame = read(&mut worker);
        if frame["type"] == "world"
            && frame["propChanges"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        {
            assert!(frame.get("builtProps").is_none());
            patch = Some(frame["propChanges"].clone());
            break;
        }
    }
    let patch = patch.expect("resource stock update");
    for _ in 0..4 {
        let frame = read(&mut worker);
        assert_eq!(frame["type"], "state");
        assert!(
            frame["state"].get("propChanges").is_none()
                && frame["state"].get("builtProps").is_none()
        );
    }
    let (_new, late) = join(address, "Newcomer", "red");
    assert_eq!(late["state"]["propChanges"], patch);
    server.quit();
}

#[test]
fn developer_grants_require_server_opt_in_and_loopback_connection() {
    fn issue(address: SocketAddr, enabled: bool) {
        let (mut player, welcome) = join(address, "Operator", "blue");
        assert_eq!(welcome["developer"], enabled);
        assert_eq!(welcome["vehicleService"], enabled);
        let id = &welcome["id"];
        let metal = |state: &Value| {
            state["players"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| &p["id"] == id)
                .unwrap()["metal"]
                .as_u64()
                .unwrap()
        };
        let before = metal(&welcome["state"]);
        send(
            &mut player,
            json!({"type":"debug","op":"grant","resource":"metal","amount":16,"developer":true}),
        );
        let expected = if enabled {
            (before + 16).min(32)
        } else {
            before
        };
        for _ in 0..3 {
            until(&mut player, |s| metal(s) == expected);
        }
        send(
            &mut player,
            json!({"type":"debug","op":"vehicleService","developer":true}),
        );
        send(&mut player, json!({"type":"input","seq":1}));
        until(&mut player, |s| {
            let operator = s["players"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| &p["id"] == id)
                .unwrap();
            if operator["seq"] != 1 {
                return false;
            }
            let handled = matches!(
                operator["message"].as_str(),
                Some("Подойди к союзной машине" | "Машина отремонтирована, топливо и боезапас пополнены")
            );
            assert_eq!(handled, enabled);
            true
        });
    }
    let (mut normal, address) = launch(0, 120);
    issue(address, false);
    normal.quit();
    let (mut dev, address) = launch_configured(0, 120, "0.0.0.0", true);
    issue(SocketAddr::from(([127, 0, 0, 1], address.port())), true);
    // A connection to this machine's LAN address exercises the non-loopback peer path
    // on development machines. Loopback-only test environments have no such endpoint.
    if let Some(lan) = frontline_surge::server::lan_endpoint(address.port()) {
        issue(lan, false);
    }
    dev.quit();
}

#[test]
fn loose_inventory_survives_late_join_and_clears_on_pickup() {
    fn metal(state: &Value, id: &Value) -> u64 {
        state["players"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| &p["id"] == id)
            .unwrap()["metal"]
            .as_u64()
            .unwrap()
    }
    fn world(stream: &mut TcpStream, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            let frame = read(stream);
            if frame["type"] == "world"
                && frame["groundDrops"].is_array()
                && predicate(&frame["groundDrops"])
            {
                return frame["groundDrops"].clone();
            }
        }
        panic!("ground inventory patch not received")
    }
    let (mut server, address) = launch_configured(0, 120, "127.0.0.1", true);
    let (mut owner, welcome) = join(address, "Quartermaster", "blue");
    let id = &welcome["id"];
    let before = metal(&welcome["state"], id);
    let gifted = (before + 16).min(32);
    send(
        &mut owner,
        json!({"type":"debug","op":"grant","resource":"metal","amount":16}),
    );
    until(&mut owner, |s| metal(s, id) == gifted);
    send(
        &mut owner,
        json!({"type":"input","seq":1,"inventory":{"action":"drop","resource":"metal","amount":5}}),
    );
    let loose = |item: &Value| {
        item["resource"] == "metal" && item["amount"] == 5 && item.get("source").is_none()
    };
    let drops = world(&mut owner, |items| {
        items.as_array().unwrap().iter().any(loose)
    });
    let pile = &drops
        .as_array()
        .unwrap()
        .iter()
        .find(|item| loose(item))
        .unwrap()["id"];
    let (mut observer, late) = join(address, "Late", "red");
    assert!(late["state"]["groundDrops"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| &p["id"] == pile));
    assert_eq!(metal(&late["state"], id), gifted - 5);
    // Wait for the ordinary interaction cooldown via real authority frames.
    for _ in 0..5 {
        until(&mut observer, |s| {
            assert!(s.get("groundDrops").is_none());
            true
        });
    }
    send(
        &mut owner,
        json!({"type":"input","seq":2,"inventory":{"action":"pickup","resource":"metal","amount":5,"dropId":pile}}),
    );
    world(&mut observer, |items| {
        items.as_array().unwrap().iter().all(|p| &p["id"] != pile)
    });
    until(&mut observer, |s| metal(s, id) == gifted);
    server.quit();
}
