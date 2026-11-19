#!/usr/bin/env python3
"""LOCAL-ONLY controlled RFB 3.8 None-auth test server.

Usage: python3 tests/mock_rfb.py --port 5999 --width 800 --height 450
This intentionally has no password, only binds 127.0.0.1, and is not a real desktop server.
Never modify it to listen on 0.0.0.0 or expose it to untrusted clients.
"""
import argparse
import socket
import struct
import time


def read_exact(sock, size):
    result = bytearray()
    while len(result) < size:
        chunk = sock.recv(size - len(result))
        if not chunk:
            raise EOFError('client disconnected')
        result.extend(chunk)
    return bytes(result)


def pixels(w, h):
    # Requested pixel format 32 bpp, little-endian, Rshift16, Gshift8, Bshift0.
    # Horizontal diagnostic stripes: red / green / blue / white.
    rows = []
    colors = [(0, 0, 255, 0), (0, 255, 0, 0), (255, 0, 0, 0), (255, 255, 255, 0)]
    for y in range(h):
        color = bytes(colors[min(3, y * 4 // h)])
        rows.append(color * w)
    return b''.join(rows)


def serve_one(sock, w, h, disconnect_after, send_cuttext=None, send_cursor=False, led_state=None, log_pointer=False):
    sock.settimeout(20)
    sock.sendall(b'RFB 003.008\n')
    version = read_exact(sock, 12)
    if version != b'RFB 003.008\n':
        raise RuntimeError(f'unexpected version: {version!r}')
    sock.sendall(b'\x01\x01')  # one security type: None
    if read_exact(sock, 1) != b'\x01':
        raise RuntimeError('client chose unexpected security')
    sock.sendall(struct.pack('!I', 0))
    read_exact(sock, 1)  # Shared flag
    fmt = bytes([32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0])
    name = b'GlintVNC localhost diagnostic'
    sock.sendall(struct.pack('!HH', w, h) + fmt + struct.pack('!I', len(name)) + name)
    frames = 0
    last_render = pixels(w, h)
    if send_cuttext is not None:
        payload = send_cuttext.encode('utf-8')
        sock.sendall(bytes([3, 0, 0, 0]) + struct.pack('!I', len(payload)) + payload)
        print(f'SENT_CUTTEXT: {send_cuttext!r}', flush=True)
    if send_cursor:
        cursor_pixels = bytes([10,20,30,0, 40,50,60,0, 70,80,90,0, 100,110,120,0])
        mask = bytes([0b11000000, 0b10000000])
        rect = struct.pack('!HHHH i', 1, 1, 2, 2, -239)
        sock.sendall(bytes([0, 0]) + struct.pack('!H', 1) + rect + cursor_pixels + mask)
        print('SENT_CURSOR', flush=True)
    if led_state is not None:
        rect = struct.pack('!HHHH i', 0, 0, 0, 0, -261) + bytes([led_state & 0xff])
        sock.sendall(bytes([0, 0]) + struct.pack('!H', 1) + rect)
        print(f'SENT_LEDSTATE: {led_state & 0xff}', flush=True)
    start = time.monotonic()
    while True:
        t = read_exact(sock, 1)[0]
        if t == 0:  # SetPixelFormat
            payload = read_exact(sock, 19)
            if payload[3:7] != bytes([32, 24, 0, 1]):
                raise RuntimeError(f'unsupported requested pixel format: {payload!r}')
        elif t == 2:  # SetEncodings
            header = read_exact(sock, 3)
            n = struct.unpack('!H', header[1:3])[0]
            if n > 128:
                raise RuntimeError('excessive encodings')
            read_exact(sock, 4*n)
        elif t == 3:  # FramebufferUpdateRequest, 9 more bytes
            read_exact(sock, 9)
            header = bytes([0,0]) + struct.pack('!H', 1)
            rect = struct.pack('!HHHHi', 0,0,w,h,0)
            sock.sendall(header + rect + last_render)
            frames += 1
            if disconnect_after and frames >= disconnect_after:
                raise EOFError('requested disconnect after frames')
            # Do not turn the mock into a high-cadence stress test by default.
            time.sleep(0.15)
        elif t == 4:  # KeyEvent
            read_exact(sock, 7)
        elif t == 5:  # PointerEvent
            payload = read_exact(sock, 5)
            if log_pointer:
                x, y, mask = struct.unpack('!HHB', payload)
                print(f'POINTER: x={x} y={y} mask=0x{mask:02x}', flush=True)
        elif t == 6:  # ClientCutText
            header = read_exact(sock, 7)
            n = struct.unpack('!I', header[3:7])[0]
            if n > 1_048_576:
                raise RuntimeError('clipboard too large')
            payload = read_exact(sock, n)
            try:
                text = payload.decode('utf-8')
            except UnicodeDecodeError:
                text = payload.decode('latin-1')
            print(f'CLIENT_CUTTEXT: {text!r}', flush=True)
        else:
            raise RuntimeError(f'unrecognized client opcode {t}')
    print('session duration:',time.monotonic()-start)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port',type=int,default=5999)
    parser.add_argument('--width',type=int,default=800)
    parser.add_argument('--height',type=int,default=450)
    parser.add_argument('--disconnect-after',type=int,default=0)
    parser.add_argument('--send-cuttext',default=None,help='send a ServerCutText (clipboard) to the client after framebuffer init, then continue serving')
    parser.add_argument('--send-cursor',action='store_true',help='send a 2x2 test Cursor pseudo-encoding rectangle (encoding -239) after framebuffer init')
    parser.add_argument('--led-state',type=lambda x:int(x,0),default=None,help='send a QEMU LED State pseudo-encoding (-261) 1-byte state (bit0 scroll, bit1 num, bit2 caps)')
    parser.add_argument('--log-pointer',action='store_true',help='log PointerEvent (x, y, mask) received from the client (coords only)')
    args=parser.parse_args()
    if args.width <= 0 or args.height <= 0 or args.width * args.height > 16_000_000:
        parser.error('dimensions must be positive and <=16 megapixels')
    with socket.socket(socket.AF_INET,socket.SOCK_STREAM) as srv:
        srv.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
        srv.bind(('127.0.0.1',args.port))
        srv.listen(2)
        print(f'LOCAL ONLY: VNC test server on 127.0.0.1:{srv.getsockname()[1]} ({args.width}x{args.height})',flush=True)
        while True:
            conn, addr=srv.accept()
            with conn:
                try:
                    serve_one(conn,args.width,args.height,args.disconnect_after,args.send_cuttext,args.send_cursor,args.led_state,args.log_pointer)
                except (EOFError,ConnectionError,TimeoutError,RuntimeError,OSError) as exc:
                    print('test session ended:',exc,flush=True)

if __name__=='__main__':
    main()
