use crate::model::TrafficEvent;
use anyhow::{Result, anyhow};
use ferrisetw::{
    EventRecord, KernelTrace, SchemaLocator,
    parser::Parser,
    provider::{
        Provider,
        kernel_providers::{KernelProvider, TCP_IP_PROVIDER},
    },
    trace::{TraceProperties, TraceTrait, stop_trace_by_name},
};
use std::{
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    time::Duration,
};
use windows_sys::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, ControlTraceW, EVENT_TRACE_CONTROL_QUERY, EVENT_TRACE_PROPERTIES,
};

// ProcessTrace converts timestamps to UTC FILETIME because RAW_TIMESTAMP is not set.
// FerrisETW 1.2 FileTime::as_quad incorrectly repeats the high DWORD; use the intact raw value.
fn filetime_to_unix(raw: i64) -> i64 {
    raw.div_euclid(10_000_000) - 11_644_473_600
}
#[derive(Default)]
pub struct CaptureStats {
    pub parsed: AtomicU64,
    pub errors: AtomicU64,
    pub drops: AtomicU64,
}
pub struct EtwCapture {
    trace: Option<KernelTrace>,
    consumer: Option<std::thread::JoinHandle<()>>,
    pub events: Receiver<TrafficEvent>,
    pub stats: Arc<CaptureStats>,
    name: Vec<u16>,
}
fn callback(
    tx: SyncSender<TrafficEvent>,
    stats: Arc<CaptureStats>,
    protocol: &'static str,
) -> impl FnMut(&EventRecord, &SchemaLocator) + Send + Sync + 'static {
    move |record, locator| {
        // Count data transfer events only. Connect/ACK/retransmit events are not added again.
        let upload = match record.opcode() {
            10 | 26 => true,
            11 | 27 => false,
            _ => return,
        };
        let parsed = (|| {
            let schema = locator.event_schema(record).ok()?;
            let parser = Parser::create(record, &schema);
            // The event header may belong to a worker thread. PID in the payload owns the IO.
            let pid = parser.try_parse::<u32>("PID").ok()?;
            let size = parser.try_parse::<u32>("size").ok()? as u64;
            let src = parser.try_parse::<IpAddr>("saddr").ok()?;
            let dst = parser.try_parse::<IpAddr>("daddr").ok()?;
            let port = |name: &str| {
                parser
                    .try_parse::<u16>(name)
                    .map(u16::from_be)
                    .or_else(|_| {
                        parser
                            .try_parse::<u32>(name)
                            .map(|v| u16::from_be(v as u16))
                    })
                    .unwrap_or(0)
            };
            let sport = port("sport");
            let dport = port("dport");
            Some(TrafficEvent {
                local_port: if upload { sport } else { dport },
                remote_port: if upload { dport } else { sport },
                timestamp: filetime_to_unix(record.raw_timestamp()),
                pid,
                local: if upload { src } else { dst },
                remote: if upload { dst } else { src },
                protocol,
                down: if upload { 0 } else { size },
                up: if upload { size } else { 0 },
            })
        })();
        if let Some(event) = parsed {
            stats.parsed.fetch_add(1, Ordering::Relaxed);
            if tx.try_send(event).is_err() {
                stats.drops.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            stats.errors.fetch_add(1, Ordering::Relaxed);
        }
    }
}
impl EtwCapture {
    pub fn start() -> Result<Self> {
        let (tx, events) = sync_channel(65_536);
        let stats = Arc::new(CaptureStats::default());
        let tcp = Provider::kernel(&TCP_IP_PROVIDER)
            .add_callback(callback(tx.clone(), stats.clone(), "TCP"))
            .build();
        // The NETWORK_TCPIP flag emits UDP as well, but it has a separate provider GUID.
        let udp_provider = KernelProvider::new(
            ferrisetw::GUID::from_u128(0xbf3a50c5_a9c9_4988_a005_2df0b7c80f80),
            TCP_IP_PROVIDER.flags,
        );
        let udp = Provider::kernel(&udp_provider)
            .add_callback(callback(tx, stats.clone(), "UDP"))
            .build();
        let name = format!(
            "OpenNetworkMonitor-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        );
        let props = TraceProperties {
            buffer_size: 64,
            min_buffer: 32,
            max_buffer: 128,
            flush_timer: Duration::from_secs(1),
            ..Default::default()
        };
        let (trace,handle)=KernelTrace::new().named(name.clone()).set_trace_properties(props).enable(tcp).enable(udp)
            .start().map_err(|e|anyhow!("Kernel capture could not start: {e:?}. Run as administrator for app usage and LAN/WAN totals."))?;
        let consumer = std::thread::Builder::new()
            .name("network-events".into())
            .spawn(move || {
                let _ = KernelTrace::process_from_handle(handle);
            })?;
        Ok(Self {
            trace: Some(trace),
            consumer: Some(consumer),
            events,
            stats,
            name: name.encode_utf16().chain(Some(0)).collect(),
        })
    }
    pub fn stop(&mut self) {
        let Some(trace) = self.trace.take() else {
            return;
        };
        let session = String::from_utf16_lossy(&self.name[..self.name.len().saturating_sub(1)]);
        // Stop the owned producer first, leaving ProcessTrace open to deliver its final buffers.
        // FerrisETW's stop closes its consumer first, which can discard the trailing events.
        if stop_trace_by_name(&session).is_ok() {
            if let Some(join) = self.consumer.take() {
                let _ = join.join();
            }
            let _ = trace.stop();
        } else {
            let _ = trace.stop();
            if let Some(join) = self.consumer.take() {
                let _ = join.join();
            }
        }
    }
    pub fn health(&self) -> Result<u64> {
        #[repr(C)]
        struct Properties {
            props: EVENT_TRACE_PROPERTIES,
            name: [u16; 256],
        }
        // SAFETY: EVENT_TRACE_PROPERTIES is a plain Windows output structure; zero initialization is valid.
        let mut p: Properties = unsafe { std::mem::zeroed() };
        p.props.Wnode.BufferSize = std::mem::size_of::<Properties>() as u32;
        p.props.LoggerNameOffset = std::mem::offset_of!(Properties, name) as u32;
        // SAFETY: a properly sized and aligned properties buffer and a terminated session name are passed; query does not mutate the trace.
        let code = unsafe {
            ControlTraceW(
                CONTROLTRACE_HANDLE { Value: 0 },
                self.name.as_ptr(),
                &mut p.props,
                EVENT_TRACE_CONTROL_QUERY,
            )
        };
        if code != 0 {
            return Err(anyhow!(
                "Capture health query failed (Windows error {code}); restart capture if traffic has stopped."
            ));
        }
        Ok(u64::from(p.props.EventsLost) + u64::from(p.props.RealTimeBuffersLost))
    }
}
impl Drop for EtwCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod time_tests {
    use super::*;
    #[test]
    fn preserves_low_bits_of_windows_filetime_and_utc_epoch() {
        assert_eq!(filetime_to_unix(133_444_736_009_876_543), 1_700_000_000);
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0);
        assert_eq!(filetime_to_unix(116_444_735_999_999_999), -1);
    }
}
