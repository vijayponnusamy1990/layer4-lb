import socket
import time

def send_request():
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.settimeout(2)
            s.connect(('127.0.0.1', 8080))
            s.sendall(b"Ping")
            data = s.recv(1024)
            print(f"Response: {data.decode() if data else 'Empty'}")
    except Exception as e:
        print(f"Request failed: {e}")

if __name__ == "__main__":
    print("Sending 5 requests...")
    for _ in range(5):
        send_request()
        time.sleep(0.5)
