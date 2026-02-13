import socket
import threading
import time

def test_port(port, message):
    try:
        start_time = time.time()
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.connect(('127.0.0.1', port))
            s.sendall(message.encode())
            data = s.recv(1024)
            print(f"Port {port}: Received {data.decode()}")
        duration = time.time() - start_time
        print(f"Port {port}: Completed in {duration:.4f}s")
    except Exception as e:
        print(f"Port {port}: Error - {e}")

if __name__ == "__main__":
    t1 = threading.Thread(target=test_port, args=(8081, "Hello from 8081"))
    t2 = threading.Thread(target=test_port, args=(8082, "Hello from 8082"))
    
    print("Starting concurrent requests...")
    t1.start()
    t2.start()
    
    t1.join()
    t2.join()
    print("Test complete.")
