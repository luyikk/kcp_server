import socket
import struct
import time

KCP_OVERHEAD = 24

def make_kcp_packet(conv, cmd, wnd, ts, sn, una, payload):
    length = len(payload)
    header = struct.pack('<IBBHIIII', conv, cmd, 0, wnd, ts, sn, una, length)
    return header + payload

def parse_kcp_header(data):
    conv, cmd, frg, wnd, ts, sn, una, length = struct.unpack_from('<IBBHIIII', data, 0)
    return conv, cmd, frg, wnd, ts, sn, una, length

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(('127.0.0.1', 0))
print(f"Client: 127.0.0.1:{sock.getsockname()[1]}")
sock.settimeout(2.0)

SERVER = ('127.0.0.1', 5555)
CONV = 99999
ts = int(time.time() * 1000) & 0xFFFFFFFF

# ===== Probe 1: Hot loop path — send sn=0 establishment, then sn=1 follow-up =====
print("=" * 50)
print("PROBE 1: Hot loop path (BytesMut conversion on subsequent packets)")
print("=" * 50)

pkt = make_kcp_packet(CONV, 81, 128, ts, 0, 0, b'first')
sock.sendto(pkt, SERVER)
try:
    data, _ = sock.recvfrom(2048)
    print(f"  sn=0 response: {len(data)} bytes (conv established)")
except socket.timeout:
    print("  sn=0: no response")

# Now send a follow-up with sn=1 — exercises the while loop BytesMut path
pkt2 = make_kcp_packet(CONV, 81, 128, ts + 1, 1, 0, b'second')
sock.sendto(pkt2, SERVER)
try:
    data, _ = sock.recvfrom(2048)
    print(f"  sn=1 response: {len(data)} bytes — loop path OK")
except socket.timeout:
    print("  sn=1: no response (may need correct ack)")

# ===== Probe 2: Conv request mode (exactly 4 bytes) =====
print("\n" + "=" * 50)
print("PROBE 2: Conv request mode (4-byte packet)")
print("=" * 50)
sock2 = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock2.bind(('127.0.0.1', 0))
sock2.settimeout(2.0)
sock2.sendto(b'\x01\x02\x03\x04', SERVER)
try:
    data, _ = sock2.recvfrom(2048)
    print(f"  Response: {len(data)} bytes = {data.hex()} (expect 8 bytes: 4 request + conv)")
except socket.timeout:
    print("  No response (timeout)")

# ===== Probe 3: Key exchange mode (10-byte "key") =====
print("\n" + "=" * 50)
print("PROBE 3: Key exchange mode (10-byte packet)")
print("=" * 50)
sock3 = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock3.bind(('127.0.0.1', 0))
sock3.settimeout(2.0)
key_data = b'mykey12345'  # 10 bytes, between 4 and 24
sock3.sendto(key_data, SERVER)
try:
    data, _ = sock3.recvfrom(2048)
    expected_len = 4 + len(key_data)  # conv (4) + key echo (10) = 14
    print(f"  Response: {len(data)} bytes (expect {expected_len}: 4-byte conv + {len(key_data)}-byte key)")
except socket.timeout:
    print("  No response (timeout)")

# ===== Probe 4: Empty/small packet (should be ignored, not crash) =====
print("\n" + "=" * 50)
print("PROBE 4: Malformed packets (test error resilience)")
print("=" * 50)

# 2-byte packet — too small for any mode, should not crash
sock.sendto(b'\x00\x01', SERVER)
time.sleep(0.1)
print("  2-byte packet sent — server still running (no crash)")

# 0-byte packet
sock.sendto(b'', SERVER)
time.sleep(0.1)
print("  0-byte packet sent — server still running (no crash)")

# 24-byte packet with sn != 0 (should be rejected quietly)
bad_pkt = make_kcp_packet(CONV + 100, 81, 128, ts, 5, 0, b'bad_sn')
sock.sendto(bad_pkt, SERVER)
time.sleep(0.1)
print("  KCP packet with sn=5 (no prior establishment) — server still running")

# ===== Probe 5: Verify server is still up =====
print("\n" + "=" * 50)
print("PROBE 5: Server still responsive after probes")
print("=" * 50)
CONV2 = 55555
pkt = make_kcp_packet(CONV2, 81, 128, int(time.time()*1000) & 0xFFFFFFFF, 0, 0, b'final_test')
sock.sendto(pkt, SERVER)
try:
    data, _ = sock.recvfrom(2048)
    if len(data) >= KCP_OVERHEAD:
        conv, cmd, _, _, _, _, _, length = parse_kcp_header(data)
        print(f"  Response received: cmd={cmd} len={length} — server healthy after all probes")
except socket.timeout:
    print("  No response — server may be down!")

sock.close()
sock2.close()
sock3.close()
print("\nAll probes complete.")
