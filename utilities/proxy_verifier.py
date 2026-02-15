import socket
import struct
import sys

def verify_proxy_protocol(port):
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(('127.0.0.1', port))
    sock.listen(1)
    print(f"Listening on {port}...")

    conn, addr = sock.accept()
    print(f"Accepted connection from {addr}")

    # Read signature (12 bytes) + meta (4 bytes) + addresses (12 bytes for IPv4) = 28 bytes minimum
    data = conn.recv(1024)
    
    v2_sig = b'\x0D\x0A\x0D\x0A\x00\x0D\x0A\x51\x55\x49\x54\x0A'
    
    if data.startswith(v2_sig):
        print("SUCCESS: Proxy Protocol V2 Signature did match.")
        
        # Parse version/cmd
        ver_cmd = data[12]
        if ver_cmd == 0x21:
            print("SUCCESS: Version 2, PROXY command.")
        else:
            print(f"FAILURE: Unexpected Ver/Cmd: {hex(ver_cmd)}")

        # Parse family/proto
        fam_proto = data[13]
        if fam_proto == 0x11:
             print("SUCCESS: AF_INET (IPv4), STREAM (TCP).")
        else:
             print(f"FAILURE: Unexpected Fam/Proto: {hex(fam_proto)}")
        
        # Address length
        addr_len = struct.unpack('!H', data[14:16])[0]
        print(f"Address Length: {addr_len}")

        if fam_proto == 0x11 and addr_len == 12:
             src_ip = socket.inet_ntop(socket.AF_INET, data[16:20])
             dst_ip = socket.inet_ntop(socket.AF_INET, data[20:24])
             src_port = struct.unpack('!H', data[24:26])[0]
             dst_port = struct.unpack('!H', data[26:28])[0]
             
             print(f"Source: {src_ip}:{src_port}")
             print(f"Dest: {dst_ip}:{dst_port}")
             
             if src_ip == "127.0.0.1":
                 print("SUCCESS: Source IP is correct.")
             else:
                 print("FAILURE: Source IP mismatch.")

    else:
        print("FAILURE: Proxy Protocol V2 Signature did NOT match.")
        print(f"Received: {data[:12].hex()}")

    conn.close()
    sock.close()

if __name__ == "__main__":
    verify_proxy_protocol(9096)
