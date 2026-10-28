#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(not(windows))]
compile_error!("OpenNetworkMonitor currently supports Windows only.");

use anyhow::{Context, Result};
use open_network_monitor::{app, capture, model, store, tray, widgets};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream, UdpSocket},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn main() {
    if let Err(error) = run() {
        let text = format!("OpenNetworkMonitor: {error:#}");
        eprintln!("{text}");
        if !std::env::args().any(|a| a == "--diagnose" || a == "--self-test-capture") {
            let _ = rfd::MessageDialog::new()
                .set_title("OpenNetworkMonitor")
                .set_description(text)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help") {
        println!(
            "OpenNetworkMonitor\n\nRun normally for the desktop app.\n  --diagnose [seconds]     Read-only capture diagnostics (default 8)\n  --self-test-capture      Verify own-process TCP/UDP IPv4/IPv6 capture\n\nRun as administrator for detailed app traffic. No driver installation required."
        );
        return Ok(());
    }
    if args
        .iter()
        .any(|a| a == "--diagnose" || a == "--self-test-capture")
    {
        return diagnose(&args);
    }
    let _instance = capture::InstanceGuard::acquire()?;
    let data_dir =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?)
            .join("OpenNetworkMonitor");
    std::fs::create_dir_all(&data_dir)?;
    let store = store::Store::open(&data_dir.join("usage.db"))?;
    let icon = eframe::egui::IconData {
        rgba: tray::icon_pixels(),
        width: 32,
        height: 32,
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("OpenNetworkMonitor")
            .with_inner_size(widgets::DEFAULT_WINDOW_SIZE)
            .with_min_inner_size(widgets::MIN_WINDOW_SIZE)
            .with_clamp_size_to_monitor_size(true)
            .with_icon(icon),
        persistence_path: Some(data_dir.join("preferences.ron")),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "OpenNetworkMonitor",
        options,
        Box::new(move |cc| Ok(Box::new(app::MonitorApp::new(cc, store, data_dir)?))),
    )
    .map_err(|e| anyhow::anyhow!("Could not start the interface: {e}"))?;
    Ok(())
}
fn diagnose(args: &[String]) -> Result<()> {
    let self_test = args.iter().any(|a| a == "--self-test-capture");
    let seconds = args
        .iter()
        .position(|a| a == "--diagnose")
        .and_then(|i| args.get(i + 1))
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(8)
        .clamp(3, 60);
    let adapters = capture::adapters()?;
    let names = capture::process_names()?;
    let connections = capture::connections(&names)?;
    let started = Instant::now();
    let capture = capture::EtwCapture::start();
    let mut expected_ports = TestPorts::new();
    if self_test && capture.is_ok() {
        thread::sleep(Duration::from_millis(300));
        expected_ports = generate_traffic()?;
    } else {
        thread::sleep(Duration::from_secs(seconds));
    }
    // Self-test stops immediately after delivery to exercise final-buffer flushing.
    let mut protocols = std::collections::BTreeMap::<String, (u64, u64, u64)>::new();
    let mut own_bytes = std::collections::BTreeMap::<String, (u64, u64)>::new();
    let mut invalid_own_metadata = 0;
    let mut invalid_own_ports = 0;
    let mut own = 0;
    let mut errors = 0;
    let mut drops = 0;
    let mut losses = 0;
    let mut event_count = 0;
    let mut self_test_protocols = std::collections::BTreeSet::<String>::new();
    let capture_error = match capture {
        Ok(mut c) => {
            losses = c.health().unwrap_or(0);
            c.stop();
            errors = c.stats.errors.load(std::sync::atomic::Ordering::Relaxed);
            drops = c.stats.drops.load(std::sync::atomic::Ordering::Relaxed);
            for event in c.events.try_iter() {
                event_count += 1;
                let key = format!(
                    "{}{}",
                    event.protocol,
                    if event.remote.is_ipv4() { "4" } else { "6" }
                );
                if event.pid == std::process::id() {
                    own += 1;
                    self_test_protocols.insert(key.clone());
                    if self_test
                        && !expected_ports.get(&key).is_some_and(|ports| {
                            ports.contains(&(event.local_port, event.remote_port))
                        })
                    {
                        invalid_own_ports += 1;
                    }
                    let bytes = own_bytes.entry(key.clone()).or_default();
                    bytes.0 += event.down;
                    bytes.1 += event.up;
                    if model::classify(event.remote, &[]) != model::Scope::Lan
                        || (event.timestamp - chrono::Utc::now().timestamp()).abs() > 120
                    {
                        invalid_own_metadata += 1;
                    }
                }
                let count = protocols.entry(key).or_default();
                count.0 += 1;
                count.1 += event.down;
                count.2 += event.up;
            }
            None
        }
        Err(e) => Some(e.to_string()),
    };
    let after = capture::adapters()?;
    let adapter_delta: u64 = after
        .iter()
        .filter_map(|a| {
            adapters.iter().find(|b| a.id == b.id).map(|b| {
                model::counter_delta(b.received, a.received) + model::counter_delta(b.sent, a.sent)
            })
        })
        .sum();
    let output = serde_json::json!({"adapters":adapters.iter().map(|a|serde_json::json!({"name":a.name,"up":a.up,"physical":a.physical,"addresses":a.addresses.iter().map(|(ip,p)|format!("{ip}/{p}")).collect::<Vec<_>>()})).collect::<Vec<_>>(),"connections":connections.len(),"processes":names.len(),"capture_error":capture_error,"observed_events":event_count,"own_process_events":own,"protocols":protocols,"own_protocols":self_test_protocols,"own_bytes":own_bytes,"invalid_own_metadata":invalid_own_metadata,"invalid_own_ports":invalid_own_ports,"decode_errors":errors,"queue_drops":drops,"kernel_losses":losses,"raw_adapter_byte_delta":adapter_delta,"elapsed_seconds":started.elapsed().as_secs_f64()});
    let json = serde_json::to_string_pretty(&output)?;
    println!("{json}");
    if let Some(path) = args
        .iter()
        .position(|a| a == "--report")
        .and_then(|i| args.get(i + 1))
    {
        std::fs::write(path, &json).context("Write diagnostics report")?;
    }
    if self_test {
        anyhow::ensure!(
            capture_error.is_none(),
            "Kernel capture unavailable; run the test as administrator"
        );
        anyhow::ensure!(
            ["TCP4", "TCP6", "UDP4", "UDP6"]
                .iter()
                .all(|p| self_test_protocols.contains(*p)),
            "Not all generated protocols were attributed to this process"
        );
        anyhow::ensure!(
            errors == 0
                && drops == 0
                && losses == 0
                && invalid_own_metadata == 0
                && invalid_own_ports == 0
                && own_bytes.iter().all(|(protocol, (down, up))| {
                    let expected = if protocol.starts_with("TCP") {
                        4 * 1024 * 1024
                    } else {
                        16 * 1024
                    };
                    *down == expected && *up == expected
                }),
            "Capture reported decode errors, event loss, incorrect timestamps/classification/ports, or mismatched payload byte counts"
        );
    }
    Ok(())
}
type TestPorts = std::collections::BTreeMap<String, std::collections::HashSet<(u16, u16)>>;
fn generate_traffic() -> Result<TestPorts> {
    let mut ports = TestPorts::new();
    for address in ["127.0.0.1:0", "[::1]:0"] {
        let server = TcpListener::bind(address)?;
        let addr = server.local_addr()?;
        let reader = thread::spawn(move || -> std::io::Result<u64> {
            let (mut stream, _) = server.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut total = 0;
            let mut buffer = [0; 65536];
            loop {
                let read = stream.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                total += read as u64;
            }
            Ok(total)
        });
        let mut stream = TcpStream::connect(addr)?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let local_port = stream.local_addr()?.port();
        let remote_port = addr.port();
        let family = if addr.is_ipv4() { "4" } else { "6" };
        ports.insert(
            format!("TCP{family}"),
            [(local_port, remote_port), (remote_port, local_port)].into(),
        );
        for _ in 0..64 {
            stream.write_all(&[42; 65536])?;
        }
        drop(stream);
        anyhow::ensure!(
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("TCP test reader stopped"))??
                == 4 * 1024 * 1024,
            "TCP payload verification failed"
        );
        let receiver = UdpSocket::bind(address)?;
        receiver.set_read_timeout(Some(Duration::from_secs(2)))?;
        let sender = UdpSocket::bind(address)?;
        let local_port = sender.local_addr()?.port();
        let remote_port = receiver.local_addr()?.port();
        ports.insert(
            format!("UDP{family}"),
            [(local_port, remote_port), (remote_port, local_port)].into(),
        );
        for _ in 0..16 {
            sender.send_to(&[42; 1024], receiver.local_addr()?)?;
            let mut buf = [0; 2048];
            anyhow::ensure!(
                receiver.recv(&mut buf)? == 1024,
                "UDP payload verification failed"
            );
        }
    }
    Ok(ports)
}
