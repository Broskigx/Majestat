# 0004. Rust and wgpu as the foundation

* Status: Accepted
* Date: 2026-10-13

## Context

The alternatives for a professional non-linear editor's foundation are essentially:

* **C++ with Qt, Vulkan/D3D12/Metal.** What Resolve and Premiere are built on. Maximum
  ecosystem maturity — OpenFX, VST3, Qt widgets, decades of codec integration — at the
  cost of manual memory management in a program that runs untrusted decoders over
  arbitrary files for hours.
* **Electron or Tauri with a native media core.** Fast to build the interface and panels,
  but a video editor's timeline and viewer need low, predictable input-to-pixel latency
  and a GPU surface they control. A webview in that path is a liability.
* **Rust with a portable GPU abstraction.** Memory safety across the whole engine, a real
  type system for the model, and one graphics API that compiles to D3D12, Vulkan and
  Metal. Less mature ecosystem for audio and plugin hosts.

Constraints from the host environment at the time of the decision: Rust 1.99 with both
GNU and MSVC toolchains installed, an RTX 4060 Ti, and no C toolchain (see ADR 0009).

## Decision

Rust, with `wgpu` for graphics and compute.

* Layers 0 and 1 have **no** GPU, codec or platform dependency at all, which is why they
  can be tested exhaustively without a device.
* `wgpu` gives one code path over D3D12 and Vulkan, and covers Metal when macOS arrives.
  It supports compute, which the effect graph needs.
* Portability is a *hypothesis to test*, not a premise. Decoder-surface interop, HDR
  presentation and plugin GPU paths must be prototyped and measured in M4 before the
  cross-platform promise is made. Narrow native bridges are expected where `wgpu` cannot
  reach — that is a planned compromise, not an emergency.
* The interface starts on `egui`, because it shares the `wgpu` device and gets a usable
  shell fast, and is kept thin and isolated so it can be replaced by a custom-rendered
  native interface when visual fidelity demands it. Reversible by design.

## Consequences

* The editor inherits memory safety for parsed-file handling, which is the class of bug
  most likely to be exploited in a program that opens arbitrary media.
* Crates for audio I/O, plugin hosting and codec binding are less mature than their C++
  equivalents. Each is isolated behind a trait boundary at its layer.
* The `wgpu`/WebGPU specification is still evolving, so pins will need periodic attention
  and some churn is expected.
* Target-era Rust with edition 2024 is assumed. The MSVC toolchain is preferred for
  release distribution, but the host currently has no Visual Studio installation, so
  builds here use the GNU toolchain with the bundled `rust-lld` linker.
* `wgpu` is deliberately **not** added to the workspace in M0. It arrives in M4 with the
  render graph, so the foundation stays buildable in seconds.
