# Local mock server quick start (P0)

Terminal A:

```sh
python3 tests/mock_rfb.py --port 5999 --width 800 --height 450
```

Terminal B:

```sh
cargo run -p glintvnc-app -- profiles add "Demo" 127.0.0.1 --port 5999
cargo run -p glintvnc-app -- probe Demo
cargo run -p glintvnc-app -- connect Demo
```

Expected: a 4-stripe red/green/blue/white pattern (not a real remote desktop), native full-screen if enabled, no password required for this **loopback-only** server. Press the app's Connections button to exit viewing. To test disconnect handling, restart mock server with `--disconnect-after 5` and inspect connection overlay/backoff. No public interface is opened.

**The mock server is provided as a test fixture only and has not been exercised with the Rust binary in the artifact-authoring environment.**
