//! Regression tests for the authoritative actions sent by a player and the state
//! clients receive. Coordinates are small so these tests do not need a renderer.
use super::*;

fn player(id: u32, team: &str, x: f32, z: f32) -> Player {
    let mut p = Player::default();
    p.id = id;
    p.team = team.into();
    p.x = x;
    p.z = z;
    p.alive = true;
    p
}

fn spawn(kind: &str, x: f32) -> VehicleSpawn {
    VehicleSpawn {
        kind: kind.into(),
        team: "blue".into(),
        x,
        z: 0.,
        yaw: 0.,
    }
}

fn pool(vehicle: &str) -> Logistics {
    let mut logistics = Logistics::default();
    logistics.reset(&[spawn(vehicle, 0.)]);
    logistics
}

fn bounds() -> Bounds {
    Bounds {
        min_x: -500.,
        max_x: 500.,
        min_z: -500.,
        max_z: 500.,
    }
}

fn prop(kind: &str, team: &str, x: f32, z: f32, w: f32, d: f32) -> Prop {
    let mut p = Prop {
        kind: kind.into(),
        team: Some(team.into()),
        x,
        z,
        w,
        d,
        h: 2.,
        solid: true,
        built: true,
        ..Default::default()
    };
    p.prepare_geometry();
    p
}

fn action(l: &mut Logistics, p: &mut Player, scene: &[Prop], key: &str) -> Vec<Value> {
    let pilots = BTreeMap::from([(p.id, p.clone())]);
    l.tick(0.15, scene, &pilots);
    l.update(p, &json!({(key):true}), 1. / 60., scene, &bounds())
}

fn cargo_totals(p: &Player, v: &Vehicle) -> [u32; 5] {
    [
        p.wood + v.cargo.wood,
        p.metal + v.cargo.metal,
        p.oil + v.cargo.oil,
        p.fuel + v.cargo.fuel,
        p.powder + v.cargo.powder,
    ]
}

#[test]
fn a_turn_bends_the_travel_direction_gradually_and_reverse_unwinds_the_wheels() {
    let mut l = pool("patrol4x4");
    let mut p = player(1, "blue", 2.3, 0.);
    action(&mut l, &mut p, &[], "vehicle");
    for _ in 0..20 {
        l.update(&mut p, &json!({"forward":1}), 0.05, &[], &bounds());
    }
    let (x, z) = (p.x, p.z);
    l.update(&mut p, &json!({"forward":1,"strafe":1}), 0.05, &[], &bounds());
    let heading = (p.x - x).atan2(z - p.z);
    assert!(heading > 0. && heading < l.vehicles[0].yaw);

    for _ in 0..40 {
        l.update(&mut p, &json!({"forward":-1}), 0.05, &[], &bounds());
    }
    let travel = l.vehicles[0].travel;
    let (x, z) = (p.x, p.z);
    l.update(&mut p, &json!({"forward":-1}), 0.05, &[], &bounds());
    assert!(l.vehicles[0].speed < 0.);
    assert!((travel - l.vehicles[0].travel - distance(x, z, p.x, p.z)).abs() < 0.001);
}

#[test]
fn debug_service_selects_only_live_friendly_vehicles_and_prefers_the_driver_seat() {
    let mut l = Logistics::default();
    let mut enemy = spawn("mbt", 0.);
    enemy.team = "red".into();
    l.reset(&[
        enemy,
        spawn("mbt", 0.),
        spawn("apc", 2.),
        spawn("mbt", 5.),
    ]);
    l.vehicles[1].destroyed = true;
    for v in &mut l.vehicles {
        v.hp = 1.;
        v.fuel = 2.;
        v.mg_ammo = 3;
        v.shells = 0;
    }
    let mut p = player(1, "blue", 0., 0.);
    l.debug_vehicle_service(&mut p);
    let apc = &l.vehicles[2];
    assert_eq!(
        (apc.hp, apc.fuel, apc.mg_ammo),
        (apc.max_hp, apc.max_fuel, apc.mg_capacity)
    );
    assert_eq!(apc.shells, 0);
    assert_eq!(l.vehicles[0].hp, 1.);
    assert_eq!(l.vehicles[1].hp, 1.);
    assert_eq!(l.vehicles[3].hp, 1.);

    l.vehicles[2].hp = 7.;
    l.vehicles[3].driver = Some(p.id);
    l.vehicles[3].max_hp += 80.;
    l.vehicles[3].max_fuel += 10.;
    l.debug_vehicle_service(&mut p);
    let tank = &l.vehicles[3];
    assert_eq!(
        (tank.hp, tank.fuel, tank.mg_ammo, tank.shells),
        (tank.max_hp, tank.max_fuel, tank.mg_capacity, MAX_TANK_SHELLS)
    );
    assert_eq!(l.vehicles[2].hp, 7.);
    l.vehicles[3].driver = None;

    p.alive = false;
    l.debug_vehicle_service(&mut p);
    assert_eq!(l.vehicles[2].hp, 7.);
    p.alive = true;
    p.x = 15.;
    l.debug_vehicle_service(&mut p);
    assert_eq!(l.vehicles[2].hp, 7.);
    assert_eq!(p.message, "Подойди к союзной машине");
}

#[test]
fn cargo_is_team_owned_conserves_each_resource_and_respects_both_capacity_limits() {
    let mut l = pool("cargo6x6");
    let mut ally = player(7, "blue", 2.8, 0.);
    let mut foe = player(8, "red", 2.8, 0.);
    l.vehicles[0].cargo = Cargo {
        wood: 30,
        metal: 18,
        oil: 20,
        fuel: 15,
        powder: 7,
    };
    foe.oil = 8;
    let enemy_stock = cargo_totals(&foe, &l.vehicles[0]);
    action(&mut l, &mut foe, &[], "cargo");
    action(&mut l, &mut foe, &[], "vehicle");
    assert_eq!(foe.oil, 8);
    assert_eq!(cargo_totals(&foe, &l.vehicles[0]), enemy_stock);
    assert_eq!(l.driver_vehicle(foe.id), None);

    ally.oil = 3;
    ally.fuel = 2;
    ally.wood = 4;
    ally.powder = 1;
    let stock = cargo_totals(&ally, &l.vehicles[0]);
    action(&mut l, &mut ally, &[], "cargo");
    assert_eq!(l.vehicles[0].cargo.total(), 96);
    assert_eq!(bag(&ally), 4);
    assert_eq!(cargo_totals(&ally, &l.vehicles[0]), stock);
    action(&mut l, &mut ally, &[], "cargo");
    assert_eq!(
        bag(&ally),
        4,
        "a full cargo bay did not accept the remainder"
    );

    let mut receiver = player(9, "blue", 2.8, 0.);
    let contents = cargo_totals(&receiver, &l.vehicles[0]);
    action(&mut l, &mut receiver, &[], "cargo");
    assert_eq!(bag(&receiver), 96);
    assert_eq!(l.vehicles[0].cargo.total(), 0);
    assert_eq!(cargo_totals(&receiver, &l.vehicles[0]), contents);
}

#[test]
fn kits_are_exclusive_target_specific_and_cost_only_on_success() {
    let mut l = pool("cargo6x6");
    let mut p = player(1, "blue", 2.8, 0.);
    let shop = [prop("workshop", "blue", 6.5, 0., 2., 2.)];
    let enemy_shop = [prop("workshop", "red", 6.5, 0., 2., 2.)];
    p.wood = 10;
    p.metal = 20;
    action(&mut l, &mut p, &enemy_shop, "upgrade");
    assert_eq!((p.wood, p.metal), (10, 20));
    action(&mut l, &mut p, &shop, "upgradeSelect");
    action(&mut l, &mut p, &shop, "upgrade");
    assert_eq!(
        (l.vehicles[0].upgrade_level, l.vehicles[0].cargo_capacity),
        (1, 128)
    );
    action(&mut l, &mut p, &shop, "upgrade");
    assert_eq!(
        (l.vehicles[0].upgrade_level, l.vehicles[0].cargo_capacity),
        (2, 160)
    );
    assert_eq!((p.wood, p.metal), (4, 8));
    action(&mut l, &mut p, &shop, "upgrade");
    assert_eq!(
        (p.wood, p.metal),
        (4, 8),
        "a capped kit cannot charge again"
    );

    l.vehicles[0].cargo.wood = 100;
    action(&mut l, &mut p, &shop, "upgradeSelect");
    action(&mut l, &mut p, &shop, "upgrade");
    assert_eq!(l.vehicles[0].upgrade.as_deref(), Some("utility"));
    assert_eq!(l.vehicles[0].cargo.total(), 100);
    assert_eq!(
        (p.wood, p.metal),
        (4, 8),
        "downsize cannot consume freight or supplies"
    );

    l.vehicles[0].cargo.wood = 95;
    action(&mut l, &mut p, &shop, "upgrade");
    assert_eq!(l.vehicles[0].upgrade.as_deref(), Some("mobility"));
    assert_eq!(l.vehicles[0].kit("utility"), 0.);
    assert_eq!(
        (l.vehicles[0].upgrade_level, l.vehicles[0].cargo_capacity),
        (1, 96)
    );
    assert_eq!((p.wood, p.metal), (2, 4));
    action(&mut l, &mut p, &shop, "upgrade");
    assert_eq!(
        (p.wood, p.metal),
        (2, 4),
        "unaffordable second level must be atomic"
    );
    assert_eq!(l.vehicles[0].upgrade_level, 1);
}

#[test]
fn engine_and_armour_have_observable_speed_fuel_and_health_tradeoffs() {
    let shop = [prop("workshop", "blue", 6.5, 0., 2., 2.)];
    fn compare(utility: Option<bool>, shop: &[Prop]) -> (f32, f32, f32) {
        let mut l = pool("mbt");
        let mut p = player(1, "blue", 2.8, 0.);
        p.wood = 6;
        p.metal = 12;
        if let Some(utility) = utility {
            if utility {
                action(&mut l, &mut p, shop, "upgradeSelect");
            }
            for _ in 0..2 {
                action(&mut l, &mut p, shop, "upgrade");
            }
        }
        action(&mut l, &mut p, shop, "vehicle");
        assert_eq!(l.driver_vehicle(p.id), Some(0));
        let fuel = l.vehicles[0].fuel;
        for _ in 0..140 {
            l.update(&mut p, &json!({"forward":1}), 0.05, shop, &bounds());
        }
        let v = &l.vehicles[0];
        (v.speed, (fuel - v.fuel) / v.z.abs(), v.max_hp)
    }
    let base = compare(None, &shop);
    let mobility = compare(Some(false), &shop);
    let armour = compare(Some(true), &shop);
    assert!(mobility.0 > base.0 && base.0 > armour.0);
    assert!(mobility.1 > base.1 && (base.1 - armour.1).abs() < 0.001);
    assert!(armour.2 > base.2 && mobility.2 == base.2);
}

#[test]
fn refinery_delivers_to_same_team_after_the_timer_and_loses_orders_when_destroyed() {
    let mut l = Logistics::default();
    let mut site = vec![prop("refinery", "blue", 0., 0., 4., 4.)];
    let mut worker = player(1, "blue", 2.8, 0.);
    let mut thief = player(2, "red", 2.8, 0.);
    worker.oil = 8;
    thief.oil = 8;
    action(&mut l, &mut thief, &site, "interact");
    assert!(l.runs.is_empty());
    action(&mut l, &mut worker, &site, "interact");
    assert_eq!((worker.oil, l.runs[&0].pending), (4, 6));
    action(&mut l, &mut worker, &site, "interact");
    assert_eq!(
        worker.oil, 4,
        "a second click cannot commission a duplicate batch"
    );
    l.tick(11.7, &site, &BTreeMap::new());
    assert_eq!(l.runs[&0].ready, 0);
    // End just inside the snap tolerance at the real server tick rate.
    l.runs.get_mut(&0).unwrap().remaining = 1. / 60. + 0.00002;
    l.tick(1. / 60., &site, &BTreeMap::new());
    assert_eq!((l.runs[&0].ready, l.runs[&0].pending), (6, 0));
    let mut snapshot = json!({"players":[]});
    l.write_snapshot(&mut snapshot);
    assert_eq!(snapshot["factories"][0]["remaining"], 0.0);

    action(&mut l, &mut thief, &site, "interact");
    assert_eq!((thief.fuel, l.runs[&0].ready), (0, 6));
    worker.metal = 27;
    worker.fuel = 31; // Only one fuel slot remains, independent of metal and oil.
    action(&mut l, &mut worker, &site, "interact");
    assert_eq!((worker.fuel, l.runs[&0].ready), (32, 5));
    assert_eq!(bag(&worker), 63);
    action(&mut l, &mut worker, &site, "interact");
    assert_eq!(l.runs[&0].ready, 5);
    let mut mate = player(3, "blue", 2.8, 0.);
    action(&mut l, &mut mate, &site, "interact");
    assert_eq!((mate.fuel, l.runs[&0].ready), (5, 0));

    action(&mut l, &mut worker, &site, "interact");
    assert_eq!((worker.oil, l.runs[&0].pending), (0, 6));
    site[0].dead = true;
    l.tick(13., &site, &BTreeMap::new());
    assert!(l.runs.is_empty());
    let mut snapshot = json!({"players":[]});
    l.write_snapshot(&mut snapshot);
    assert_eq!(snapshot["factories"], json!([]));
}

#[test]
fn destroyed_vehicles_eject_driver_yield_finite_scrap_and_wait_for_a_free_pad() {
    let mut l = Logistics::default();
    l.reset(&[spawn("patrol4x4", 0.), spawn("cargo6x6", 24.)]);
    let mut p = player(3, "blue", 2.2, 0.);
    action(&mut l, &mut p, &[], "vehicle");
    assert_eq!(l.driver_vehicle(p.id), Some(0));
    for _ in 0..55 {
        l.update(&mut p, &json!({"forward":1}), 0.05, &[], &bounds());
    }
    let wreck_z = l.vehicles[0].z;
    assert!(wreck_z < -8.);
    let hp = l.vehicles[0].hp;
    assert!(l.hit(0, 999., p.id, "blue").is_empty());
    assert_eq!(l.vehicles[0].hp, hp);
    assert_eq!(l.hit(0, 999., 11, "red")[0]["type"], "destroy");
    assert!(l.hit(0, 999., 11, "red").is_empty());
    assert!(l.driver_vehicle(p.id).is_none());
    l.update(&mut p, &json!({}), 0.05, &[], &bounds());
    assert!(!l.vehicles[0].shape().intersects(p.x, p.z, 0.36));
    let mut salvager = player(6, "red", 2.1, wreck_z);
    salvager.wood = 31;
    salvager.metal = 31;
    action(&mut l, &mut salvager, &[], "interact");
    assert_eq!((salvager.metal, l.vehicles[0].salvage), (32, 5));
    action(&mut l, &mut salvager, &[], "interact");
    assert_eq!(
        l.vehicles[0].salvage, 5,
        "a full backpack leaves the rest for other players"
    );
    let mut teammate = player(9, "red", 2.4, wreck_z);
    for _ in 0..4 {
        action(&mut l, &mut teammate, &[], "interact");
    }
    assert_eq!(
        (salvager.metal - 31 + teammate.metal, l.vehicles[0].salvage),
        (6, 0)
    );

    // A parked truck takes the vacant pad before the scout car's timer expires.
    l.vehicles[1].x = 0.;
    let duration = spec("patrol4x4").respawn;
    l.tick(duration - 2., &[], &BTreeMap::new());
    assert!(l.vehicles[0].destroyed);
    l.tick(2., &[], &BTreeMap::new());
    assert!(
        l.vehicles[0].destroyed,
        "a respawn overlapped a parked vehicle"
    );
    l.vehicles[1].x = 24.;
    let barrier = prop("wall", "blue", 0., 0., 3.1, 0.75);
    l.tick(0.2, &[barrier], &BTreeMap::new());
    assert!(
        l.vehicles[0].destroyed,
        "a respawn overlapped a newly built wall"
    );
    let waiting = player(16, "blue", 0., 0.);
    l.tick(0.2, &[], &BTreeMap::from([(waiting.id, waiting)]));
    assert!(l.vehicles[0].destroyed, "a respawn overlapped infantry");
    l.tick(0.2, &[], &BTreeMap::new());
    let v = &l.vehicles[0];
    assert!(!v.destroyed && v.hp == v.max_hp && v.driver.is_none());
    assert_eq!((v.id, v.x, v.z, v.upgrade_level), (1, 0., 0., 0));
}

#[test]
fn vehicles_stop_at_static_walls_and_require_fuel_to_move() {
    let mut l = pool("cargo6x6");
    let mut p = player(1, "blue", 2.7, 0.);
    let wall = [prop("wall", "blue", 0., -13., 14., 0.9)];
    action(&mut l, &mut p, &wall, "vehicle");
    let zero = l.vehicles[0].z;
    let start_fuel = l.vehicles[0].fuel;
    for _ in 0..170 {
        l.update(&mut p, &json!({"forward":1}), 0.05, &wall, &bounds());
    }
    let z = l.vehicles[0].z;
    assert!(z < zero - 3. && z > -11.);
    let truck_length = spec("cargo6x6").d;
    assert!(z - truck_length * 0.5 >= wall[0].z + wall[0].d * 0.5);
    assert!(l.vehicles[0].fuel < start_fuel);
    l.vehicles[0].fuel = 0.;
    for _ in 0..70 {
        l.update(&mut p, &json!({"forward":-1}), 0.05, &wall, &bounds());
    }
    assert_eq!(l.vehicles[0].z, z);
    p.fuel = 7;
    action(&mut l, &mut p, &wall, "service");
    assert_eq!((p.fuel, l.vehicles[0].fuel), (0, 7.));
    for _ in 0..20 {
        l.update(&mut p, &json!({"forward":-1}), 0.05, &wall, &bounds());
    }
    assert!(l.vehicles[0].z > z + 1.);
    assert!(l.vehicles[0].fuel < 7.);
}

#[test]
fn workshop_only_spends_game_tokens_when_it_can_issue_ammunition() {
    let mut logistics = pool("mbt");
    let mut driver = player(1, "blue", 2.8, 0.);
    let friendly = [prop("workshop", "blue", 6.5, 0., 2., 2.)];
    let hostile = [prop("workshop", "red", 6.5, 0., 2., 2.)];
    action(&mut logistics, &mut driver, &friendly, "vehicle");
    driver.metal = 2;
    driver.powder = 4;
    action(&mut logistics, &mut driver, &hostile, "ammoCraft");
    assert_eq!(logistics.vehicles[0].shells, 8);
    assert_eq!((driver.metal, driver.powder), (2, 4));
    action(&mut logistics, &mut driver, &friendly, "ammoCraft");
    assert_eq!(
        (logistics.vehicles[0].shells, driver.metal, driver.powder),
        (11, 1, 2)
    );
    action(&mut logistics, &mut driver, &friendly, "ammoCraft");
    assert_eq!(
        (logistics.vehicles[0].shells, driver.metal, driver.powder),
        (12, 0, 0)
    );
    driver.metal = 1;
    driver.powder = 2;
    action(&mut logistics, &mut driver, &friendly, "ammoCraft");
    assert_eq!(
        (logistics.vehicles[0].shells, driver.metal, driver.powder),
        (12, 1, 2)
    );

    let mut foot = player(2, "blue", 8.5, 0.);
    foot.powder = 1;
    foot.metal = 2;
    action(&mut logistics, &mut foot, &friendly, "ammoCraft");
    assert_eq!((foot.reserve, foot.metal, foot.powder), (0, 2, 1));
    foot.powder = 2;
    action(&mut logistics, &mut foot, &friendly, "ammoCraft");
    assert_eq!((foot.reserve, foot.metal, foot.powder), (24, 1, 0));
}
