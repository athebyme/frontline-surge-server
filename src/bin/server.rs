//! Foreground dedicated server. Console input is optional, so service managers
//! may close stdin and stop the process with their normal termination signal.
use frontline_surge::{server, PROTOCOL};
use std::env;
use std::io::{self, BufRead, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

static STOP: AtomicBool = AtomicBool::new(false);
const HELP: &str = "Frontline Surge dedicated server

Usage: frontline-server [--bind IP] [--port PORT] [--bots COUNT] [--round-minutes MIN] [--dev-local]

  --bind IP       Listen address. Default: 0.0.0.0 (all IPv4 interfaces).
  --port PORT     TCP port. Default: 8787. Use 0 to request an ephemeral port.
  --bots COUNT    Add 0–10 computer players when the first player joins.
                  Default: 0. Bots share the 12 slots with human players.
  --round-minutes MIN  Round length from 120 to 180 minutes, default 120.
  --dev-local     Enable developer menu for peers connecting via loopback on this host.
                  Disabled by default; LAN and Internet peers get normal player permissions.
  --version       Print server and protocol versions.
  --help          Display this help.

Examples:
  frontline-server --bind 0.0.0.0 --port 8787
  frontline-server --bind 127.0.0.1 --bots 5

Stop with Ctrl+C, SIGTERM or the 'quit' console command.";

struct Options {
    bind: IpAddr,
    port: u16,
    bots: usize,
    round_minutes: u32,
    dev_local: bool,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options {
        bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        port: 8787,
        bots: 0,
        round_minutes: 120,
        dev_local: false,
    };
    while let Some(arg) = args.next() {
        let (option, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        if option == "--help" || option == "-h" {
            println!("{HELP}");
            return Ok(None);
        }
        if option == "--version" || option == "-V" {
            println!(
                "Frontline Surge {} · protocol {PROTOCOL}",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(None);
        }
        if option == "--dev-local" && inline.is_none() {
            options.dev_local = true;
            continue;
        }
        if !matches!(option, "--bind" | "--port" | "--bots" | "--round-minutes") {
            return Err(format!("Unknown option {option}. See --help."));
        }
        let value = inline
            .map(str::to_owned)
            .or_else(|| args.next())
            .ok_or_else(|| format!("{option} requires a value."))?;
        match option {
            "--bind" => {
                options.bind = value
                    .parse()
                    .map_err(|_| format!("Invalid listen IP: {value}"))?
            }
            "--port" => {
                options.port = value
                    .parse()
                    .map_err(|_| format!("Invalid TCP port: {value}"))?
            }
            "--round-minutes" => {
                options.round_minutes = value
                    .parse()
                    .map_err(|_| format!("Invalid round length: {value}"))?;
                if !(120..=180).contains(&options.round_minutes) {
                    return Err("--round-minutes must be between 120 and 180.".into());
                }
            }
            "--bots" => {
                options.bots = value
                    .parse()
                    .map_err(|_| format!("Invalid bot count: {value}"))?;
                if options.bots > 10 {
                    return Err("--bots must be between 0 and 10.".into());
                }
            }
            _ => unreachable!(),
        }
    }
    Ok(Some(options))
}

#[cfg(unix)]
fn signals() -> io::Result<()> {
    extern "C" fn stop(_: i32) {
        STOP.store(true, Ordering::Release);
    }
    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }
    for sig in [2, 15] {
        // POSIX SIGINT and SIGTERM.
        if unsafe { signal(sig, stop) } == usize::MAX {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
#[cfg(windows)]
fn signals() -> io::Result<()> {
    unsafe extern "system" fn stop(kind: u32) -> i32 {
        match kind {
            0 | 1 | 2 | 5 | 6 => {
                STOP.store(true, Ordering::Release);
                1
            }
            _ => 0,
        }
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleCtrlHandler(handler: unsafe extern "system" fn(u32) -> i32, add: i32) -> i32;
    }
    if unsafe { SetConsoleCtrlHandler(stop, 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn serve(options: Options) -> io::Result<()> {
    let socket = TcpListener::bind(SocketAddr::new(options.bind, options.port))?;
    let address = socket.local_addr()?;
    signals()?;
    println!(
        "Frontline Surge {} | dedicated server | protocol {PROTOCOL}",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "Listening on {address} (TCP), {} human slots and {} bots.",
        server::PLAYER_LIMIT - options.bots,
        options.bots
    );
    if options.dev_local {
        println!("Developer menu enabled for loopback connections (127.0.0.1 / ::1).");
    }
    if address.ip().is_unspecified() {
        let local = if address.is_ipv4() {
            format!("127.0.0.1:{}", address.port())
        } else {
            format!("[::1]:{}", address.port())
        };
        println!("Join on this computer: {local}");
        if let Some(lan) = address
            .is_ipv4()
            .then(|| server::lan_endpoint(address.port()))
            .flatten()
        {
            println!("LAN address from local routing: {lan}");
        } else {
            println!(
                "LAN players: use this computer's network interface IP and port {}.",
                address.port()
            );
        }
        println!("Internet players connect to the host's public IP and TCP port {}. Allow this port on the VPS or forward it on the router.", address.port());
    } else {
        println!("Client address: {address}");
    }
    println!(
        "Round length: {} minutes. Type quit or press Ctrl+C to stop.",
        options.round_minutes
    );
    io::stdout().flush()?;
    thread::Builder::new()
        .name("server-console".into())
        .spawn(|| {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                if matches!(
                    line.trim().to_ascii_lowercase().as_str(),
                    "quit" | "exit" | "stop"
                ) {
                    STOP.store(true, Ordering::Release);
                    break;
                }
            }
        })?;
    server::run(
        socket,
        &STOP,
        options.bots,
        options.round_minutes * 60,
        options.dev_local,
    )?;
    println!("Server stopped.");
    Ok(())
}
fn main() -> ExitCode {
    match parse_args(env::args().skip(1)) {
        Ok(Some(opts)) => match serve(opts) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Server error: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(None) => ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("{reason}");
            ExitCode::from(2)
        }
    }
}
