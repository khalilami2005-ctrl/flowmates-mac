# Flowmates local AI resources on macOS

This document records properties that are reproducible from the current
repository. It intentionally does not reuse historical Windows/Vulkan
benchmarks as macOS performance claims.

## Supported runtime

- macOS 14 or newer.
- Apple Silicon (`arm64`) and Intel (`x86_64`) from one universal application.
- `llama.cpp` tag `b10103`, built as a universal Mach-O with Metal enabled.
- Local HTTP listener bound to `127.0.0.1` on an app-managed port.

The repository binary `local_llm/bin/llama-server` is 32,597,376 bytes and
contains both required architectures. Tauri bundles it as the signed sidecar
`Contents/MacOS/llama-server`, while the GGUF files remain under Resources.
Its pre-signing repository SHA-256 is:

```text
91daa04508cd9159642debaf36cfefc7c5ee4c6ef9405bdbfcebdf38b3d0c2f6
```

The binary's deployment target is macOS 14, which is why the Tauri bundle
cannot claim compatibility with an older release.

## Model assets

| Asset | Bytes | SHA-256 |
|---|---:|---|
| `Qwen3-VL-2B-Instruct-Q3_K_M.gguf` | 939,540,160 | `d4346b52a40d103ed6892b09fd3643e0a11b2dd26d3234f37ec68a94ec20ae24` |
| `mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf` | 445,053,216 | `f9a68fabba69c3b81e153367b2c7521030b0fa8bb0de400c9599c8e6725f9c82` |

The model, projector, and server total 1,417,190,752 bytes (about 1.32 GiB)
before application code and disk-image compression. The model files are not
committed. `scripts/fetch-models.mjs` downloads the pinned release assets and
requires exact byte count and SHA-256; a partial file or same-size substitution
is rejected. The same command verifies the local server's byte count, SHA-256,
executable bit, and both Mach-O slices before any build proceeds.

The application bundle includes Flowmates's license documents and
`THIRD_PARTY_NOTICES.md`, including the complete MIT notice for `llama.cpp`
and Apache License 2.0 terms for Qwen3-VL.

## Runtime configuration

The current backend:

- resizes screenshots to 960×540 before inference;
- uses a 4,096-token context;
- configures two parallel slots and two CPU threads;
- selects GPU layers automatically, with a manual override for diagnostics;
- keeps the model and activity database in local app storage;
- sends only aggregate summaries to Supabase when cloud sync is enabled and
  the user has an active entitlement.

Peak RAM, Metal allocation, inference latency, and energy use vary by Mac,
display topology, and GPU-layer selection. Release qualification should record
those measurements separately for at least one Apple Silicon Mac and one Intel
Mac instead of extrapolating from another platform.

## Release checks

The tagged release workflow verifies both `arm64` and `x86_64` slices in the
application executable and bundled server with `lipo`, verifies the sidecar
signature explicitly, then verifies the deep app signature, notarization
ticket, and Gatekeeper assessment. Model hashes are checked before signing.

When replacing any binary or model, update all of the following together:

1. The GitHub model release.
2. Size and SHA-256 metadata in `scripts/fetch-models.mjs`.
3. Resource names in `tauri.conf.json` and `vision_model.rs` if names changed.
4. This inventory and the release smoke-test results.
