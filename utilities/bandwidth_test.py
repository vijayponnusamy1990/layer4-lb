import socket
import time
import sys

def test_bandwidth(host, port, size_bytes):
    print(f"Connecting to {host}:{port}...")
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.connect((host, port))
    
    # Send a large payload
    # Echo server reads and writes back.
    # We want to measure the time it takes to receive the echo.
    
    payload = b'A' * size_bytes
    print(f"Sending {size_bytes} bytes...")
    
    start_time = time.time()
    s.sendall(payload)
    
    # Shutdown write to signal end of stream to echo server (if it supports it)
    # Our simple echo server might just read loop.
    # Let's just read back same amount.
    
    received = 0
    while received < size_bytes:
        chunk = s.recv(4096)
        if not chunk:
            break
        received += len(chunk)
        
    end_time = time.time()
    duration = end_time - start_time
    
    print(f"Received {received} bytes in {duration:.2f} seconds.")
    speed = (received / duration) / 1024 / 1024 # MB/s
    print(f"Speed: {speed:.2f} MB/s")
    
    s.close()

if __name__ == "__main__":
    # Test with 100KB. Config limit is 10KB/s.
    # Should take ~10 seconds.
    test_bandwidth('127.0.0.1', 8080, 100 * 1024)
