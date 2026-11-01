//! DXO CLI - the test bench for every milestone before the UI exists.
//!
//!   dxo list                      M1  show detected controllers
//!   dxo raw                       M1  hex-dump raw input reports
//!   dxo monitor                   M1  live parsed state
//!   dxo lightbar <r> <g> <b>      M2  set lightbar colour
//!   dxo rumble <left> <right>     M2  rumble for 1 s
//!   dxo trigger <off|feedback|weapon>   M2  right trigger effect
//!   dxo run [profiles_dir]        M3+ full engine: virtual pad, HidHide, profiles
//!   dxo profile-template <file>   M5  write a default profile TOML

use hidapi::HidApi;
use dxo_core::device::{self, Kind};
use dxo_core::engine::{Engine, EngineConfig};
use dxo_core::output::{self, OutputState, Rgb, TriggerEffect};
use dxo_core::profile::Profile;
use dxo_core::state::{Buttons, PadState};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

type R = Result<(), Box<dyn std::error::Error>>;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = match args.first().map(String::as_str) {
        Some("list") => cmd_list(),
        Some("raw") => cmd_raw(),
        Some("monitor") => cmd_monitor(),
        Some("lightbar") => cmd_lightbar(&args[1..]),
        Some("rumble") => cmd_rumble(&args[1..]),
        Some("trigger") => cmd_trigger(&args[1..]),
        Some("run") => cmd_run(args.get(1).map(PathBuf::from)),
        Some("profile-template") => cmd_template(args.get(1)),
        _ => {
            eprintln!("{}", include_str!("usage.txt"));
            Ok(())
        }
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn cmd_list() -> R {
    let api = HidApi::new()?;
    let found = device::enumerate(&api, true);
    if found.is_empty() {
        println!("No controllers found.");
    }
    for f in found {
        println!("{:<14} {:04X}:{:04X} {:?}  {}", format!("{:?}", f.kind), f.vid, f.pid, f.transport, f.product);
    }
    Ok(())
}

fn cmd_raw() -> R {
    let api = HidApi::new()?;
    let dev = device::open_first(&api, true)?;
    println!("{} ({:?}) - Ctrl+C to stop", dev.info.product, dev.info.kind);
    let mut buf = [0u8; 128];
    loop {
        let n = dev.hid.read_timeout(&mut buf, 100)?;
        if n > 0 {
            let hex: Vec<String> = buf[..n.min(64)].iter().map(|b| format!("{b:02X}")).collect();
            print!("\r{}", hex.join(" "));
            std::io::stdout().flush()?;
        }
    }
}

fn button_names(b: Buttons) -> String {
    const N: [(u32, &str); 19] = [
        (Buttons::SOUTH, "X"),
        (Buttons::EAST, "O"),
        (Buttons::WEST, "[]"),
        (Buttons::NORTH, "/\\"),
        (Buttons::L1, "L1"),
        (Buttons::R1, "R1"),
        (Buttons::L2_DIGITAL, "L2"),
        (Buttons::R2_DIGITAL, "R2"),
        (Buttons::L3, "L3"),
        (Buttons::R3, "R3"),
        (Buttons::SELECT, "Create"),
        (Buttons::START, "Options"),
        (Buttons::HOME, "PS"),
        (Buttons::TOUCHPAD, "Pad"),
        (Buttons::MUTE, "Mute"),
        (Buttons::DPAD_UP, "Up"),
        (Buttons::DPAD_DOWN, "Down"),
        (Buttons::DPAD_LEFT, "Left"),
        (Buttons::DPAD_RIGHT, "Right"),
    ];
    N.iter().filter(|(bit, _)| b.has(*bit)).map(|(_, n)| *n).collect::<Vec<_>>().join(" ")
}

fn print_state(s: &PadState, rate: u32) {
    print!(
        "\rL({:+.2},{:+.2}) R({:+.2},{:+.2}) L2 {:.2} R2 {:.2} | gyro {:>6} {:>6} {:>6} | bat {:>3}% {:?} | {:>4} Hz | {:<40}",
        s.lx,
        s.ly,
        s.rx,
        s.ry,
        s.l2,
        s.r2,
        s.gyro[0],
        s.gyro[1],
        s.gyro[2],
        s.battery_percent.unwrap_or(0),
        s.battery_status,
        rate,
        button_names(s.buttons)
    );
    let _ = std::io::stdout().flush();
}

fn cmd_monitor() -> R {
    let api = HidApi::new()?;
    let mut dev = device::open_first(&api, true)?;
    println!("{} ({:?}, {:?}) - Ctrl+C to stop", dev.info.product, dev.info.kind, dev.info.transport);
    let (mut count, mut rate, mut t) = (0u32, 0u32, Instant::now());
    let mut last_print = Instant::now();
    loop {
        if let Some(s) = dev.poll(100)? {
            count += 1;
            if t.elapsed() >= Duration::from_secs(1) {
                rate = count;
                count = 0;
                t = Instant::now();
            }
            // Printing every report would bottleneck the terminal; ~30 fps is plenty.
            if last_print.elapsed() >= Duration::from_millis(33) {
                print_state(&s, rate);
                last_print = Instant::now();
            }
        }
    }
}

fn send_output(o: &OutputState) -> R {
    let api = HidApi::new()?;
    let dev = device::open_first(&api, false)?;
    let rep = output::build_report(dev.info.kind, dev.info.transport, o, 0);
    if rep.is_empty() {
        return Err(format!("output not implemented for {:?} over {:?} yet", dev.info.kind, dev.info.transport).into());
    }
    dev.write(&rep)?;
    Ok(())
}

fn parse_u8(args: &[String], i: usize) -> Result<u8, Box<dyn std::error::Error>> {
    Ok(args.get(i).ok_or("missing argument")?.parse()?)
}

fn cmd_lightbar(a: &[String]) -> R {
    let o = OutputState { lightbar: Rgb(parse_u8(a, 0)?, parse_u8(a, 1)?, parse_u8(a, 2)?), ..Default::default() };
    send_output(&o)
}

fn cmd_rumble(a: &[String]) -> R {
    let o = OutputState { rumble_left: parse_u8(a, 0)?, rumble_right: parse_u8(a, 1)?, ..Default::default() };
    send_output(&o)?;
    std::thread::sleep(Duration::from_secs(1));
    send_output(&OutputState::default())
}

fn cmd_trigger(a: &[String]) -> R {
    let effect = match a.first().map(String::as_str) {
        Some("feedback") => TriggerEffect::Feedback { start: 3, strength: 6 },
        Some("weapon") => TriggerEffect::Weapon { start: 3, end: 6, strength: 8 },
        _ => TriggerEffect::Off,
    };
    let api = HidApi::new()?;
    let dev = device::open_first(&api, false)?;
    if !matches!(dev.info.kind, Kind::DualSense | Kind::DualSenseEdge) {
        return Err("adaptive triggers need a DualSense".into());
    }
    drop(dev);
    send_output(&OutputState { trigger_right: effect, trigger_left: effect, ..Default::default() })?;
    println!("Applied {effect:?}. Press R2/L2 to feel it.");
    Ok(())
}

fn cmd_run(profiles_dir: Option<PathBuf>) -> R {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // Stop cleanly on Ctrl+C so the engine unhides the pad and unplugs the
    // virtual controller instead of leaving them behind.
    let running = Arc::new(AtomicBool::new(true));
    let r2 = running.clone();
    ctrlc::set_handler(move || r2.store(false, Ordering::SeqCst))?;

    let mut engine = Engine::start(EngineConfig { profiles_dir, include_generic: false, enable_virtual: true });
    println!("Engine running - Ctrl+C to stop");
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(33));
        let s = engine.snapshot();
        if s.connected {
            print_state(&s.mapped, s.reports_per_sec);
        } else if let Some(e) = &s.last_error {
            print!("\rwaiting for controller: {e:<60}");
            let _ = std::io::stdout().flush();
        }
    }
    println!("\nStopping...");
    engine.stop();
    Ok(())
}

fn cmd_template(path: Option<&String>) -> R {
    let path = PathBuf::from(path.map(String::as_str).unwrap_or("default.toml"));
    Profile::default().save(&path)?;
    println!("Wrote {}", path.display());
    Ok(())
}
