//! Android Rust-in-app FFI bridge (BRIDGE_PLAN step 1, WS-G).
//!
//! A uniffi **control-plane** surface over [`wdr_refsim::sink::QuicAudioSink`]:
//! the mobile shells construct an [`EngineHandle`], push interleaved PCM blocks,
//! and finish. ADR-001/002 discipline — the engine runs on its **own worker
//! thread** (a `QuicAudioSink` is deliberately not `Send`: the codec adapters
//! are single-threaded C state), and the handle crosses FFI holding only
//! channel senders + an atomic status. The `on_block` command is for a normal
//! capture-worker thread, never a realtime callback (RT hardening lives in
//! `wdr_rt`).
//!
//! Host proof: the exported functions are callable directly (Rust) from
//! `tests/loopback.rs`, which drives the engine against a live
//! `QuicRenderReceiver` and asserts the canonical golden hash. The android `.so`
//! production (cargo-ndk @ real ABIs) is the `android-ci`/device gate.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use wdr_entitlement::provider::Tier;
use wdr_proto::{ChannelLayout, Codec, SampleRepr};
use wdr_refsim::sink::{AudioFrameSink, QuicAudioSink, SinkFormat};

uniffi::setup_scaffolding!();

/// Completed-emission readout from the engine (emitter side). The receiver's
/// integrity hash lives on the receiver, not here — the host loopback test
/// asserts it separately.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FinishReport {
    pub packets_sent: u64,
    pub bytes_sent: u64,
}

/// High-level engine lifecycle state.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq)]
pub enum EngineStatus {
    Idle,
    Dialing,
    Streaming,
    Finished,
    Error,
}

/// Codec selector for the control plane (0 = FLAC lossless, 1 = PCM lossless;
/// Opus/lossy is refused by the secure/control surface).[]
fn codec_from_code(codec: u8) -> Result<Codec, String> {
    match codec {
        0 => Ok(Codec::Flac),
        1 => Ok(Codec::Pcm),
        other => Err(format!("unsupported codec code {other} (0=FLAC, 1=PCM)")),
    }
}

fn tier_from_code(tier: u8) -> Result<Tier, String> {
    match tier {
        0 => Ok(Tier::Free),
        1 => Ok(Tier::Pro),
        other => Err(format!("unsupported tier code {other} (0=free, 1=pro)")),
    }
}

/// Uniform reply for the engine command channel (a control plane round-trip).
enum Reply {
    U,
    R(FinishReport),
    E(String),
}

/// Commands the handle sends to the engine worker thread.
enum Cmd {
    Start {
        addr: String,
        tier: u8,
        codec: u8,
        fingerprint: Option<Vec<u8>>,
        reply: Sender<Reply>,
    },
    OnFormat {
        sample_rate: u32,
        channels: u16,
        sample_repr: u8,
        reply: Sender<Reply>,
    },
    OnBlock {
        block: Vec<u8>,
        reply: Sender<Reply>,
    },
    Finish {
        reply: Sender<Reply>,
    },
}

/// A running emitter engine, wrapped for the uniffi control plane. The object
/// itself is `Send + Sync` (channel senders + a status cell); the non-`Send`
/// `QuicAudioSink` lives on the engine worker thread.
#[derive(uniffi::Object)]
pub struct EngineHandle {
    tx: Arc<Mutex<Option<Sender<Cmd>>>>,
    status: Arc<Mutex<EngineStatus>>,
}

#[uniffi::export]
impl EngineHandle {
    /// Construct a new idle engine (spawns its worker thread).
    #[uniffi::constructor]
    pub fn new() -> Self {
        let (tx, rx) = channel::<Cmd>();
        let status = Arc::new(Mutex::new(EngineStatus::Idle));
        let st = status.clone();
        std::thread::Builder::new()
            .name("wdr-bridge-engine".into())
            .spawn(move || engine_loop(rx, st))
            .expect("spawn bridge engine thread");
        Self {
            tx: Arc::new(Mutex::new(Some(tx))),
            status,
        }
    }

    /// Dial the receiver at `addr` and (optionally) pair over Noise XX with a
    /// pinned peer fingerprint, then prepare the FLAC/PCM lane. Applied once
    /// per session; `on_format` must follow before the first `on_block`. The
    /// Free-tier/lossless policy gate runs inside the sink before any byte is
    /// sent (FR-042).
    pub fn start(
        &self,
        addr: String,
        tier: u8,
        codec: u8,
        fingerprint: Option<Vec<u8>>,
    ) -> Result<(), String> {
        let (reply, rcv) = channel();
        self.send(Cmd::Start {
            addr,
            tier,
            codec,
            fingerprint,
            reply,
        });
        match rcv.recv().map_err(|_| "engine closed".to_string())? {
            Reply::U => Ok(()),
            Reply::E(e) => Err(e),
            Reply::R(_) => Err("unexpected start reply".into()),
        }
    }

    /// Announce the capture format (once, before the first block). Support:
    /// i16/stereo (repr 0) and i24-packed/stereo (repr 2).
    pub fn on_format(
        &self,
        sample_rate: u32,
        channels: u16,
        sample_repr: u8,
    ) -> Result<(), String> {
        let (reply, rcv) = channel();
        self.send(Cmd::OnFormat {
            sample_rate,
            channels,
            sample_repr,
            reply,
        });
        match rcv.recv().map_err(|_| "engine closed".to_string())? {
            Reply::U => Ok(()),
            Reply::E(e) => Err(e),
            Reply::R(_) => Err("unexpected format reply".into()),
        }
    }

    /// Push one interleaved PCM block (canonical LE bytes: 2 B/sample for i16,
    /// low-3 B for i24) into the engine. Call from a capture worker thread —
    /// never from a realtime callback.
    pub fn on_block(&self, block: Vec<u8>) -> Result<(), String> {
        let (reply, rcv) = channel();
        self.send(Cmd::OnBlock { block, reply });
        match rcv.recv().map_err(|_| "engine closed".to_string())? {
            Reply::U => Ok(()),
            Reply::E(e) => Err(e),
            Reply::R(_) => Err("unexpected block reply".into()),
        }
    }

    /// Flush the accumulation, send the end-of-stream marker and return the
    /// emitter-side emission report.
    pub fn finish(&self) -> Result<FinishReport, String> {
        let (reply, rcv) = channel();
        self.send(Cmd::Finish { reply });
        match rcv.recv().map_err(|_| "engine closed".to_string())? {
            Reply::R(r) => Ok(r),
            Reply::E(e) => Err(e),
            Reply::U => Err("unexpected finish reply".into()),
        }
    }

    /// Current engine state (read directly from the shared status cell).
    pub fn status(&self) -> EngineStatus {
        *self.status.lock().unwrap()
    }
}

/// Non-exported internals (uniffi exports every method of the `#[uniffi::export]
/// impl` block, so helpers that must not cross FFI live in a plain impl).
impl EngineHandle {
    /// Send a command to the engine thread (or surface a closed-engine error).
    fn send(&self, cmd: Cmd) {
        let guard = self.tx.lock().unwrap();
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(cmd);
        }
    }
}

impl Default for EngineHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// The engine worker thread: owns the non-`Send` `QuicAudioSink` and executes
/// each command synchronously (the sink's own sync methods block_on its quinn
/// runtime internally).
fn engine_loop(rx: std::sync::mpsc::Receiver<Cmd>, status: Arc<Mutex<EngineStatus>>) {
    let mut sink: Option<QuicAudioSink> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start {
                addr,
                tier,
                codec,
                fingerprint,
                reply,
            } => {
                let res = (|| -> Result<(), String> {
                    let t = tier_from_code(tier)?;
                    let c = codec_from_code(codec)?;
                    let built = match fingerprint {
                        Some(fp) => {
                            if fp.len() != 32 {
                                return Err(format!(
                                    "fingerprint must be 32 bytes, got {}",
                                    fp.len()
                                ));
                            }
                            let mut pin = [0u8; 32];
                            pin.copy_from_slice(&fp);
                            QuicAudioSink::connect_secure(
                                &addr,
                                t,
                                c,
                                Some(pin),
                                wdr_crypto::identity::IdentityKeyPair::generate(),
                            )
                        }
                        None => QuicAudioSink::connect(&addr, t, c),
                    };
                    sink = Some(built.map_err(|e| e.to_string())?);
                    Ok(())
                })();
                *status.lock().unwrap() = if res.is_ok() {
                    EngineStatus::Streaming
                } else {
                    EngineStatus::Error
                };
                let _ = reply.send(match res {
                    Ok(()) => Reply::U,
                    Err(e) => Reply::E(e),
                });
            }
            Cmd::OnFormat {
                sample_rate,
                channels,
                sample_repr,
                reply,
            } => {
                let res = (|| -> Result<(), String> {
                    let repr = match sample_repr {
                        0 => SampleRepr::I16,
                        2 => SampleRepr::I24Packed,
                        other => {
                            return Err(format!(
                                "unsupported sample repr code {other} (0=i16, 2=i24packed)"
                            ))
                        }
                    };
                    if channels != 2 {
                        return Err(format!(
                            "unsupported channel count {channels} (stereo-only)"
                        ));
                    }
                    let fmt = SinkFormat {
                        sample_rate,
                        channels,
                        sample_repr: repr,
                        channel_layout: ChannelLayout::Stereo,
                    };
                    let s = sink.as_mut().ok_or("engine not started".to_string())?;
                    s.on_format(fmt).map_err(|e| e.to_string())
                })();
                let _ = reply.send(match res {
                    Ok(()) => Reply::U,
                    Err(e) => Reply::E(e),
                });
            }
            Cmd::OnBlock { block, reply } => {
                let res = match sink.as_mut() {
                    Some(s) => s.on_block(&block).map_err(|e| e.to_string()),
                    None => Err("engine not started".to_string()),
                };
                let _ = reply.send(match res {
                    Ok(()) => Reply::U,
                    Err(e) => Reply::E(e),
                });
            }
            Cmd::Finish { reply } => {
                let res = (|| -> Result<FinishReport, String> {
                    let s = sink.as_mut().ok_or("engine not started".to_string())?;
                    s.finish().map_err(|e| e.to_string())?;
                    Ok(FinishReport {
                        packets_sent: s.packets_sent(),
                        bytes_sent: s.bytes_sent(),
                    })
                })();
                *status.lock().unwrap() = if res.is_ok() {
                    EngineStatus::Finished
                } else {
                    EngineStatus::Error
                };
                let _ = reply.send(match res {
                    Ok(r) => Reply::R(r),
                    Err(e) => Reply::E(e),
                });
            }
        }
    }
}
