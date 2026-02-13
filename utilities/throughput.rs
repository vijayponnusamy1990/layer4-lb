use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::{self, Duration, Instant};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage:");
        eprintln!("  Server (Sink): {} server <port>", args[0]);
        eprintln!("  Client (Source): {} client <target_addr> <connections> <seconds>", args[0]);
        return Ok(());
    }

    match args[1].as_str() {
        "server" => {
            let port = args.get(2).unwrap_or(&"9001".to_string()).parse::<u16>()?;
            run_server(port).await?;
        }
        "client" => {
            let addr = args.get(2).expect("Missing target addr");
            let connections = args.get(3).unwrap_or(&"1".to_string()).parse::<usize>()?;
            let duration = args.get(4).unwrap_or(&"10".to_string()).parse::<u64>()?;
            run_client(addr, connections, duration).await?;
        }
        _ => eprintln!("Invalid mode. Use 'server' or 'client'"),
    }
    Ok(())
}

async fn run_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("Sink Server listening on port {}", port);

    let bytes_received = Arc::new(AtomicUsize::new(0));
    let bytes_clone = bytes_received.clone();

    // Stats reporter
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(1));
        let mut last_bytes = 0;
        loop {
            interval.tick().await;
            let current = bytes_clone.load(Ordering::Relaxed);
            let delta = current - last_bytes;
            last_bytes = current;
            let gb_s = delta as f64 / 1_000_000_000.0;
            println!("Speed: {:.2} GB/s ({} MB/s)", gb_s, delta / 1_000_000);
        }
    });

    loop {
        let (mut socket, _) = listener.accept().await?;
        let bytes = bytes_received.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 64 * 1024]; // 64KB buffer
            // For max performance, we just read and discard
            loop {
                match socket.read(&mut buf).await {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        bytes.fetch_add(n, Ordering::Relaxed);
                    }
                    Err(_) => break,
                }
            }
        });
    }
}

async fn run_client(target: &str, connections: usize, seconds: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting {} sockets to {} for {} seconds...", connections, target, seconds);
    
    let bytes_sent = Arc::new(AtomicUsize::new(0));
    let start_signal = Arc::new(tokio::sync::Notify::new());
    
    let mut handles = vec![];

    for _ in 0..connections {
        let target = target.to_string();
        let bytes = bytes_sent.clone();
        let start = start_signal.clone();
        
        handles.push(tokio::spawn(async move {
            let mut stream = match TcpStream::connect(&target).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Connect failed: {}", e);
                    return;
                }
            };
            
            // Disable Nagle's algorithm for max throughput
            let _ = stream.set_nodelay(true);

            let buf = vec![0u8; 128 * 1024]; // 128KB chunks
            
            // Wait for start signal
            start.notified().await;
            
            let deadline = Instant::now() + Duration::from_secs(seconds);
            
            while Instant::now() < deadline {
                if let Err(_) = stream.write_all(&buf).await {
                    break;
                }
                bytes.fetch_add(buf.len(), Ordering::Relaxed);
            }
        }));
    }

    // Give time for connections to establish
    time::sleep(Duration::from_secs(1)).await;
    println!("Starting flood...");
    start_signal.notify_waiters();

    let start_time = Instant::now();
    time::sleep(Duration::from_secs(seconds)).await;
    let duration = start_time.elapsed().as_secs_f64();
    
    let total_bytes = bytes_sent.load(Ordering::Relaxed);
    let gb_s = (total_bytes as f64 / 1_000_000_000.0) / duration;
    
    println!("Finished.");
    println!("Total Transferred: {:.2} GB", total_bytes as f64 / 1_000_000_000.0);
    println!("Average Speed: {:.2} GB/s", gb_s);

    Ok(())
}
