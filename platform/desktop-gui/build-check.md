# desktop-gui — build & validation check

Cross-platform (Windows + Linux) iced GUI scaffold. Standalone workspace, like
every `platform/` shell. **Feature split:** `default` (no `gui`) = the transport
driver (`StreamDriver` trait + real `FixtureDriver` over
`wdr_refsim::sink::QuicAudioSink`) — verifiable on ANY host. The `gui` feature
(iced window: status card, single Stop/Start, receiver address, metrics,
activity log, tier, dark/light) needs a native runner's display/GPU libraries.

## On this host (macOS, 2026-09-12)

```bash
cd platform/desktop-gui
cargo check                       # wraps the whole core seam (quinn/opus/flac) — slow first time
cargo test                        # driver unit tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

- `cargo check --features gui` on this macOS host pulls iced + winit but does
  NOT build Windows/Linux natives — that is the runner gate.
- **Windows native link / Linux windowing (`libxkbcommon`, Wayland, etc.)** are
  runner-gated (windows-ci / linux-ci) — stated, not faked.
- The fixture driver streams the same canonical fixture the macOS `--stream`
  gate uses (hash-perfect vs `ref_receiver`), so the GUI's numbers are real,
  not simulated. Native win/linux **system capture** is a recorded follow-up.

## Next gate

- Window: implement the iced view + wire `StreamDriver` on the runner.
- Native capture: `win-emitter --stream` / `linux-emitter --stream` equivalents
  (WASAPI loopback / PipeWire) then the GUI drives real audio.
