#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(not(windows))]
compile_error!("OpenNetworkMonitor currently supports Windows only.");

use anyhow::{Context, Result};
use open_network_monitor::{app, capture, model, store, tray, widgets};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn main() {
    if let Err(error) = run() {
        let text = format!("OpenNetworkMonitor: {error:#}");
        eprintln!("{text}");
        if !std::env::args()
            .any(|a| a == "--diagnose" || a == "--self-test-capture" || a == "--self-test-public")
        {
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
            "OpenNetworkMonitor\n\nRun normally for the desktop app.\n  --diagnose [seconds]     Read-only capture diagnostics (default 8)\n  --self-test-capture      Verify local TCP/UDP IPv4/IPv6 capture\n  --self-test-public       Verify a small public IPv4 TCP transfer\n\nRun as administrator for detailed app traffic. No driver installation required."
        );
        return Ok(());
    }
    if args
        .iter()
        .any(|a| a == "--diagnose" || a == "--self-test-capture" || a == "--self-test-public")
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
    let loopback_test = args.iter().any(|a| a == "--self-test-capture");
    let public_test = args.iter().any(|a| a == "--self-test-public");
    anyhow::ensure!(
        !(loopback_test && public_test),
        "Select one capture self-test at a time"
    );
    let self_test = loopback_test || public_test;
    let seconds = args
        .iter()
        .position(|a| a == "--diagnose")
        .and_then(|i| args.get(i + 1))
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(8)
        .clamp(3, 60);
    let adapters = capture::adapters()?;
    let all_adapters = if public_test {
        capture::all_adapters()?
    } else {
        Vec::new()
    };
    let names = capture::process_names()?;
    let connections = capture::connections(&names)?;
    let started = Instant::now();
    let capture = capture::EtwCapture::start();
    let mut expected_endpoints = TestEndpoints::new();
    let mut expected_public_bytes = None;
    if self_test && capture.is_ok() {
        thread::sleep(Duration::from_millis(300));
        if public_test {
            let (flow, down, up) = generate_public_traffic()?;
            expected_endpoints.insert("TCP4".into(), flow);
            expected_public_bytes = Some((down, up));
        } else {
            expected_endpoints = generate_traffic()?;
        }
    } else if !self_test {
        thread::sleep(Duration::from_secs(seconds));
    }
    // Self-test stops immediately after delivery to exercise final-buffer flushing.
    let mut protocols = std::collections::BTreeMap::<String, (u64, u64, u64)>::new();
    let mut own_bytes = std::collections::BTreeMap::<String, (u64, u64)>::new();
    let mut invalid_own_metadata = 0;
    let mut invalid_own_ports = 0;
    let mut invalid_own_addresses = 0;
    let mut invalid_own_adapter = 0u64;
    let mut endpoint_mismatch_counts =
        std::collections::BTreeMap::<String, TestMismatchCounts>::new();
    let mut endpoint_mismatch_examples =
        std::collections::BTreeMap::<String, Vec<serde_json::Value>>::new();
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
                    if self_test {
                        let expected = expected_endpoints.get(&key).map(|endpoints| {
                            // On loopback, both TCP sockets have the same process ID. Windows
                            // may associate send events with either socket, so accept a fully
                            // reversed pair only there. Receive and UDP remain strict.
                            let actual = (
                                SocketAddr::new(event.local, event.local_port),
                                SocketAddr::new(event.remote, event.remote_port),
                            );
                            if event.down > 0
                                || (loopback_test
                                    && event.protocol == "TCP"
                                    && actual == endpoints.download)
                            {
                                endpoints.download
                            } else {
                                endpoints.upload
                            }
                        });
                        let invalid_ports = !expected.is_some_and(|(local, remote)| {
                            local.port() == event.local_port && remote.port() == event.remote_port
                        });
                        let invalid_addresses = !expected.is_some_and(|(local, remote)| {
                            model::normalize_ip(local.ip()) == model::normalize_ip(event.local)
                                && model::normalize_ip(remote.ip())
                                    == model::normalize_ip(event.remote)
                        });
                        invalid_own_ports += u64::from(invalid_ports);
                        invalid_own_addresses += u64::from(invalid_addresses);
                        if invalid_ports || invalid_addresses {
                            let group = format!(
                                "{key} {}",
                                if event.down > 0 { "receive" } else { "send" }
                            );
                            let counts = endpoint_mismatch_counts.entry(group.clone()).or_default();
                            counts.ports += u64::from(invalid_ports);
                            counts.addresses += u64::from(invalid_addresses);
                            let examples = endpoint_mismatch_examples.entry(group).or_default();
                            if examples.len() < 2 {
                                if public_test {
                                    examples.push(serde_json::json!({
                                        "actual_is_reversed": expected.is_some_and(|(local, remote)| {
                                            SocketAddr::new(event.local, event.local_port) == remote
                                                && SocketAddr::new(event.remote, event.remote_port) == local
                                        }),
                                        "ports_mismatch": invalid_ports,
                                        "addresses_mismatch": invalid_addresses,
                                    }));
                                } else {
                                    examples.push(serde_json::json!({
                                        "actual_local": SocketAddr::new(event.local, event.local_port).to_string(),
                                        "actual_remote": SocketAddr::new(event.remote, event.remote_port).to_string(),
                                        "expected_local": expected.map(|(local, _)| local.to_string()),
                                        "expected_remote": expected.map(|(_, remote)| remote.to_string()),
                                    }));
                                }
                            }
                        }
                    }
                    let bytes = own_bytes.entry(key.clone()).or_default();
                    bytes.0 += event.down;
                    bytes.1 += event.up;
                    let expected_scope = if public_test {
                        model::Scope::Wan
                    } else {
                        model::Scope::Lan
                    };
                    let adapter = if public_test {
                        let adapter = model::adapter_for(event.local, &all_adapters);
                        invalid_own_adapter += u64::from(adapter.is_none());
                        adapter
                    } else {
                        None
                    };
                    let local_networks = adapter
                        .map(|adapter| adapter.addresses.as_slice())
                        .unwrap_or(&[]);
                    if model::classify(event.remote, local_networks) != expected_scope
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
    let output = serde_json::json!({"adapters":if public_test { Vec::new() } else { adapters.iter().map(|a|serde_json::json!({"name":a.name,"up":a.up,"physical":a.physical,"addresses":a.addresses.iter().map(|(ip,p)|format!("{ip}/{p}")).collect::<Vec<_>>()})).collect::<Vec<_>>() },"connections":connections.len(),"processes":names.len(),"capture_error":capture_error,"observed_events":event_count,"own_process_events":own,"protocols":protocols,"own_protocols":self_test_protocols,"own_bytes":own_bytes,"invalid_own_metadata":invalid_own_metadata,"invalid_own_ports":invalid_own_ports,"invalid_own_addresses":invalid_own_addresses,"invalid_own_adapter":invalid_own_adapter,"endpoint_mismatch_counts":endpoint_mismatch_counts,"endpoint_mismatch_examples":endpoint_mismatch_examples,"decode_errors":errors,"queue_drops":drops,"kernel_losses":losses,"raw_adapter_byte_delta":adapter_delta,"elapsed_seconds":started.elapsed().as_secs_f64()});
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
            if public_test {
                self_test_protocols.contains("TCP4")
            } else {
                ["TCP4", "TCP6", "UDP4", "UDP6"]
                    .iter()
                    .all(|p| self_test_protocols.contains(*p))
            },
            "Not all generated protocols were attributed to this process"
        );
        anyhow::ensure!(
            errors == 0
                && drops == 0
                && losses == 0
                && invalid_own_metadata == 0
                && invalid_own_ports == 0
                && invalid_own_addresses == 0
                && invalid_own_adapter == 0
                && if public_test {
                    expected_public_bytes.is_some_and(|(down, up)| {
                        down > 0 && up > 0 && own_bytes.get("TCP4") == Some(&(down, up))
                    })
                } else {
                    own_bytes.iter().all(|(protocol, (down, up))| {
                        let expected = if protocol.starts_with("TCP") {
                            4 * 1024 * 1024
                        } else {
                            16 * 1024
                        };
                        *down == expected && *up == expected
                    })
                },
            "Capture reported decode errors, event loss, incorrect timestamps/classification/endpoints, or mismatched payload byte counts"
        );
    }
    Ok(())
}
#[derive(Default, serde::Serialize)]
struct TestMismatchCounts {
    ports: u64,
    addresses: u64,
}

#[derive(Clone, Copy)]
struct TestFlow {
    upload: (SocketAddr, SocketAddr),
    download: (SocketAddr, SocketAddr),
}
type TestEndpoints = std::collections::BTreeMap<String, TestFlow>;
/// Explicit opt-in probe: send only a fixed HTTP request to a public IP and
/// read at most 64 KiB. No DNS, user data, cookies, or external programs.
fn generate_public_traffic() -> Result<(TestFlow, u64, u64)> {
    let peer: SocketAddr = "1.1.1.1:80".parse()?;
    let mut stream = TcpStream::connect_timeout(&peer, Duration::from_secs(5))
        .context("Connect public capture test")?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let local = stream.local_addr()?;
    let request = b"GET / HTTP/1.0\r\nHost: 1.1.1.1\r\nConnection: close\r\n\r\n";
    stream.write_all(request)?;
    let mut received = Vec::new();
    let mut buffer = [0u8; 8192];
    let deadline = Instant::now() + Duration::from_secs(5);
    while received.len() < 64 * 1024 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(
            !remaining.is_zero(),
            "Public capture test response timed out"
        );
        stream.set_read_timeout(Some(remaining))?;
        let limit = buffer.len().min(64 * 1024 - received.len());
        let count = stream
            .read(&mut buffer[..limit])
            .context("Read public capture test response")?;
        if count == 0 {
            break;
        }
        received.extend_from_slice(&buffer[..count]);
    }
    anyhow::ensure!(
        !received.is_empty(),
        "Public capture test received no response"
    );
    Ok((
        TestFlow {
            upload: (local, peer),
            download: (local, peer),
        },
        received.len() as u64,
        request.len() as u64,
    ))
}
fn generate_traffic() -> Result<TestEndpoints> {
    let mut endpoints = TestEndpoints::new();
    // Distinct IPv4 loopback addresses reveal endpoint reversals that a
    // same-address test cannot detect. IPv6 loopback has only one address,
    // so direction-specific ports still check the local/remote association.
    for (server_bind, client_bind) in [("127.0.0.2:0", "127.0.0.1:0"), ("[::1]:0", "[::1]:0")] {
        let server = TcpListener::bind(server_bind)?;
        let server_address = server.local_addr()?;
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
        let mut stream = TcpStream::connect(server_address)?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let client_address = stream.local_addr()?;
        let family = if server_address.is_ipv4() { "4" } else { "6" };
        if server_address.is_ipv4() {
            anyhow::ensure!(
                client_address.ip() != server_address.ip(),
                "IPv4 test endpoints must use different loopback addresses"
            );
        }
        endpoints.insert(
            format!("TCP{family}"),
            TestFlow {
                upload: (client_address, server_address),
                download: (server_address, client_address),
            },
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
        let receiver = UdpSocket::bind(server_bind)?;
        receiver.set_read_timeout(Some(Duration::from_secs(2)))?;
        let sender = UdpSocket::bind(client_bind)?;
        let receiver_address = receiver.local_addr()?;
        let sender_address = sender.local_addr()?;
        endpoints.insert(
            format!("UDP{family}"),
            TestFlow {
                upload: (sender_address, receiver_address),
                download: (receiver_address, sender_address),
            },
        );
        for _ in 0..16 {
            sender.send_to(&[42; 1024], receiver_address)?;
            let mut buf = [0; 2048];
            anyhow::ensure!(
                receiver.recv(&mut buf)? == 1024,
                "UDP payload verification failed"
            );
        }
    }
    Ok(endpoints)
}
