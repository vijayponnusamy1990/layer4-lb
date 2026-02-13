import socket
import sys

TARGET_HOST = '127.0.0.1'
TARGET_PORT = 9000 # Unconfigured port in valid range

try:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(2.0)
    print(f"Connecting to {TARGET_HOST}:{TARGET_PORT}...")
    s.connect((TARGET_HOST, TARGET_PORT))
    print("Connected? This should not happen if no listener.")
    s.close()
except ConnectionRefusedError:
    print("Success: Connection Refused (Correct behavior for closed port).")
    sys.exit(0)
except TimeoutError:
    print("Failure: Connection Timed Out (Silent Drop - Bad).")
    sys.exit(1)
except Exception as e:
    print(f"Other Error: {e}")
    # ConnectionReset is also acceptable
    if "Reset" in str(e):
         print("Success: Connection Reset (Acceptable).")
         sys.exit(0)
    sys.exit(1)
