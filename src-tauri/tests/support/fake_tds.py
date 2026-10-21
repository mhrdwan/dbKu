# Minimal fake SQL Server: PRELOGIN, then a TLS 1.0 handshake wrapped in TDS
# packets (like SQL Server 2012 RTM with a 1024-bit self-signed cert).
import socket, ssl, struct, sys
port, cert, key = int(sys.argv[1]), sys.argv[2], sys.argv[3]

def tds(payload):
    return struct.pack(">BBHHBB", 0x12, 0x01, 8 + len(payload), 0, 1, 0) + payload

def read_packet(c):
    head = b""
    while len(head) < 8:
        chunk = c.recv(8 - len(head))
        if not chunk:
            raise ConnectionError("client closed")
        head += chunk
    n = struct.unpack(">H", head[2:4])[0] - 8
    body = b""
    while len(body) < n:
        body += c.recv(n - len(body))
    return body

srv = socket.socket(); srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", port)); srv.listen(1)
c, _ = srv.accept(); c.settimeout(8)
read_packet(c)
# PRELOGIN response: version 11.0.2100, ENCRYPT_ON
opts = [(0x00, bytes([11, 0]) + struct.pack(">H", 2100) + b"\x00\x00"), (0x01, b"\x01")]
off = 5 * len(opts) + 1; tok = data = b""
for t, v in opts:
    tok += struct.pack(">BHH", t, off, len(v)); data += v; off += len(v)
c.sendall(tds(tok + b"\xff" + data))
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.minimum_version = ssl.TLSVersion.TLSv1; ctx.maximum_version = ssl.TLSVersion.TLSv1
ctx.set_ciphers("ALL:@SECLEVEL=0"); ctx.load_cert_chain(cert, key)
inc, out = ssl.MemoryBIO(), ssl.MemoryBIO()
tls = ctx.wrap_bio(inc, out, server_side=True)
try:
    while True:
        try:
            tls.do_handshake(); break
        except ssl.SSLWantReadError:
            pending = out.read()
            if pending: c.sendall(tds(pending))
            inc.write(read_packet(c))
    pending = out.read()
    if pending: c.sendall(tds(pending))
    print("SERVER: TLS handshake done:", tls.version(), tls.cipher()[0], flush=True)
    c.settimeout(5)
    raw = c.recv(4096)  # the encrypted LOGIN7 would follow
    print("SERVER: received", len(raw), "bytes after handshake (login packet)", flush=True)
except Exception as e:
    print("SERVER: handshake failed:", type(e).__name__, e, flush=True)
c.close()
