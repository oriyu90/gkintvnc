//! In-process mock RFB 3.8 server used to exercise `protocol::run_once` on
//! loopback only. It implements just enough of RFC 6143 for the client and
//! never listens on external interfaces.
use glintvnc_core::configuration::{Profile, SecurityPolicy};
use glintvnc_core::protocol::{self, Command, Event};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{self, sync_channel};
use std::time::Duration;

const W: u16 = 8;
const H: u16 = 4;

fn read_exact(stream: &mut std::net::TcpStream, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_hello(stream: &mut std::net::TcpStream) {
    stream.write_all(b"RFB 003.008\n").unwrap();
    let mut version = [0u8; 12];
    stream.read_exact(&mut version).unwrap();
    assert_eq!(&version, b"RFB 003.008\n");
}

fn send_framebuffer_init(stream: &mut std::net::TcpStream) {
    let name = b"mock";
    let fmt = [32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0];
    let mut msg = Vec::new();
    msg.extend_from_slice(&W.to_be_bytes());
    msg.extend_from_slice(&H.to_be_bytes());
    msg.extend_from_slice(&fmt);
    msg.extend_from_slice(&(name.len() as u32).to_be_bytes());
    msg.extend_from_slice(name);
    stream.write_all(&msg).unwrap();
}

fn expect_client_init(stream: &mut std::net::TcpStream) {
    // Client message 1 (SetPixelFormat): 20 bytes; then SetEncodings (2).
    let msg = read_exact(stream, 1).unwrap();
    assert_eq!(msg[0], 0, "expected SetPixelFormat");
    let _ = read_exact(stream, 19).unwrap();
    let msg = read_exact(stream, 1).unwrap();
    assert_eq!(msg[0], 2, "expected SetEncodings");
    let _ = read_exact(stream, 1).unwrap(); // padding
    let n = read_exact(stream, 2).unwrap();
    let count = u16::from_be_bytes([n[0], n[1]]) as usize;
    let _ = read_exact(stream, 4 * count).unwrap();
    let msg = read_exact(stream, 1).unwrap();
    assert_eq!(msg[0], 3, "expected FramebufferUpdateRequest");
    let _ = read_exact(stream, 9).unwrap();
}

fn send_zrle_frame(stream: &mut std::net::TcpStream) {
    // One 8x4 rect, ZRLE encoded, one zlib stream for the whole connection.
    // Tile content: packed palette with 2 entries (blue-ish vs red-ish), 1 bit per pixel.
    let subenc = 2u8; // palette_size == 2 -> 1-bit packed pixels
    let palette = [[255u8, 0, 0], [0, 0, 255]]; // cpixel B,G,R: first B=255,N second R=255
    let mut tile = vec![subenc];
    for px in palette {
        tile.extend_from_slice(&px);
    }
    for row in 0..H {
        let mut bits = 0u8;
        for x in 0..W {
            if (x + row).is_multiple_of(2) {
                bits |= 0x80u8 >> x; // even pixel -> palette[0]
            }
        }
        tile.push(bits);
    }
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&tile).unwrap();
    let compressed = enc.finish().unwrap();
    let mut msg = vec![0u8, 0u8]; // message type + padding
    msg.extend_from_slice(&1u16.to_be_bytes()); // one rectangle
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&W.to_be_bytes());
    msg.extend_from_slice(&H.to_be_bytes());
    msg.extend_from_slice(&16i32.to_be_bytes()); // ZRLE encoding
    msg.extend_from_slice(&(compressed.len() as u32).to_be_bytes()); // U32 zlib length
    msg.extend_from_slice(&compressed);
    stream.write_all(&msg).unwrap();
}

#[test]
fn zrle_rectangle_is_decoded_end_to_end() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_zrle_frame(stream);
        // Client must not disconnect: send an incremental request after applying it.
        let msg = read_exact(stream, 1).unwrap();
        assert_eq!(msg[0], 3, "expected incremental FramebufferUpdateRequest");
        let _ = read_exact(stream, 9).unwrap();
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let mut canvas = vec![0u8; W as usize * H as usize * 4];
    let mut got = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && !got {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(ev @ Event::Frame { .. }) => {
                apply_frame(&mut canvas, W as usize, &ev);
                got = true;
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    assert!(got, "expected a decoded ZRLE frame");
    for y in 0..H as usize {
        for x in 0..W as usize {
            let o = (y * W as usize + x) * 4;
            let expect_blue = (x + y) % 2 == 0;
            if expect_blue {
                assert_eq!(&canvas[o..o + 3], &[255, 0, 0]); // B=255 -> RGBA B channel
            } else {
                assert_eq!(&canvas[o..o + 3], &[0, 0, 255]); // R=255 -> RGBA R channel
            }
        }
    }
}

fn send_copyrect_frame(stream: &mut std::net::TcpStream) {
    let mut msg = vec![0u8, 0u8]; // message type + padding
    msg.extend_from_slice(&1u16.to_be_bytes()); // one rectangle
    msg.extend_from_slice(&4u16.to_be_bytes()); // x
    msg.extend_from_slice(&2u16.to_be_bytes()); // y
    msg.extend_from_slice(&4u16.to_be_bytes()); // width
    msg.extend_from_slice(&2u16.to_be_bytes()); // height
    msg.extend_from_slice(&1i32.to_be_bytes()); // CopyRect encoding
    msg.extend_from_slice(&0u16.to_be_bytes()); // src x
    msg.extend_from_slice(&0u16.to_be_bytes()); // src y
    stream.write_all(&msg).unwrap();
}

fn send_one_raw_frame(stream: &mut std::net::TcpStream) {
    let mut msg = vec![0u8, 0u8]; // message type + padding
    msg.extend_from_slice(&1u16.to_be_bytes()); // one rectangle
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&W.to_be_bytes());
    msg.extend_from_slice(&H.to_be_bytes());
    msg.extend_from_slice(&0i32.to_be_bytes()); // Raw encoding
    for y in 0..H {
        for x in 0..W {
            let px = [x as u8, y as u8, 128u8, 0u8]; // B, G, R, unused (LE true-colour)
            msg.extend_from_slice(&px);
        }
    }
    stream.write_all(&msg).unwrap();
}

/// Drain client -> server messages until the client disconnects or `limit`
/// bytes arrive; used to keep the mock alive while the client runs.
fn drain(stream: &mut std::net::TcpStream) {
    drain_record(stream, None);
}

/// Like `drain`, but also appends everything the client sends into `sink` when provided.
fn drain_record(
    stream: &mut std::net::TcpStream,
    sink: Option<std::sync::Arc<std::sync::Mutex<Vec<u8>>>>,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut buf = [0u8; 256];
    let mut total = 0usize;
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Some(s) = &sink {
                    s.lock().unwrap().extend_from_slice(&buf[..n]);
                }
                total += n;
                if total > 64 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn spawn_mock<F>(handler: F) -> u16
where
    F: FnOnce(&mut std::net::TcpStream) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        handler(&mut stream);
    });
    port
}

fn test_profile(port: u16) -> Profile {
    Profile {
        id: "integrationtest".into(),
        name: "integration".into(),
        host: "127.0.0.1".into(),
        port,
        username: None,
        auto_reconnect: false,
        fullscreen_on_connect: false,
        scale_mode: Default::default(),
        key_preset: Default::default(),
        security: SecurityPolicy::LocalhostOnly,
    }
}

#[test]
fn none_auth_raw_frame_round_trip() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap(); // one security type: None
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap(); // SecurityResult OK
        let _ = read_exact(stream, 1).unwrap(); // shared flag
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_one_raw_frame(stream);
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let mut got_connected = false;
    let mut got_frame = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && !(got_connected && got_frame) {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Connected { width, height, .. }) => {
                assert_eq!((width, height), (W, H));
                got_connected = true;
            }
            Ok(Event::Frame {
                width,
                height,
                rects,
            }) => {
                assert_eq!((width as usize, height as usize), (W as usize, H as usize));
                let bytes: usize = rects.iter().map(|r| r.rgba.len()).sum();
                assert_eq!(bytes, W as usize * H as usize * 4);
                got_frame = true;
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    assert!(
        got_connected && got_frame,
        "expected Connected and Frame events"
    );
}

#[test]
fn vncauth_wrong_password_is_terminal_error() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 2]).unwrap(); // offer VNC auth only
        assert_eq!(read_exact(stream, 1).unwrap()[0], 2);
        stream.write_all(&[0xAB; 16]).unwrap(); // fixed challenge
        let _ = read_exact(stream, 16).unwrap(); // client response
        stream.write_all(&1u32.to_be_bytes()).unwrap(); // SecurityResult: failed
        let reason = b"mock says no";
        stream
            .write_all(&(reason.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(reason).unwrap();
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(2);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let result = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(
            profile,
            Some("mock-password".into()),
            cmd_rx,
            event_tx,
            None,
        ))
    })
    .join()
    .unwrap();
    let err = result.expect_err("wrong password must fail");
    assert!(err.to_string().contains("mock says no") || err.to_string().contains("authentication"));
    assert!(!err.retryable(), "auth failure must not be retryable");
    drop(event_rx);
}

#[test]
fn non_loopback_blocked_by_default() {
    let mut profile = test_profile(5900);
    profile.host = "192.0.2.10".into(); // TEST-NET-1, documentation range
    let (event_tx, _event_rx) = sync_channel(1);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt
        .block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
        .expect_err("must refuse plain RFB off-loopback");
    assert!(matches!(err, protocol::Error::InsecureBlocked));
}

#[test]
fn oversized_name_rejected() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        // Framebuffer init with an oversized name (> 4096 bytes)
        let fmt = [32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0];
        let mut msg = Vec::new();
        msg.extend_from_slice(&W.to_be_bytes());
        msg.extend_from_slice(&H.to_be_bytes());
        msg.extend_from_slice(&fmt);
        msg.extend_from_slice(&5000u32.to_be_bytes());
        stream.write_all(&msg).unwrap();
    });
    let profile = test_profile(port);
    let (event_tx, _event_rx) = sync_channel(1);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt
        .block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
        .expect_err("oversized name must fail");
    assert!(matches!(err, protocol::Error::Protocol(_)));
}

fn send_server_cuttext(stream: &mut std::net::TcpStream, text: &str) {
    let payload = text.as_bytes();
    let mut msg = vec![3u8, 0, 0, 0]; // ServerCutText + padding
    msg.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    msg.extend_from_slice(payload);
    stream.write_all(&msg).unwrap();
}

#[test]
fn incremental_updates_preserve_unchanged_pixels() {
    // Regression test: the client used to discard the canvas between updates and
    // apply incremental rectangles onto a zeroed buffer (the "black screen" bug).
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_one_raw_frame(stream);
        let msg = read_exact(stream, 1).unwrap();
        assert_eq!(msg[0], 3, "expected incremental FramebufferUpdateRequest");
        let _ = read_exact(stream, 9).unwrap();
        send_copyrect_frame(stream);
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let mut canvas = vec![0u8; W as usize * H as usize * 4];
    let mut snapshots = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && snapshots.len() < 2 {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(ev @ Event::Frame { .. }) => {
                apply_frame(&mut canvas, W as usize, &ev);
                snapshots.push(canvas.clone());
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    assert_eq!(snapshots.len(), 2);
    let px = |f: &Vec<u8>, x: usize, y: usize| -> [u8; 4] {
        let o = (y * W as usize + x) * 4;
        [f[o], f[o + 1], f[o + 2], f[o + 3]]
    };
    // (0,0) receives no updates from the CopyRect; it must survive from frame 1.
    assert_eq!(px(&snapshots[1], 0, 0), px(&snapshots[0], 0, 0));
    assert_ne!(px(&snapshots[1], 0, 0), [0, 0, 0, 0]);
}

/// Apply a Frame event's dirty rectangles onto a persistent RGBA canvas (test helper).
fn apply_frame(canvas: &mut [u8], fb_w: usize, frame: &Event) {
    if let Event::Frame { rects, .. } = frame {
        for r in rects {
            let src_stride = usize::from(r.width) * 4;
            for row in 0..usize::from(r.height) {
                let src = &r.rgba[row * src_stride..(row + 1) * src_stride];
                let dst = ((usize::from(r.y) + row) * fb_w + usize::from(r.x)) * 4;
                canvas[dst..dst + src_stride].copy_from_slice(src);
            }
        }
    }
}

#[test]
fn clipboard_cut_text_round_trip() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = captured.clone();
    let port = spawn_mock(move |stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_server_cuttext(stream, "remote-clip");
        send_one_raw_frame(stream);
        drain_record(stream, Some(sink));
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let mut got_text = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && got_text.is_none() {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::ClipboardText(t)) => got_text = Some(t),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    cmd_tx
        .send(Command::CutText {
            text: "local-clip".into(),
        })
        .unwrap();
    cmd_tx.send(Command::Stop).unwrap();
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    assert_eq!(got_text.as_deref(), Some("remote-clip"));
    std::thread::sleep(Duration::from_millis(100));
    let buf = captured.lock().unwrap().clone();
    let mut expected = vec![6u8, 0, 0, 0];
    expected.extend_from_slice(&10u32.to_be_bytes());
    expected.extend_from_slice(b"local-clip");
    assert!(
        buf.windows(expected.len()).any(|w| w == expected),
        "ClientCutText bytes not found in {buf:02x?}"
    );
}

fn send_cursor_rect(stream: &mut std::net::TcpStream) {
    // 2x2 cursor, hotspot (1,1). Pixels in our negotiated format are B,G,R (little-endian,
    // pixel 0x00RRGGBB); the bitmask is MSB-first, padded to whole bytes per scanline.
    let pixels: [u8; 16] = [
        10, 20, 30, 0, // (0,0) -> B=10,G=20,R=30
        40, 50, 60, 0, // (1,0)
        70, 80, 90, 0, // (0,1)
        100, 110, 120, 0, // (1,1)
    ];
    let mask = [0b1100_0000u8, 0b1000_0000]; // row0: both valid; row1: only (0,1)
    let mut msg = Vec::new();
    msg.extend_from_slice(&[0, 0]); // FramebufferUpdate + padding
    msg.extend_from_slice(&1u16.to_be_bytes()); // one rectangle
    msg.extend_from_slice(&1u16.to_be_bytes()); // x = hotspot_x = 1
    msg.extend_from_slice(&1u16.to_be_bytes()); // y = hotspot_y = 1
    msg.extend_from_slice(&2u16.to_be_bytes()); // width = 2
    msg.extend_from_slice(&2u16.to_be_bytes()); // height = 2
    msg.extend_from_slice(&(-239i32).to_be_bytes()); // Cursor pseudo-encoding
    msg.extend_from_slice(&pixels);
    msg.extend_from_slice(&mask);
    stream.write_all(&msg).unwrap();
}

#[test]
fn cursor_pseudo_encoding_is_parsed() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap(); // ClientInit shared flag
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_cursor_rect(stream);
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let mut cursor = None;
    let mut saw_frame = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && (cursor.is_none() || !saw_frame) {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(protocol::Event::Cursor {
                width,
                height,
                hot_x,
                hot_y,
                rgba,
            }) => {
                cursor = Some((width, height, hot_x, hot_y, rgba));
            }
            Ok(protocol::Event::Frame { .. }) => saw_frame = true,
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    assert!(
        saw_frame,
        "cursor-only update should still surface a Frame event"
    );
    let (width, height, hot_x, hot_y, rgba) = cursor.expect("expected Event::Cursor");
    assert_eq!((width, height, hot_x, hot_y), (2, 2, 1, 1));
    // (0,0): raw (10,20,30) -> RGBA (30,20,10, valid mask=1)
    assert_eq!(&rgba[0..4], &[30, 20, 10, 255]);
    // (1,1): raw (100,110,120), mask bit 0 -> transparent
    assert_eq!(&rgba[12..16], &[120, 110, 100, 0]);
}

fn send_led_state(stream: &mut std::net::TcpStream, state: u8) {
    let mut msg = Vec::new();
    msg.extend_from_slice(&[0, 0]);
    msg.extend_from_slice(&1u16.to_be_bytes());
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&(-261i32).to_be_bytes());
    msg.push(state);
    stream.write_all(&msg).unwrap();
}

#[test]
fn led_state_pseudo_encoding_is_parsed() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_led_state(stream, 0b0000_0110); // caps + num lock
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let mut led = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && led.is_none() {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(protocol::Event::LedState(s)) => led = Some(s),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    assert_eq!(led, Some(0b0000_0110));
}

#[test]
fn key_and_pointer_events_are_transmitted() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = captured.clone();
    let port = spawn_mock(move |stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_one_raw_frame(stream);
        drain_record(stream, Some(sink));
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    // Wait for the first frame, then send one key and one pointer event, then Stop.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Frame { .. }) => break,
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for frame"
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("worker disconnected before frame"),
        }
    }
    cmd_tx
        .send(Command::Key {
            keysym: 0xff0d,
            pressed: true,
        })
        .unwrap();
    cmd_tx
        .send(Command::Key {
            keysym: 0xff0d,
            pressed: false,
        })
        .unwrap();
    cmd_tx
        .send(Command::Pointer {
            x: 100,
            y: 200,
            buttons: 1,
        })
        .unwrap();
    cmd_tx.send(Command::Stop).unwrap();
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    let buf = captured.lock().unwrap().clone();
    let key_down = [4u8, 1, 0, 0, 0x00, 0x00, 0xff, 0x0d];
    let key_up = [4u8, 0, 0, 0, 0x00, 0x00, 0xff, 0x0d];
    let pointer = [5u8, 1, 0x00, 100, 0x00, 200];
    let contains = |needle: &[u8]| buf.windows(needle.len()).any(|w| w == needle);
    assert!(
        contains(&key_down),
        "key down event not found in {buf:02x?}"
    );
    assert!(contains(&key_up), "key up event not found in {buf:02x?}");
    assert!(contains(&pointer), "pointer event not found in {buf:02x?}");
}
#[test]
fn refresh_triggers_full_framebuffer_request() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_zrle_frame(stream);
        // First incremental request (sent after the initial frame).
        let msg = read_exact(stream, 1).unwrap();
        assert_eq!(msg[0], 3, "expected FramebufferUpdateRequest");
        let rest = read_exact(stream, 9).unwrap();
        assert_eq!(rest[0], 1, "first request must be incremental");
        // The Refresh command must produce a NON-incremental request.
        let msg = read_exact(stream, 1).unwrap();
        assert_eq!(msg[0], 3, "expected FramebufferUpdateRequest after Refresh");
        let rest = read_exact(stream, 9).unwrap();
        assert_eq!(
            rest[0], 0,
            "Refresh must force a full (non-incremental) request"
        );
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut refreshed = false;
    while std::time::Instant::now() < deadline && !refreshed {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Frame { .. }) => {
                let _ = cmd_tx.send(Command::Refresh);
                refreshed = true;
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(
        matches!(result, Ok(())),
        "refresh request should not kill the session"
    );
    assert!(
        refreshed,
        "expected to see the first frame and issue Refresh"
    );
}

/// Read one FramebufferUpdateRequest and return its incremental flag.
fn read_fur_incremental(stream: &mut std::net::TcpStream) -> u8 {
    let msg = read_exact(stream, 10).unwrap();
    assert_eq!(msg[0], 3, "expected FramebufferUpdateRequest");
    msg[1]
}

#[test]
fn desktop_size_forces_non_incremental_request() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        send_one_raw_frame(stream);
        // First follow-up is incremental as usual.
        assert_eq!(read_fur_incremental(stream), 1);
        // DesktopSize discards our canvas: RFC 6143 requires incremental=0 next.
        let mut msg = vec![0u8, 0, 0, 1]; // type, padding, count=1
        msg.extend_from_slice(&0u16.to_be_bytes()); // x
        msg.extend_from_slice(&0u16.to_be_bytes()); // y
        msg.extend_from_slice(&4u16.to_be_bytes()); // new width
        msg.extend_from_slice(&2u16.to_be_bytes()); // new height
        msg.extend_from_slice(&(-223i32).to_be_bytes()); // DesktopSize
        stream.write_all(&msg).unwrap();
        assert_eq!(
            read_fur_incremental(stream),
            0,
            "DesktopSize must force a full (non-incremental) request"
        );
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut full_frame_after_resize = false;
    while std::time::Instant::now() < deadline {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Frame { width, height, .. }) => {
                if (width, height) == (4, 2) {
                    full_frame_after_resize = true;
                    break;
                }
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(
        matches!(result, Ok(())),
        "resize should not kill the session"
    );
    assert!(
        full_frame_after_resize,
        "expected a frame after DesktopSize"
    );
}

#[test]
fn libvncsrv_keyboard_led_state_is_parsed() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        // LibVNCServer LED State: state in rect x, no payload byte.
        let mut msg = vec![0u8, 0, 0, 1];
        msg.extend_from_slice(&0b0000_0110u16.to_be_bytes()); // x = state
        msg.extend_from_slice(&0u16.to_be_bytes()); // y
        msg.extend_from_slice(&1u16.to_be_bytes()); // w
        msg.extend_from_slice(&1u16.to_be_bytes()); // h
        msg.extend_from_slice(&(0xFFFE0000u32 as i32).to_be_bytes());
        stream.write_all(&msg).unwrap();
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let mut led = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && led.is_none() {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::LedState(s)) => led = Some(s),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    assert_eq!(led, Some(0b0000_0110));
}

#[test]
fn negative_cursor_hotspot_is_clamped() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap();
        assert_eq!(read_exact(stream, 1).unwrap()[0], 1);
        stream.write_all(&0u32.to_be_bytes()).unwrap();
        let _ = read_exact(stream, 1).unwrap();
        send_framebuffer_init(stream);
        expect_client_init(stream);
        let pixels: [u8; 16] = [
            10, 20, 30, 0, 40, 50, 60, 0, 70, 80, 90, 0, 100, 110, 120, 0,
        ];
        let mut msg = vec![0u8, 0, 0, 1];
        msg.extend_from_slice(&0xffffu16.to_be_bytes()); // x = -1 as S16
        msg.extend_from_slice(&0xffffu16.to_be_bytes()); // y = -1 as S16
        msg.extend_from_slice(&2u16.to_be_bytes());
        msg.extend_from_slice(&2u16.to_be_bytes());
        msg.extend_from_slice(&(-239i32).to_be_bytes());
        msg.extend_from_slice(&pixels);
        msg.extend_from_slice(&[0b1100_0000u8, 0b1000_0000]);
        stream.write_all(&msg).unwrap();
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, event_rx) = sync_channel(4);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(profile, None, cmd_rx, event_tx, None))
    });
    let mut hotspot = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && hotspot.is_none() {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Cursor { hot_x, hot_y, .. }) => hotspot = Some((hot_x, hot_y)),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = cmd_tx.send(Command::Stop);
    let result = handle.join().unwrap();
    assert!(matches!(result, Ok(())));
    assert_eq!(
        hotspot,
        Some((0, 0)),
        "negative/out-of-range hotspot must clamp"
    );
}

#[test]
fn saved_password_never_silently_downgrades_to_none_auth() {
    let port = spawn_mock(|stream| {
        write_hello(stream);
        stream.write_all(&[1, 1]).unwrap(); // only None offered
                                            // Client must reject before selecting a security type.
        std::thread::sleep(Duration::from_millis(200));
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, _event_rx) = sync_channel(4);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let result = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once(
            profile,
            Some("secret".into()),
            cmd_rx,
            event_tx,
            None,
        ))
    })
    .join()
    .unwrap();
    assert!(
        result.is_err(),
        "password + None-only server must be rejected"
    );
    assert!(
        matches!(result, Err(protocol::Error::Unsupported(_))),
        "unexpected error: {result:?}"
    );
}

#[test]
fn stalled_server_hits_read_timeout() {
    let port = spawn_mock(|stream| {
        write_hello(stream); // greeting only, then stall
        std::thread::sleep(Duration::from_secs(2));
        drain(stream);
    });
    let profile = test_profile(port);
    let (event_tx, _event_rx) = sync_channel(4);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let started = std::time::Instant::now();
    let result = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(protocol::run_once_with_timeout(
            profile,
            None,
            cmd_rx,
            event_tx,
            None,
            Duration::from_millis(300),
        ))
    })
    .join()
    .unwrap();
    assert!(
        matches!(result, Err(protocol::Error::Timeout)),
        "stalled server must time out, got {result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "timeout waited too long"
    );
}
