//! RFB 3.3/3.7/3.8 client for loopback and user-approved legacy networks.
//! Encodings: Raw, CopyRect, ZRLE (zlib, persistent per-connection stream; hard decode caps).
//! IMPORTANT: still an intentionally limited development slice, NOT the final production engine.
//! No TLS/VeNCrypt/ARD/Tight/Hextile yet. Do not claim them as implemented.
use crate::configuration::{Profile, SecurityPolicy};
use des::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};
use std::{io, sync::mpsc::SyncSender, time::Duration};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

/// 16 Mpx = 64 MiB RGBA canvas. 5K desktops still fit; the previous 64 Mpx cap
/// allowed a 256 MiB canvas plus a 192 MiB ZRLE tile stream (over MAX_ZRLE_OUTPUT)
/// and amplified the dirty-rect DoS below.
const MAX_PIXELS: usize = 16 * 1024 * 1024;
/// Hard cap on the RGBA bytes buffered from a single FramebufferUpdate. A
/// malicious server can send 16k full-frame CopyRects (4 bytes of payload each)
/// which would otherwise allocate TBs before the UI ever sees an event.
const MAX_DAMAGE_PER_UPDATE: usize = 128 * 1024 * 1024;
/// Per-read network timeout. Prevents a stalled server from pinning a worker
/// thread and its framebuffer forever; retryable, so auto-reconnect can recover.
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_NAME: u32 = 4096;
const MAX_CUTTEXT: usize = 1024 * 1024;
const MAX_ZRLE_INPUT: usize = 64 * 1024 * 1024;
const MAX_ZRLE_OUTPUT: usize = 64 * 1024 * 1024;
/// Negotiated pixel format is 32bpp little-endian true-colour, depth 24, R shift 16, G 8, B 0,
/// so a CPIXEL occupies the least-significant 3 bytes and is exactly 3 bytes for ZRLE.
const CPIXEL_LEN: usize = 3;
/// LibVNCServer (x11vnc) keyboard LED State pseudo-encoding; state is in the
/// rectangle's x-coordinate and carries no payload byte (unlike QEMU -261).
const RFB_ENCODING_LED_STATE: i32 = 0xFFFE0000u32 as i32;

#[derive(Debug, Error)]
pub enum Error {
    #[error("network I/O: {0}")]
    Io(#[from] io::Error),
    #[error("connection timed out")]
    Timeout,
    #[error("authentication refused: {0}")]
    Authentication(String),
    #[error("unsupported RFB feature: {0}")]
    Unsupported(String),
    #[error("malformed server message: {0}")]
    Protocol(String),
    #[error("unencrypted RFB is blocked; use localhost tunnel or explicitly opt in under connection settings")]
    InsecureBlocked,
    #[error("unauthenticated VNC (None) allowed only on loopback")]
    NoAuthBlocked,
}
impl Error {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Io(_) | Self::Timeout)
    }
}

#[derive(Debug, Clone)]
pub enum Command {
    Key {
        keysym: u32,
        pressed: bool,
    },
    Pointer {
        x: u16,
        y: u16,
        buttons: u8,
    },
    /// Send local clipboard text to the remote (ClientCutText, message type 6).
    CutText {
        text: String,
    },
    /// Force a full (non-incremental) FramebufferUpdateRequest — the client rebuilds
    /// its framebuffer from scratch. Used by the floating bar Refresh / Cmd+R so any
    /// permanent ghost pixels from imperfect server-side damage can always be cleared.
    Refresh,
    Stop,
}
/// One update rectangle inside a `Frame` event (see [`Event::Frame`]).
#[derive(Debug, Clone)]
pub struct DirtyRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    /// Straight (non-premultiplied) RGBA bytes, row-major, `width*height*4` bytes.
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub enum Event {
    Connected {
        name: String,
        width: u16,
        height: u16,
    },
    /// Remote clipboard text received (ServerCutText, message type 3).
    ClipboardText(String),
    /// Lock-key (LED) state from the QEMU LED State pseudo-encoding (-261).
    /// Bit 2 (0x04) = Caps Lock, bit 1 (0x02) = Num Lock, bit 0 (0x01) = Scroll Lock.
    LedState(u8),
    /// A server-sent framebuffer update, streamed as the rectangles the server actually
    /// reported (dirty rects) instead of a full framebuffer copy. `rgba` is straight RGBA,
    /// one byte per channel, covering exactly `width*height*4` bytes in row-major order.
    Frame {
        width: u16,
        height: u16,
        rects: Vec<DirtyRect>,
    },
    /// Server-side cursor shape from the Cursor pseudo-encoding (-239).
    /// width==0 means "cursor unset". Pixels are straight RGBA; hotspot is x/y of the rect header.
    Cursor {
        width: u16,
        height: u16,
        hot_x: u16,
        hot_y: u16,
        rgba: Vec<u8>,
    },
    Closed {
        message: String,
        retryable: bool,
    },
}

/// Persistent zlib stream for ZRLE; the RFB spec requires one stream per connection,
/// shared strictly in order across ZRLE rectangles.
struct ZrleState {
    decompressor: flate2::Decompress,
}
impl Default for ZrleState {
    fn default() -> Self {
        Self {
            decompressor: flate2::Decompress::new(true),
        }
    }
}

fn zrle_inflate(dec: &mut flate2::Decompress, input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(1 << 16);
    let mut in_pos = 0usize;
    loop {
        let start = out.len();
        out.resize(start + (1 << 16), 0);
        let before_in = dec.total_in();
        let before_out = dec.total_out();
        let status = dec
            .decompress(
                &input[in_pos..],
                &mut out[start..],
                flate2::FlushDecompress::None,
            )
            .map_err(|_| Error::Protocol("zrle zlib inflate failed".into()))?;
        let consumed = (dec.total_in() - before_in) as usize;
        let produced = (dec.total_out() - before_out) as usize;
        in_pos += consumed;
        out.truncate(start + produced);
        if out.len() > MAX_ZRLE_OUTPUT {
            return Err(Error::Protocol("zrle output exceeds bound".into()));
        }
        match status {
            flate2::Status::StreamEnd => {
                if in_pos == input.len() || produced > 0 {
                    break;
                }
                return Err(Error::Protocol("zrle stream ended early".into()));
            }
            _ => {
                if in_pos == input.len() && produced == 0 {
                    break;
                }
                if consumed == 0 && produced == 0 {
                    return Err(Error::Protocol("zrle inflate stalled".into()));
                }
            }
        }
    }
    Ok(out)
}

/// Run-length decode of the ZRLE 255-byte scheme: the length is one more than the sum of
/// all length bytes. A run is [255...] + final byte (<255), then +1.
fn zrle_run_len(data: &[u8], pos: &mut usize, max: usize) -> Result<usize, Error> {
    let mut sum = 0usize;
    loop {
        let byte = *data
            .get(*pos)
            .ok_or_else(|| Error::Protocol("zrle run length truncated".into()))?;
        *pos += 1;
        sum += byte as usize;
        if byte != 255 {
            break;
        }
        if sum >= max {
            return Err(Error::Protocol("zrle run too long".into()));
        }
    }
    let run = sum + 1;
    if run > max {
        return Err(Error::Protocol("zrle run too long".into()));
    }
    Ok(run)
}

fn zrle_cpixel(data: &[u8], pos: &mut usize) -> Result<[u8; 3], Error> {
    if data.len() < *pos + CPIXEL_LEN {
        return Err(Error::Protocol("zrle truncated CPIXEL".into()));
    }
    let px = [data[*pos], data[*pos + 1], data[*pos + 2]];
    *pos += CPIXEL_LEN;
    Ok(px)
}

fn zrle_put(canvas: &mut [u8], fb_w: usize, x: usize, y: usize, px: [u8; 3]) {
    let o = (y * fb_w + x) * 4;
    canvas[o] = px[2];
    canvas[o + 1] = px[1];
    canvas[o + 2] = px[0];
    canvas[o + 3] = 255;
}

/// Decode one ZRLE rectangle (encoding 16) into the canvas at (x, y, w, h).
/// `data` is the full decompressed tile stream for the rectangle. Pure function (testable).
fn zrle_decode_tiles(
    data: &[u8],
    canvas: &mut [u8],
    fb_w: usize,
    x: u16,
    y: u16,
    rw: u16,
    rh: u16,
) -> Result<(), Error> {
    let mut pos = 0usize;
    let (rx, ry) = (usize::from(x), usize::from(y));
    let (w, h) = (usize::from(rw), usize::from(rh));
    for ty in (0..h).step_by(64) {
        for tx in (0..w).step_by(64) {
            let tw = 64.min(w - tx);
            let th = 64.min(h - ty);
            let base_x = rx + tx;
            let base_y = ry + ty;
            let subenc = *data
                .get(pos)
                .ok_or_else(|| Error::Protocol("zrle truncated tile".into()))?;
            pos += 1;
            let rle = subenc & 0x80 != 0;
            let palette_size = (subenc & 0x7f) as usize;
            match (rle, palette_size) {
                (false, 0) => {
                    // Raw tile
                    for row in 0..th {
                        for col in 0..tw {
                            let px = zrle_cpixel(data, &mut pos)?;
                            zrle_put(canvas, fb_w, base_x + col, base_y + row, px);
                        }
                    }
                }
                (false, 1) => {
                    // Solid tile
                    let px = zrle_cpixel(data, &mut pos)?;
                    for row in 0..th {
                        for col in 0..tw {
                            zrle_put(canvas, fb_w, base_x + col, base_y + row, px);
                        }
                    }
                }
                (false, 2..=16) => {
                    // Packed palette: 1/2/4 bit indices, rows padded to whole bytes, MSB first
                    let mut palette = Vec::with_capacity(palette_size);
                    for _ in 0..palette_size {
                        palette.push(zrle_cpixel(data, &mut pos)?);
                    }
                    let bits = if palette_size == 2 {
                        1usize
                    } else if palette_size <= 4 {
                        2usize
                    } else {
                        4usize
                    };
                    let row_bytes = (tw * bits).div_ceil(8);
                    let mask = (1u8 << bits) - 1;
                    for row in 0..th {
                        if data.len() < pos + row_bytes {
                            return Err(Error::Protocol("zrle truncated palette row".into()));
                        }
                        let row_data = &data[pos..pos + row_bytes];
                        pos += row_bytes;
                        for col in 0..tw {
                            let bit_off = col * bits;
                            let byte_i = bit_off / 8;
                            let shift = 8 - bits - (bit_off % 8);
                            let idx = ((row_data[byte_i] >> shift) & mask) as usize;
                            let px = *palette.get(idx).ok_or_else(|| {
                                Error::Protocol("zrle palette index out of range".into())
                            })?;
                            zrle_put(canvas, fb_w, base_x + col, base_y + row, px);
                        }
                    }
                }
                (true, 0) => {
                    // Plain RLE: runs of a pixel value may continue across rows
                    let mut done = 0usize;
                    let total = tw * th;
                    while done < total {
                        let px = zrle_cpixel(data, &mut pos)?;
                        let run = zrle_run_len(data, &mut pos, total - done)?;
                        for i in 0..run {
                            let cell = done + i;
                            zrle_put(canvas, fb_w, base_x + cell % tw, base_y + cell / tw, px);
                        }
                        done += run;
                    }
                }
                (true, 2..=127) => {
                    // Palette RLE: single-index runs or extended runs (top bit set).
                    // Subencoding 129 (palette_size == 1) is reserved.
                    let mut palette = Vec::with_capacity(palette_size);
                    for _ in 0..palette_size {
                        palette.push(zrle_cpixel(data, &mut pos)?);
                    }
                    let mut done = 0usize;
                    let total = tw * th;
                    while done < total {
                        let b = *data.get(pos).ok_or_else(|| {
                            Error::Protocol("zrle truncated palette index".into())
                        })?;
                        pos += 1;
                        let idx = (b & 0x7f) as usize;
                        let px = *palette.get(idx).ok_or_else(|| {
                            Error::Protocol("zrle palette index out of range".into())
                        })?;
                        let run = if b & 0x80 != 0 {
                            zrle_run_len(data, &mut pos, total - done)?
                        } else {
                            1
                        };
                        for i in 0..run {
                            let cell = done + i;
                            zrle_put(canvas, fb_w, base_x + cell % tw, base_y + cell / tw, px);
                        }
                        done += run;
                    }
                }
                _ => {
                    return Err(Error::Protocol(
                        "zrle reserved subencoding (17..=127 without RLE, or 129)".into(),
                    ))
                }
            }
        }
    }
    if pos != data.len() {
        return Err(Error::Protocol("zrle trailing bytes in rectangle".into()));
    }
    Ok(())
}

fn canvas_len(w: u16, h: u16) -> Result<usize, Error> {
    let n = usize::from(w)
        .checked_mul(usize::from(h))
        .ok_or_else(|| Error::Protocol("dimension overflow".into()))?;
    if n == 0 || n > MAX_PIXELS {
        return Err(Error::Protocol("desktop exceeds 64 megapixels".into()));
    }
    n.checked_mul(4)
        .ok_or_else(|| Error::Protocol("buffer overflow".into()))
}

fn is_loopback(host: &str) -> bool {
    matches!(
        host.trim_matches(&['[', ']'][..]),
        "localhost" | "127.0.0.1" | "::1"
    )
}

fn vnc_challenge(challenge: &[u8; 16], password: &str) -> [u8; 16] {
    // RFC 6143 requires DES with reversed bits of each of the first eight password bytes.
    // Legacy VNC auth protects the challenge, NOT framebuffer confidentiality.
    let mut key = [0u8; 8];
    for (i, &b) in password.as_bytes().iter().take(8).enumerate() {
        key[i] = b.reverse_bits();
    }
    let cipher = des::Des::new_from_slice(&key).expect("DES key length is 8");
    let mut out = *challenge;
    for part in out.as_chunks_mut::<8>().0 {
        let mut block = GenericArray::clone_from_slice(part);
        cipher.encrypt_block(&mut block);
        part.copy_from_slice(&block);
    }
    out
}
/// CutText payload: try UTF-8 first (TigerVNC and other modern implementations),
/// fall back to ISO-8859-1 (RFC 6143 mandates Latin-1 as the wire text encoding).
fn decode_cuttext(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

async fn auth(
    stream: &mut TcpStream,
    version: u16,
    password: Option<&str>,
    allow_none: bool,
) -> Result<(), Error> {
    let sec_type: u32;
    if version == 3 {
        sec_type = stream.read_u32().await?;
    } else {
        let n = stream.read_u8().await?;
        if n == 0 {
            let reason = read_reason(stream).await?;
            return Err(Error::Authentication(reason));
        }
        if n > 64 {
            return Err(Error::Protocol("too many security methods".into()));
        }
        let mut offered = vec![0u8; n as usize];
        stream.read_exact(&mut offered).await?;
        sec_type = if password.is_some() && offered.contains(&2) {
            2
        } else if allow_none && offered.contains(&1) {
            // A saved password means the user expects authentication. Never silently
            // downgrade to None even on loopback (SECURITY.md / ADR-0002).
            if password.is_some() && !offered.contains(&2) {
                return Err(Error::Unsupported(
                    "server does not offer VNC Authentication for the saved password".into(),
                ));
            }
            1
        } else {
            return Err(Error::Unsupported(format!("security types: {offered:?}")));
        };
        stream.write_u8(sec_type as u8).await?;
    }
    match sec_type {
        1 => {
            if !allow_none {
                return Err(Error::NoAuthBlocked);
            }
        }
        2 => {
            let pass =
                password.ok_or_else(|| Error::Authentication("VNC password not saved".into()))?;
            let mut challenge = [0u8; 16];
            stream.read_exact(&mut challenge).await?;
            stream.write_all(&vnc_challenge(&challenge, pass)).await?;
        }
        0 => {
            return Err(Error::Authentication(read_reason(stream).await?));
        }
        x => return Err(Error::Unsupported(format!("security method {x}"))),
    }
    if version >= 8 || sec_type == 2 {
        let status = stream.read_u32().await?;
        if status != 0 {
            let reason = if version >= 8 {
                read_reason(stream)
                    .await
                    .unwrap_or_else(|_| "authentication failed".into())
            } else {
                "authentication rejected".into()
            };
            return Err(Error::Authentication(reason));
        }
    }
    Ok(())
}
async fn read_reason(stream: &mut TcpStream) -> Result<String, Error> {
    let n = stream.read_u32().await?;
    if n > 4096 {
        return Err(Error::Protocol("oversized reason".into()));
    }
    let mut b = vec![0; n as usize];
    stream.read_exact(&mut b).await?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}
async fn request_update(
    stream: &mut TcpStream,
    w: u16,
    h: u16,
    incremental: bool,
) -> Result<(), Error> {
    stream.write_u8(3).await?;
    stream.write_u8(u8::from(incremental)).await?;
    stream.write_u16(0).await?;
    stream.write_u16(0).await?;
    stream.write_u16(w).await?;
    stream.write_u16(h).await?;
    Ok(())
}
async fn send_command(stream: &mut TcpStream, c: Command) -> Result<bool, Error> {
    match c {
        Command::Key { keysym, pressed } => {
            stream.write_u8(4).await?;
            stream.write_u8(u8::from(pressed)).await?;
            stream.write_u16(0).await?;
            stream.write_u32(keysym).await?;
        }
        Command::Pointer { x, y, buttons } => {
            stream.write_u8(5).await?;
            stream.write_u8(buttons).await?;
            stream.write_u16(x).await?;
            stream.write_u16(y).await?;
        }
        Command::CutText { text } => {
            let bytes = text.as_bytes();
            if bytes.len() > MAX_CUTTEXT {
                return Ok(true); // Skip oversized clipboard payloads silently (never log contents).
            }
            stream.write_u8(6).await?;
            stream.write_all(&[0, 0, 0]).await?;
            stream.write_u32(bytes.len() as u32).await?;
            stream.write_all(bytes).await?;
        }
        Command::Stop => return Ok(false),
        Command::Refresh => {} // Handled by the caller, which owns the fb dimensions.
    }
    Ok(true)
}

fn send_event(events: &SyncSender<Event>, wake: &Option<Wake>, e: Event) -> bool {
    if events.send(e).is_ok() {
        if let Some(w) = wake {
            w();
        }
        true
    } else {
        false
    }
}

fn extract_dirty_rect(canvas: &[u8], fb_w: usize, x: u16, y: u16, rw: u16, rh: u16) -> DirtyRect {
    let dst_w = fb_w;
    let mut out = Vec::with_capacity(usize::from(rw) * usize::from(rh) * 4);
    let src_stride = usize::from(rw) * 4;
    for row in 0..usize::from(rh) {
        let src_start = ((usize::from(y) + row) * dst_w + usize::from(x)) * 4;
        out.extend_from_slice(&canvas[src_start..src_start + src_stride]);
    }
    DirtyRect {
        x,
        y,
        width: rw,
        height: rh,
        rgba: out,
    }
}

/// Accumulate one dirty rect, enforcing a per-update RGBA budget BEFORE the
/// allocation in `extract_dirty_rect` happens.
fn push_damage(
    canvas: &[u8],
    fb_w: usize,
    damage: &mut Vec<DirtyRect>,
    budget: &mut usize,
    rect: (u16, u16, u16, u16),
) -> Result<(), Error> {
    push_damage_budgeted(canvas, fb_w, damage, budget, MAX_DAMAGE_PER_UPDATE, rect)
}

fn push_damage_budgeted(
    canvas: &[u8],
    fb_w: usize,
    damage: &mut Vec<DirtyRect>,
    budget: &mut usize,
    cap: usize,
    rect: (u16, u16, u16, u16),
) -> Result<(), Error> {
    let (x, y, rw, rh) = rect;
    let bytes = usize::from(rw)
        .checked_mul(usize::from(rh))
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| Error::Protocol("dimension overflow".into()))?;
    let total = budget
        .checked_add(bytes)
        .ok_or_else(|| Error::Protocol("damage budget overflow".into()))?;
    if total > cap {
        return Err(Error::Protocol("damage budget exceeded".into()));
    }
    *budget = total;
    damage.push(extract_dirty_rect(canvas, fb_w, x, y, rw, rh));
    Ok(())
}

/// Flush queued client commands mid-update (between rectangles) so input never
/// waits for a whole screen re-encode to finish. Returns Ok(false) when the
/// client requested disconnect (or the channel closed).
async fn flush_commands(
    stream: &mut TcpStream,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    fb: (u16, u16),
) -> Result<bool, Error> {
    loop {
        match commands.try_recv() {
            Ok(Command::Refresh) => {
                request_update(stream, fb.0, fb.1, false).await?;
            }
            Ok(cmd) => {
                if !send_command(stream, cmd).await? {
                    return Ok(false);
                }
            }
            Err(mpsc::error::TryRecvError::Empty) => return Ok(true),
            Err(mpsc::error::TryRecvError::Disconnected) => return Ok(false),
        }
    }
}

/// Per-update encoding tally (counts only; never pixel data). Used by the
/// GLINTVNC_DEBUG_FRAMES diagnostic to tell at a glance whether the server is
/// using CopyRect (window moves) or re-encoding everything (e.g. compositor on).
#[derive(Default, Debug, Clone, Copy)]
pub struct EncHistogram {
    pub raw: u64,
    pub copyrect: u64,
    pub zrle: u64,
    pub desktopsize: u64,
    pub cursor: u64,
    pub led: u64,
}

/// Inputs and outputs threaded through a single FramebufferUpdate pass. Bundling
/// them keeps the decoder signature small; see `read_update`.
struct UpdatePass<'a> {
    w: &'a mut u16,
    h: &'a mut u16,
    rgba: &'a mut Vec<u8>,
    extra_events: &'a mut Vec<Event>,
    damage: &'a mut Vec<DirtyRect>,
    zrle: &'a mut ZrleState,
    commands: &'a mut mpsc::UnboundedReceiver<Command>,
    hist: &'a mut EncHistogram,
    /// Set by DesktopSize: the spec requires the next request to be non-incremental
    /// when the client discards (or resizes) its framebuffer.
    needs_full_refresh: &'a mut bool,
    /// RGBA bytes accumulated in this update, bounded by MAX_DAMAGE_PER_UPDATE.
    damage_bytes: &'a mut usize,
}

async fn read_update(stream: &mut TcpStream, up: &mut UpdatePass<'_>) -> Result<bool, Error> {
    let w = &mut *up.w;
    let h = &mut *up.h;
    let rgba = &mut *up.rgba;
    let extra_events = &mut *up.extra_events;
    let damage = &mut *up.damage;
    let zrle = &mut *up.zrle;
    let commands = &mut *up.commands;
    let hist = &mut *up.hist;
    let needs_full_refresh = &mut *up.needs_full_refresh;
    let damage_bytes = &mut *up.damage_bytes;
    let _padding = stream.read_u8().await?;
    let count = stream.read_u16().await?;
    if count > 16384 {
        return Err(Error::Protocol("too many rectangles".into()));
    }
    for _ in 0..count {
        // Input is dispatched between rectangles, not after the whole
        // update: pointer events reach the server even while a large
        // framebuffer re-encode is being transferred.
        if !flush_commands(stream, commands, (*w, *h)).await? {
            return Ok(true);
        }
        let x = stream.read_u16().await?;
        let y = stream.read_u16().await?;
        let rw = stream.read_u16().await?;
        let rh = stream.read_u16().await?;
        let enc = stream.read_i32().await?;
        if enc == -223 {
            // DesktopSize pseudo-encoding
            hist.desktopsize += 1;
            *w = rw;
            *h = rh;
            *rgba = vec![0u8; canvas_len(*w, *h)?];
            damage.clear();
            *damage_bytes = 0;
            *needs_full_refresh = true;
            push_damage(rgba, usize::from(*w), damage, damage_bytes, (0, 0, *w, *h))?;
            continue;
        }
        if enc == -261 {
            // QEMU LED State pseudo-encoding: one byte, bit0=Scroll, bit1=Num, bit2=Caps.
            hist.led += 1;
            let state = stream.read_u8().await?;
            extra_events.push(Event::LedState(state));
            continue;
        }
        if enc == RFB_ENCODING_LED_STATE {
            // LibVNCServer/x11vnc: bit0 Scroll, bit1 Num, bit2 Caps, in rect x.
            hist.led += 1;
            extra_events.push(Event::LedState(x as u8));
            continue;
        }
        if enc == -239 {
            // Cursor pseudo-encoding (RFC 6143 §"Cursor Pseudo-encoding"):
            hist.cursor += 1;
            // x/y = hotspot, rw/rh = cursor size. Data = w*h PIXELs in the
            // negotiated format followed by a 1bpp bitmask (MSB first, padded to whole bytes
            // per scanline). A zero width/height means "cursor unset" and carries no data.
            if rw == 0 || rh == 0 {
                extra_events.push(Event::Cursor {
                    width: 0,
                    height: 0,
                    hot_x: 0,
                    hot_y: 0,
                    rgba: Vec::new(),
                });
                continue;
            }
            let bytes = canvas_len(rw, rh)?; // also enforces w*h*4 bounds
            let mut raw = vec![0u8; bytes];
            stream.read_exact(&mut raw).await?;
            let mask_stride = usize::from(rw).div_ceil(8);
            let mut mask = vec![0u8; mask_stride * usize::from(rh)];
            stream.read_exact(&mut mask).await?;
            let mut out = vec![0u8; bytes];
            for row in 0..usize::from(rh) {
                for col in 0..usize::from(rw) {
                    let src = (row * usize::from(rw) + col) * 4;
                    let bit = (mask[row * mask_stride + col / 8] >> (7 - (col % 8))) & 1;
                    let dst = src;
                    out[dst] = raw[src + 2];
                    out[dst + 1] = raw[src + 1];
                    out[dst + 2] = raw[src];
                    out[dst + 3] = if bit == 1 { 255 } else { 0 };
                }
            }
            // RFC rect fields are U16, but TigerVNC/RealVNC encode negative
            // hotspots as S16 (e.g. -1 -> 0xffff). egui/winit require the hotspot
            // to lie inside the bitmap, so interpret as signed then clamp.
            let clamp_hot = |v: u16, size: u16| -> u16 {
                if v as i16 <= 0 {
                    0
                } else {
                    v.min(size.saturating_sub(1))
                }
            };
            extra_events.push(Event::Cursor {
                width: rw,
                height: rh,
                hot_x: clamp_hot(x, rw),
                hot_y: clamp_hot(y, rh),
                rgba: out,
            });
            continue;
        }
        if enc == 16 {
            // ZRLE encoding: U32 length + zlib stream (persistent stream per connection).
            hist.zrle += 1;
            if usize::from(x) + usize::from(rw) > usize::from(*w)
                || usize::from(y) + usize::from(rh) > usize::from(*h)
            {
                return Err(Error::Protocol("rectangle outside framebuffer".into()));
            }
            let zlen = stream.read_u32().await? as usize;
            if zlen > MAX_ZRLE_INPUT {
                return Err(Error::Protocol(
                    "zrle rectangle length exceeds bound".into(),
                ));
            }
            let mut compressed = vec![0u8; zlen];
            stream.read_exact(&mut compressed).await?;
            let raw = zrle_inflate(&mut zrle.decompressor, &compressed)?;
            let fb_w = usize::from(*w);
            zrle_decode_tiles(&raw, rgba, fb_w, x, y, rw, rh)?;
            push_damage(rgba, fb_w, damage, damage_bytes, (x, y, rw, rh))?;
            continue;
        }
        if enc == 1 {
            // CopyRect: copy an existing framebuffer region into this rectangle.
            hist.copyrect += 1;
            let sx = stream.read_u16().await?;
            let sy = stream.read_u16().await?;
            if usize::from(x) + usize::from(rw) > usize::from(*w)
                || usize::from(y) + usize::from(rh) > usize::from(*h)
            {
                return Err(Error::Protocol("rectangle outside framebuffer".into()));
            }
            if usize::from(sx) + usize::from(rw) > usize::from(*w)
                || usize::from(sy) + usize::from(rh) > usize::from(*h)
            {
                return Err(Error::Protocol("copy source outside framebuffer".into()));
            }
            let stride = usize::from(rw) * 4;
            let dst_w = usize::from(*w);
            // Row-by-row via a small temporary: correct for overlapping regions.
            for row in 0..usize::from(rh) {
                let src_start = ((usize::from(sy) + row) * dst_w + usize::from(sx)) * 4;
                let tmp = rgba[src_start..src_start + stride].to_vec();
                let dst_start = ((usize::from(y) + row) * dst_w + usize::from(x)) * 4;
                rgba[dst_start..dst_start + stride].copy_from_slice(&tmp);
            }
            push_damage(rgba, usize::from(*w), damage, damage_bytes, (x, y, rw, rh))?;
            continue;
        }
        if enc != 0 {
            return Err(Error::Unsupported(format!(
                "server used unadvertised encoding {enc}"
            )));
        }
        if usize::from(x) + usize::from(rw) > usize::from(*w)
            || usize::from(y) + usize::from(rh) > usize::from(*h)
        {
            return Err(Error::Protocol("rectangle outside framebuffer".into()));
        }
        hist.raw += 1;
        let bytes = canvas_len(rw, rh)?;
        let mut rect = vec![0u8; bytes];
        stream.read_exact(&mut rect).await?;
        // pixel format requested: 32bpp LE, R shift 16, G 8, B 0.
        // Row-wise slice copy: avoids per-pixel bounds checks and multiplies.
        let src_stride = usize::from(rw) * 4;
        let dst_w = usize::from(*w);
        for row in 0..usize::from(rh) {
            let src_row = &rect[row * src_stride..(row + 1) * src_stride];
            let dst_start = ((usize::from(y) + row) * dst_w + usize::from(x)) * 4;
            let dst_row = &mut rgba[dst_start..dst_start + src_stride];
            for (s, d) in src_row
                .as_chunks::<4>()
                .0
                .iter()
                .zip(dst_row.as_chunks_mut::<4>().0.iter_mut())
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 255;
            }
        }
        push_damage(rgba, usize::from(*w), damage, damage_bytes, (x, y, rw, rh))?;
    }
    Ok(false)
}

/// Optional dirty-rectangle stats (sizes and encoding counts only, never pixel data).
fn debug_dump_damage(damage: &[DirtyRect], hist: &EncHistogram) {
    if std::env::var_os("GLINTVNC_DEBUG_FRAMES").is_some() {
        let bytes: usize = damage.iter().map(|r| r.rgba.len()).sum();
        eprintln!(
            "glintvnc: frame rects={} bytes={bytes} raw={} copyrect={} zrle={} desktopsize={} cursor={} led={}",
            damage.len(), hist.raw, hist.copyrect, hist.zrle, hist.desktopsize, hist.cursor, hist.led
        );
    }
}

/// Wake callback invoked after an event is handed to the app (e.g. `ctx.request_repaint()`).
/// Kept as a plain function pointer so this crate does not depend on any UI toolkit.
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;

pub async fn run_once(
    profile: Profile,
    password: Option<String>,
    commands: mpsc::UnboundedReceiver<Command>,
    events: SyncSender<Event>,
    wake: Option<Wake>,
) -> Result<(), Error> {
    run_once_with_timeout(profile, password, commands, events, wake, READ_TIMEOUT).await
}

/// Same as [`run_once`] but with an explicit inactivity timeout. Used by the
/// test suite to exercise stalled servers without waiting 15 seconds.
pub async fn run_once_with_timeout(
    profile: Profile,
    password: Option<String>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: SyncSender<Event>,
    wake: Option<Wake>,
    read_timeout: Duration,
) -> Result<(), Error> {
    profile.validate().map_err(Error::Protocol)?;
    if profile.security == SecurityPolicy::LocalhostOnly && !is_loopback(&profile.host) {
        return Err(Error::InsecureBlocked);
    }
    // Only connect over loopback by default. An external verified SSH/WireGuard tunnel
    // is outside this milestone. Explicit opt-in must be visible in the GUI.
    let mut stream = tokio::time::timeout(
        Duration::from_secs(8),
        TcpStream::connect((profile.host.as_str(), profile.port)),
    )
    .await
    .map_err(|_| Error::Timeout)??;
    let _ = stream.set_nodelay(true);
    let (mut w, mut h, name, mut rgba) = tokio::time::timeout(read_timeout, async {
        let mut greeting = [0u8; 12];
        stream.read_exact(&mut greeting).await?;
        if &greeting[0..4] != b"RFB " || greeting[7] != b'.' || greeting[11] != b'\n' {
            return Err(Error::Protocol("invalid server greeting".into()));
        }
        let remote_major = std::str::from_utf8(&greeting[4..7]).unwrap_or("000");
        let remote_minor = std::str::from_utf8(&greeting[8..11]).unwrap_or("000");
        if remote_major != "003" {
            return Err(Error::Unsupported("RFB major version".into()));
        }
        let minor: u16 = remote_minor
            .parse()
            .map_err(|_| Error::Protocol("minor version".into()))?;
        // RFC 6143 §6: unknown 3.x minor versions must be treated as 3.3.
        let negotiated = if minor == 8 {
            8
        } else if minor == 7 {
            7
        } else {
            3
        };
        stream
            .write_all(format!("RFB 003.{negotiated:03}\n").as_bytes())
            .await?;
        auth(
            &mut stream,
            negotiated,
            password.as_deref(),
            is_loopback(&profile.host),
        )
        .await?;
        stream.write_u8(1).await?; // Shared desktop
        let w = stream.read_u16().await?;
        let h = stream.read_u16().await?;
        let mut pf = [0u8; 16];
        stream.read_exact(&mut pf).await?;
        let namelen = stream.read_u32().await?;
        if namelen > MAX_NAME {
            return Err(Error::Protocol("desktop name is too long".into()));
        }
        let mut namebuf = vec![0; namelen as usize];
        stream.read_exact(&mut namebuf).await?;
        let name = String::from_utf8_lossy(&namebuf).into_owned();
        let rgba = vec![0u8; canvas_len(w, h)?];
        // SetPixelFormat: 32-bit, true-color LE, max R/G/B=255, shifts 16/8/0.
        stream
            .write_all(&[
                0, 0, 0, 0, 32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0,
            ])
            .await?;
        // SetEncodings: Raw (0), CopyRect (1), ZRLE (16), DesktopSize (-223), Cursor (-239), LED State (-261).
        // Cursor is what makes the server stop painting its own cursor into the framebuffer
        // and send the shape separately instead. LED State tells the client the server-side
        // Caps/Num/Scroll lock state (bit2/bit1/bit0 of a 1-byte pseudo-rectangle) — this is
        // also what lets TigerVNC honour 0xffe5 keysyms at all. ZRLE compresses damaged
        // rectangles (zlib + tiles + palette runs) so drags stay fluid on 100M+ links.
        // TigerVNC/Xtigervnc scan this list in order and use the first encoding they support,
        // so ZRLE goes first (lossless, high ratio). CopyRect is handled by the server
        // independently of this list, but keep it advertised as usual. Raw is the fallback.
        stream.write_u8(2).await?;
        stream.write_u8(0).await?;
        stream.write_u16(8).await?;
        stream.write_i32(16).await?;
        stream.write_i32(1).await?;
        stream.write_i32(0).await?;
        stream.write_i32(-223).await?;
        stream.write_i32(-239).await?;
        stream.write_i32(-261).await?;
        stream.write_i32(RFB_ENCODING_LED_STATE).await?;
        stream.write_i32(-255).await?;
        request_update(&mut stream, w, h, false).await?;
        Ok::<(u16, u16, String, Vec<u8>), Error>((w, h, name, rgba))
    })
    .await
    .map_err(|_| Error::Timeout)??;

    let _ = send_event(
        &events,
        &wake,
        Event::Connected {
            name,
            width: w,
            height: h,
        },
    );
    let mut zrle = ZrleState::default();
    let mut needs_full_refresh = false;
    loop {
        tokio::select! {
            next=commands.recv()=> {
                match next {
                    Some(Command::Refresh) => {
                        request_update(&mut stream, w, h, false).await?;
                    }
                    Some(cmd) => {
                        if !send_command(&mut stream, cmd).await? {
                            break;
                        }
                    }
                    None => break,
                }
            }
            message=tokio::time::timeout(read_timeout, stream.read_u8())=> {
                let msg_type = match message {
                    Ok(Ok(t))=>t,
                    // Any transport EOF (clean or mid-message) is retryable when the
                    // profile allows auto-reconnect; auth/protocol errors stay terminal.
                    Ok(Err(e))=>return Err(Error::Io(e)),
                    Err(_)=>return Err(Error::Timeout),
                };
                match msg_type {
                    0=>{
                        let mut extra_events = Vec::new();
                        let mut damage = Vec::new();
                        let mut damage_bytes = 0usize;
                        let mut hist = EncHistogram::default();
                        let mut pass = UpdatePass {
                            w: &mut w,
                            h: &mut h,
                            rgba: &mut rgba,
                            extra_events: &mut extra_events,
                            damage: &mut damage,
                            zrle: &mut zrle,
                            commands: &mut commands,
                            hist: &mut hist,
                            needs_full_refresh: &mut needs_full_refresh,
                            damage_bytes: &mut damage_bytes,
                        };
                        if tokio::time::timeout(read_timeout, read_update(&mut stream, &mut pass))
                            .await
                            .map_err(|_| Error::Timeout)??
                        {
                            break;
                        }
                        for ev in extra_events {
                            if !send_event(&events,&wake,ev) {break;}
                        }
                        debug_dump_damage(&damage, &hist);
                        // Stream only the rectangles the server actually reported instead of
                        // cloning the whole framebuffer per update (dirty-rect P3).
                        if !send_event(&events,&wake,Event::Frame{width:w,height:h,rects:damage}) {break;}
                        // DesktopSize discarded the framebuffer: RFC 6143 requires
                        // incremental=0 on the next request in that case.
                        request_update(&mut stream,w,h,!needs_full_refresh).await?;
                        needs_full_refresh=false;
                    }
                    2=>{/* Bell: no payload */}
                    3=>{ // ServerCutText; the payload is never logged.
                        let sent = tokio::time::timeout(read_timeout, async {
                            let mut pad=[0u8;3];stream.read_exact(&mut pad).await?;
                            let n=stream.read_u32().await?;
                            if n as usize > MAX_CUTTEXT {return Err(Error::Protocol("clipboard too large".into()));}
                            let mut payload=vec![0;n as usize];stream.read_exact(&mut payload).await?;
                            Ok::<bool, Error>(send_event(&events,&wake,Event::ClipboardText(decode_cuttext(&payload))))
                        }).await.map_err(|_| Error::Timeout)??;
                        if !sent {break;}
                    }
                    other=>return Err(Error::Unsupported(format!("server message type {other}")))
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bound_images() {
        assert!(canvas_len(0, 100).is_err());
        assert_eq!(canvas_len(1920, 1080).unwrap(), 1920 * 1080 * 4);
    }
    #[test]
    fn damage_budget_is_enforced_before_allocation() {
        let canvas = vec![0u8; 8 * 4 * 4];
        let mut damage = Vec::new();
        let mut budget = 0usize;
        push_damage_budgeted(&canvas, 8, &mut damage, &mut budget, 128, (0, 0, 8, 4)).unwrap();
        assert_eq!(budget, 128);
        assert!(
            push_damage_budgeted(&canvas, 8, &mut damage, &mut budget, 128, (0, 0, 8, 4)).is_err()
        );
        assert_eq!(damage.len(), 1, "rejected rect must not be buffered");
    }
    #[test]
    fn loopback_only_by_default() {
        assert!(is_loopback("localhost"));
        assert!(is_loopback("::1"));
        assert!(!is_loopback("example.com"));
    }
    #[test]
    fn challenge_is_deterministic() {
        let c = [0x12; 16];
        assert_eq!(vnc_challenge(&c, "hello"), vnc_challenge(&c, "hello"));
    }
    fn zrle_fb(w: usize, h: usize) -> Vec<u8> {
        vec![0u8; w * h * 4]
    }
    #[test]
    fn zrle_solid_tile_decodes() {
        // 4x2 solid tile, CPIXEL (B=10, G=20, R=30)
        let mut canvas = zrle_fb(4, 2);
        let data = [1u8, 10, 20, 30];
        zrle_decode_tiles(&data, &mut canvas, 4, 0, 0, 4, 2).unwrap();
        for o in canvas.as_chunks::<4>().0 {
            assert_eq!(o, &[30, 20, 10, 255]);
        }
    }
    #[test]
    fn zrle_raw_tile_decodes() {
        // 2x2 raw tile with distinct pixels (CPIXEL B, G, R)
        let mut canvas = zrle_fb(2, 2);
        let data = [
            0u8, // subenc 0 = raw
            255, 0, 0, // px0 red-ish (R=0? careful: B=255,G=0,R=0 -> blue)
            0, 255, 0, // green
            0, 0, 255, // blue
            7, 8, 9,
        ];
        zrle_decode_tiles(&data, &mut canvas, 2, 0, 0, 2, 2).unwrap();
        assert_eq!(&canvas[0..4], &[0, 0, 255, 255]); // cpix B=255 -> RGBA R=0,G=0,B=255
        assert_eq!(&canvas[4..8], &[0, 255, 0, 255]);
        assert_eq!(&canvas[8..12], &[255, 0, 0, 255]);
        assert_eq!(&canvas[12..16], &[9, 8, 7, 255]);
    }
    #[test]
    fn zrle_packed_palette_decodes() {
        // 4x2 packed palette, 4 entries, 2 bits per pixel
        let mut canvas = zrle_fb(4, 2);
        let mut data = vec![4u8];
        data.extend_from_slice(&[1, 0, 0]); // palette[0] -> red
        data.extend_from_slice(&[0, 1, 0]); // palette[1] -> green
        data.extend_from_slice(&[0, 0, 1]); // palette[2] -> blue
        data.extend_from_slice(&[9, 9, 9]); // palette[3] -> grey
        data.push(0b0001_1011); // row0: 0,1,2,3
        data.push(0b1110_0100); // row1: 3,2,1,0
        zrle_decode_tiles(&data, &mut canvas, 4, 0, 0, 4, 2).unwrap();
        assert_eq!(&canvas[0..4], &[0, 0, 1, 255]); // palette[0] RGBA (B,G,R=1,0,0)
        assert_eq!(&canvas[4..8], &[0, 1, 0, 255]);
        assert_eq!(&canvas[8..12], &[1, 0, 0, 255]);
        assert_eq!(&canvas[12..16], &[9, 9, 9, 255]);
        assert_eq!(&canvas[16..20], &[9, 9, 9, 255]);
        assert_eq!(&canvas[20..24], &[1, 0, 0, 255]);
        assert_eq!(&canvas[24..28], &[0, 1, 0, 255]);
        assert_eq!(&canvas[28..32], &[0, 0, 1, 255]);
    }
    #[test]
    fn zrle_plain_rle_run_decodes() {
        // 6x2 tile as plain RLE (subenc 128): pixel A x8, pixel B x4 (runs cross rows)
        let mut canvas = zrle_fb(6, 2);
        let mut data = vec![128u8];
        data.extend_from_slice(&[1, 2, 3]); // A
        data.push(7); // run = 7+1 = 8
        data.extend_from_slice(&[4, 5, 6]); // B
        data.push(3); // run = 3+1 = 4
        zrle_decode_tiles(&data, &mut canvas, 6, 0, 0, 6, 2).unwrap();
        assert_eq!(&canvas[0..4], &[3, 2, 1, 255]);
        let o = (6 + 2) * 4;
        assert_eq!(&canvas[o..o + 4], &[6, 5, 4, 255]);
    }
    #[test]
    fn zrle_plain_rle_long_run_decodes() {
        // run length 511 (bytes 255,255,0 -> 255+255+0+1 = 511); must fit one 64x8 tile
        let mut canvas = zrle_fb(64, 8);
        let mut data = vec![128u8];
        data.extend_from_slice(&[9, 8, 7]);
        data.extend_from_slice(&[255, 255, 0]); // A x511
        data.extend_from_slice(&[1, 2, 3]);
        data.push(0); // B x1
        zrle_decode_tiles(&data, &mut canvas, 64, 0, 0, 64, 8).unwrap();
        assert_eq!(&canvas[0..4], &[7, 8, 9, 255]);
        let last = (64 * 8 - 1) * 4;
        assert_eq!(&canvas[last..last + 4], &[3, 2, 1, 255]);
    }
    #[test]
    fn zrle_palette_rle_run_decodes() {
        // 2x2 tile, palette RLE (subenc 130): index 1 x3, index 0 x1
        let mut canvas = zrle_fb(2, 2);
        let mut data = vec![0x82u8]; // palette_size 2, RLE
        data.extend_from_slice(&[1, 0, 0]); // palette[0] red
        data.extend_from_slice(&[0, 0, 1]); // palette[1] blue
        data.push(0x81); // index 1 with top bit -> extended run
        data.push(2); // run = 2+1 = 3
        data.push(0x00); // index 0, run 1
        zrle_decode_tiles(&data, &mut canvas, 2, 0, 0, 2, 2).unwrap();
        for i in 0..3 {
            assert_eq!(&canvas[i * 4..i * 4 + 4], &[1, 0, 0, 255]); // palette[1] RGBA (B,G,R = 0,0,1)
        }
        assert_eq!(&canvas[12..16], &[0, 0, 1, 255]); // palette[0] RGBA (B,G,R=1,0,0)
    }
    #[test]
    fn zrle_reserved_subencoding_errors() {
        let mut canvas = zrle_fb(2, 2);
        let data = [17u8];
        assert!(zrle_decode_tiles(&data, &mut canvas, 2, 0, 0, 2, 2).is_err());
    }
    #[test]
    fn zrle_trailing_bytes_error() {
        let mut canvas = zrle_fb(1, 1);
        let data = [1u8, 1, 2, 3, 0]; // solid tile + 1 trailing byte
        assert!(zrle_decode_tiles(&data, &mut canvas, 1, 0, 0, 1, 1).is_err());
    }
    #[test]
    fn zrle_inflate_roundtrip() {
        use flate2::{write::ZlibEncoder, Compression};
        use std::io::Write;
        let payload = [7u8, 8, 9, 10, 11];
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&payload).unwrap();
        let compressed = enc.finish().unwrap();
        let mut dec = flate2::Decompress::new(true);
        let out = zrle_inflate(&mut dec, &compressed).unwrap();
        assert_eq!(out, payload);
    }
}
