import socket
import threading
import sys

def handle_client(conn, addr):
    # print(f"Connected by {addr}")
    while True:
        try:
            data = conn.recv(1024)
            if not data:
                break
            conn.sendall(data)
        except ConnectionResetError:
            break
    conn.close()

def start_server(port):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            s.bind(('127.0.0.1', port))
            s.listen()
            print(f"Echo server listening on port {port}")
            while True:
                conn, addr = s.accept()
                t = threading.Thread(target=handle_client, args=(conn, addr))
                t.start()
        except OSError as e:
            print(f"Failed to bind port {port}: {e}")

if __name__ == "__main__":
    ports = [8081, 8082]
    if len(sys.argv) > 1:
        ports = [int(p) for p in sys.argv[1:]]
    
    threads = []
    for port in ports:
        t = threading.Thread(target=start_server, args=(port,))
        t.start()
        threads.append(t)
    
    for t in threads:
        t.join()
