# Third-Party License & Attribution Notices

Owner: OSS/Licensing (task `t-B0-ossdep`, root `R-B0-OSSDEP`). Last verified: 2026-09-06.

**Product license:** Wavelink itself is proprietary, source-available software
(see the repo root [`LICENSE`](../../LICENSE)) — it is not MIT/Apache-2.0 or any
other open-source license. This file is the **template for third-party
attribution**: third-party components remain under their own permissive
licenses and require these notices in every shipped build. The shipping
`NOTICE`/`THIRD_PARTY_NOTICES.md` bundled into each release artifact is
generated at build time by `cargo-deny` and `cargo-license` — see SBOM_POLICY.md.

Never hand-type license text if you can copy it from the vendored origin file instead. Unknown origin (no LICENSE/NOTICE file captured at vendoring time) fails the build gate — this is enforced by `cargo-deny` (missing-license = error, see SBOM_POLICY.md `deny.toml`) and the license-audit CI workflow.

## MIT — notice blocks

MIT usually requires: © notice + permission notice, "THE SOFTWARE IS PROVIDED AS-IS…" warranty paragraph. Copy from the crate's `LICENSE`/`LICENSE-MIT` file.

Required for: `quinn` (MIT/Apache-2.0), `rubato` (MIT/Apache-2.0), `mdns-sd` (MIT/Apache-2.0), `snow` (MIT/Apache-2.0), `chacha20poly1305` / RustCrypto crates (MIT/Apache-2.0), `postcard` (MIT/Apache-2.0), `serde` (MIT/Apache-2.0), `clap` (MIT/Apache-2.0), `qrcode` (MIT/Apache-2.0), `proptest` (MIT/Apache-2.0), `criterion` (MIT/Apache-2.0), `tokio` (MIT), `tracing` / `tracing-subscriber` (MIT), PipeWire lib pipewire (MIT component), `gtk4-rs` bindings (MIT), `cargo-ndk` (MIT/Apache-2.0, tool only), `cargo-zigbuild` (MIT, tool), `cross` (MIT/Apache-2.0, tool).

```text
MIT License

Copyright (c) <COPYRIGHT HOLDER(S) FROM <crate>/LICENSE>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## BSD-2-Clause / BSD-3-Clause — notice blocks

BSD requires: copyright line + "Redistribution and use in source and binary forms…" three/four-paragraph disclaimer. Copy exact text + copyright from origin.

Required for:
- **libopus** — from vendored `libopus/COPYING` (BSD-3-Clause) — bundled via `opus`/`opusic-sys` (ADR-004).
- **libFLAC** — from vendored `libFLAC/COPYING` (BSD-3-Clause Xiph variant) — bundled via `flac-bound`/`libflac-sys` (ADR-005). We use the BSD-3 variant, **not** the LGPL option.
- **ed25519-dalek / x25519-dalek / curve25519-dalek** — from their LICENSE (BSD-3-Clause) — note also references to the `fiat` (MIT) / `Peter Schwabe` contributions; include full daleks notice as shipped upstream.
- Note any vendored C that carries BSD (e.g., if `ring` were adopted: `ring` is ISC-style with mixed licensing — see ISC below).

```text
BSD 3-Clause License

Copyright (c) <YEAR>, <HOLDERS per origin COPYING/LICENSE>.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES ...
```

(Use the full four-paragraph disclaimer text from the origin file; the ellipsis above is a placeholder for template brevity only.)

## Apache-2.0 — notice blocks

Apache-2.0 requires: full `LICENSE` text + **NOTICE** file reproduction if the distribution includes a NOTICE (Apache-2.0 §4(d)), and an appendix of unlicensed dependencies (StreamZ "Appendix" pattern). Many crates are dual MIT/Apache-2.0 — choosing MIT attribution is legally sufficient, but we ship the Apache text where a crate's `LICENSE` is Apache-only or for clarity.

Apache-only or primary: `claxon` (Apache-2.0), `hound` (Apache-2.0), `mdns-sd` (Apache-2.0 primary), `windows-rs` (MIT/Apache-2.0), `screencapturekit-rs` (Apache-2.0), **Oboe** (Apache-2.0), `uniffi` (MPL-2.0 — not Apache; see MPL below).

```text
Apache License
Version 2.0, January 2004
http://www.apache.org/licenses/

...full license text from the crate's LICENSE-APACHE / Apache-2.0 origin...

APPENDIX: How to apply the Apache License to your work...
```

## ISC — notice (if/when `ring` enters the tree)

`ring` is our default TLS provider via `rustls-ring` (quinn's default). `ring`'s modern components are **ISC-style** (with the copyright/permission notice including "Google Inc." and "Brian Smith" lines). If `ring` lands in the lockfile, include its full NOTICE/LICENCE text (ISC) from the vendored source.

```text
Copyright (c) 2015-2016 the fiat-crypto authors ...
Copyright (c) 2015-2024 the ring authors ...
...ISC permission notice from ring's LICENCE...
```

(Placeholder — text is generated at build time from `vendor/ring`.)

## Zlib — if/when included

None currently required (we do not statically ship zlib). If a platform SDK path pulls in zlib, add the zlib notice block here + in the generated NOTICE.

## MPL-2.0 — recorded decision (uniffi)

`uniffi` (and `uniffi_core`) is **MPL-2.0 (weak copyleft)**. Recorded decision (DEPENDENCY_EVALUATION.md §Policy):
- We link it for control-plane FFI only; we **do not modify** the MPL-2.0 runtime.
- Generated Kotlin/Swift bindings from our own Rust sources are **ours** (not covered by MPL).
- Shipping MPL-2.0 requires: include a copy of the **MPL-2.0** license text ("license must be made available") and provide **source availability** for the MPL-covered compiled objects (uniffi runtime). The MPL is not "copyleft" over our app code, but the source of the MPL-2.0 file (uniffi) must remain available — carry it in the SBOM `source` field + LINK.
- `deny.toml` explicitly allows MPL-2.0 **only** for uniffi (+uniffi_core, +uniffi_meta) with recorded justification.

`cbindgen` is MPL-2.0 but build-time only; its generated C headers are ours, no notice required in shipped binaries (record in SBOM as build-time tool).

## Where origin files live (placeholders — captured at vendoring time)

The following origin files are **placeholders marked "generated at build time"**; the actual text is copied into the committed/release `NOTICE` by the build (license-audit CI + cargo-deny/cargo-license), not authored here:

| Component | Origin file | License | Required notice |
|---|---|---|---|
| libopus (`opus`, `opusic-sys`) | `vendor/libopus/COPYING` | BSD-3-Clause | BSD-3 notice + copyright |
| libFLAC (`flac-bound`, `libflac-sys`) | `vendor/libFLAC/COPYING` | BSD-3-Clause (Xiph) | BSD-3 notice + copyright (not LGPL option) |
| ed25519-dalek / x25519-dalek / curve25519-dalek | `vendor/curve25519-dalek/...` LICENSE | BSD-3-Clause | BSD-3 notice + copyright |
| ring (if in lockfile via rustls-ring) | `vendor/ring/LICENCE` | ISC-style | ISC notice + copyright |
| uniffi (+uniffi_core) | `vendor/uniffi-rs/LICENSE` | MPL-2.0 | MPL-2.0 text + source availability |
| All other crates | crate `LICENSE*`/`NOTICE` files on crates.io | MIT/Apache-2.0 etc. | per-block above |

## Build-time generation

- `cargo deny licenses` / `cargo-license` → merge all license texts + copyright lines into one `NOTICE` file.
- `cargo-cyclonedx`/`syft` → structured SBOM (each entry carries `licenses` + the `source`/`purl` link — required for MPL source availability). See SBOM_POLICY.md.
- The generated file is **committed per release** next to release artifacts and embedded in installers where format allows.

## Audit trail

| Date | Action |
|---|---|
| 2026-09-06 | Template created (t-B0-ossdep); records policy for BSD/MIT/Apache/ISC/MPL blocks; placeholders marked for build-time generation |
