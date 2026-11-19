# ADR-0001: egui baseline with optional dedicated wgpu framebuffer renderer
Status: PROVISIONAL pending Intel Ventura test (2026-11-18)

Context: Want modern dark native-style GUI with low overhead, fullscreen and high resolution remote frames, Rust memory safety, one CLI/GUI config. Consider GPUI (as used by RV), egui/eframe, Tauri2/WKWebView, SwiftUI+Rust, bare winit/wgpu.

Decision: Use egui/eframe 0.37 as P0. `eframe` exposes wgpu Metal on macOS and a simple UX layer. A GPU framebuffer renderer with dirty-region updates will replace full-image texture updates in P3. The networking core remains independent.

Consequences: Easy P0 GUI in Rust; rendering may not have the absolute lowest frame overhead; 4K full-buffer texture uploads are a known performance issue. GPU/OS compatibility has to be empirically verified. `App::ui` changed in 0.37. The fallback is older compatible eframe/glow or controlled migration to GPUI, after measuring, not guessing.
