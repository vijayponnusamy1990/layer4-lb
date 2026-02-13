import socket
import time
import multiprocessing
import os

TARGET_HOST = '127.0.0.1'
TARGET_PORT = 8080
NUM_WORKERS = 8
DURATION = 10

def worker(stop_event, counter):
    count = 0
    while not stop_event.is_set():
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            s.connect((TARGET_HOST, TARGET_PORT))
            s.sendall(b"PING")
            s.recv(1024)
            s.close()
            count += 1
        except Exception:
            pass
    with counter.get_lock():
        counter.value += count

def run_benchmark():
    stop_event = multiprocessing.Event()
    counter = multiprocessing.Value('i', 0)
    workers = []

    print(f"Starting benchmark with {NUM_WORKERS} workers for {DURATION} seconds...")

    for _ in range(NUM_WORKERS):
        p = multiprocessing.Process(target=worker, args=(stop_event, counter))
        p.start()
        workers.append(p)

    time.sleep(DURATION)
    stop_event.set()

    for p in workers:
        p.join()

    rps = counter.value / DURATION
    print(f"Total Requests: {counter.value}")
    print(f"RPS: {rps:.2f}")

if __name__ == "__main__":
    run_benchmark()
