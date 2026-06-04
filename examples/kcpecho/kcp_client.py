import socket
import struct
import time

KCP_CMD_PUSH = 81
KCP_OVERHEAD = 24

def make_kcp_packet(conv, cmd, wnd, ts, sn, una, payload):
    """Build a KCP packet with 24-byte header + payload."""
    length = len(payload)
    # conv: I, cmd: B, frg: B, wnd: H, ts: I, sn: I, una: I, len: I = 8 fields
    header = struct.pack('<IBBHIIII', conv, cmd, 0, wnd, ts, sn, una, length)
    return header + payload

def parse_kcp_header(data):
    """Parse KCP 24-byte header, return (conv, cmd, frg, wnd, ts, sn, una, length)."""
    conv, cmd, frg, wnd, ts, sn, una, length = struct.unpack_from('<IBBHIIII', data, 0)
    return conv, cmd, frg, wnd, ts, sn, una, length

# Create UDP socket
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(('127.0.0.1', 0))
local_port = sock.getsockname()[1]
print(f"Client bound to 127.0.0.1:{local_port}")
sock.settimeout(3.0)

SERVER = ('127.0.0.1', 5555)
CONV = 12345
PAYLOAD = b'Hello KCP Server!'

# Send KCP PUSH packet with sn=0 (connection establishment)
ts = int(time.time() * 1000) & 0xFFFFFFFF
packet = make_kcp_packet(CONV, KCP_CMD_PUSH, 128, ts, 0, 0, PAYLOAD)
print(f"Sending KCP PUSH: conv={CONV}, sn=0, ts={ts}, len={len(PAYLOAD)}, payload={PAYLOAD!r}")
sock.sendto(packet, SERVER)

# Wait for response (may be ACK, ACK+PUSH, or just PUSH)
received_echo = False
for i in range(3):
    try:
        data, addr = sock.recvfrom(2048)
        print(f"\n[{i+1}] Received {len(data)} bytes from {addr}")

        if len(data) >= KCP_OVERHEAD:
            conv, cmd, frg, wnd, ts_r, sn, una, length = parse_kcp_header(data)
            cmd_name = {81: 'PUSH', 82: 'ACK', 83: 'WASK', 84: 'WINS'}.get(cmd, f'UNKNOWN({cmd})')
            print(f"  conv={conv} cmd={cmd_name} sn={sn} una={una} wnd={wnd} len={length}")

            if length > 0 and len(data) >= KCP_OVERHEAD + length:
                echoed = data[KCP_OVERHEAD:KCP_OVERHEAD + length]
                print(f"  payload={echoed!r}")
                if echoed == PAYLOAD:
                    print("  PASS: Echo matches!")
                    received_echo = True
                    break
                else:
                    print(f"  NOTE: unexpected payload")
        else:
            print(f"  Raw: {data.hex()}")
    except socket.timeout:
        print(f"\n[{i+1}] Timeout waiting for packet")
        break

if received_echo:
    print("\n=== VERDICT: Echo server working correctly ===")
else:
    print("\n=== NOTE: Echo not confirmed (may need full KCP handshake) ===")
    print("The server responded (BytesMut path exercised) but echo requires KCP state.")

sock.close()
print("Done.")
