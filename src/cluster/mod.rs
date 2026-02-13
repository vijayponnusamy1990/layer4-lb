use foca::{Foca, Config, Identity, BroadcastHandler, BincodeCodec, Invalidates};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use serde::{Serialize, Deserialize};
use rand::{rngs::StdRng, Rng, SeedableRng}; 
use std::time::Duration;
use bytes::Bytes;
use anyhow;
use bincode; // Now v2
use std::fmt;

// --- Data Structures ---

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum BroadcastMessage {
    UsageUpdate {
        node_id: u64,
        key: String,
        usage: u32,
    }
}

// Key for invalidation: (NodeID, Key)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BroadcastKey {
    node_id: u64,
    key: String,
}

impl Invalidates for BroadcastKey {
    fn invalidates(&self, other: &Self) -> bool {
        self == other
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct NodeIdentity {
    pub addr: SocketAddr,
    pub id: u64,
}

impl Identity for NodeIdentity {
    type Addr = SocketAddr;

    fn renew(&self) -> Option<Self> {
        Some(Self {
            addr: self.addr,
            // rand 0.9 might change gen()? 
            // If random() is preferred, check docs.
            // But let's assume r#gen() works or random().
            // ThreadRng implements Rng.
            id: rand::random(), // rand 0.9 uses rand::rng() and random()?
            // Or rand::thread_rng().gen()?
            // rand 0.9 removed thread_rng()? Replaced with rng().
            // And gen() replaced with random()?
            // Let's use `rand::random()` free function for simplicity if available?
            // Or `rand::rng().random()`.
        })
    }

    fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn win_addr_conflict(&self, _other: &Self) -> bool {
        true 
    }
}

// Commands from Application to Cluster
#[derive(Debug)]
pub enum ClusterCommand {
    BroadcastUsage(String, u32),
}

// --- Custom Error ---

#[derive(Debug)]
pub enum ClusterError {
    Bincode(bincode::error::DecodeError),
    Anyhow(String),
}

impl fmt::Display for ClusterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClusterError::Bincode(e) => write!(f, "Bincode error: {:?}", e),
            ClusterError::Anyhow(e) => write!(f, "Error: {}", e),
        }
    }
}

impl std::error::Error for ClusterError {}

// --- Custom Broadcast Handler ---

struct SimpleBroadcastHandler {
    tx_state: mpsc::Sender<(u64, String, u32)>, 
}

impl BroadcastHandler<NodeIdentity> for SimpleBroadcastHandler {
    type Key = BroadcastKey;
    type Error = ClusterError;

    fn receive_item(
        &mut self,
        data: &[u8],
        _sender: Option<&NodeIdentity>,
    ) -> Result<Option<Self::Key>, Self::Error> {
        let config = bincode::config::standard();
        let (msg, _len): (BroadcastMessage, usize) = bincode::serde::decode_from_slice(data, config)
            .map_err(ClusterError::Bincode)?;
            
        match msg {
            BroadcastMessage::UsageUpdate { node_id, key, usage } => {
                let bkey = BroadcastKey { node_id, key: key.clone() };
                let _ = self.tx_state.try_send((node_id, key, usage));
                Ok(Some(bkey))
            }
        }
    }
}

// --- Cluster Actor ---

type FocaInstance = Foca<
    NodeIdentity,
    BincodeCodec<bincode::config::Configuration>,
    StdRng,
    SimpleBroadcastHandler
>;

pub struct Cluster {
    foca: FocaInstance,
    socket: Arc<UdpSocket>,
    rx_cmd: mpsc::Receiver<ClusterCommand>,
    identity: NodeIdentity,
}

impl Cluster {
    pub async fn new(
        bind_addr: SocketAddr, 
        _peers: Vec<SocketAddr>,
        rx_cmd: mpsc::Receiver<ClusterCommand>,
        tx_state: mpsc::Sender<(u64, String, u32)>
    ) -> Result<Self, anyhow::Error> {
        let socket = UdpSocket::bind(bind_addr).await?;
        let socket = Arc::new(socket);

        let mut config = Config::simple();
        config.notify_down_members = true;
        
        let id: u64 = rand::random(); // Use free function
        let identity = NodeIdentity {
            addr: bind_addr,
            id,
        };

        // rand 0.9: impl SeedableRng
        // from_entropy is removed. Use from_rng with system rng.
        let rng = StdRng::from_rng(&mut rand::rng());
        
        // Codec MUST handle NodeIdentity
        let codec = BincodeCodec(bincode::config::standard());
        // Pass tx_state to handler
        let broadcast_handler = SimpleBroadcastHandler { tx_state: tx_state.clone() };

        let foca = Foca::with_custom_broadcast(
            identity.clone(),
            config,
            rng,
            codec,
            broadcast_handler, 
        );
        
        Ok(Self {
            foca,
            socket,
            rx_cmd,
            identity,
        })
    }

    pub async fn run(mut self, _seeds: Vec<SocketAddr>) {
        let mut buf = vec![0u8; 65535];
        let mut timer = tokio::time::interval(Duration::from_millis(100));
        
        loop {
            // We use AccumulatingRuntime to capture actions from Foca
            let mut runtime = foca::AccumulatingRuntime::new();
            
            tokio::select! {
                _ = timer.tick() => {
                     // Periodic Gossip triggering
                     if let Err(e) = self.foca.gossip(&mut runtime) {
                         eprintln!("Foca gossip error: {:?}", e);
                     }
                }
                
                result = self.socket.recv_from(&mut buf) => {
                    if let Ok((len, _from)) = result {
                        let data = &buf[..len];
                        let mut bytes_buf = Bytes::copy_from_slice(data);
                        if let Err(_e) = self.foca.handle_data(&mut bytes_buf, &mut runtime) {
                             // error
                        }
                    }
                }
                
                Some(cmd) = self.rx_cmd.recv() => {
                     match cmd {
                         ClusterCommand::BroadcastUsage(key, usage) => {
                             let msg = BroadcastMessage::UsageUpdate { 
                                 node_id: self.identity.id,
                                 key, 
                                 usage 
                             };
                             
                             let config = bincode::config::standard();
                             if let Ok(bytes) = bincode::serde::encode_to_vec(&msg, config) {
                                 if let Err(e) = self.foca.add_broadcast(&bytes) {
                                     eprintln!("Broadcast error: {:?}", e);
                                 }
                             }
                         }
                     }
                }
            }
            
            self.handle_runtime(runtime).await;
        }
    }
    
    async fn handle_runtime(&mut self, mut runtime: foca::AccumulatingRuntime<NodeIdentity>) {
        // Drain to_send
        while let Some((dst, data)) = runtime.to_send() {
             let _ = self.socket.send_to(&data, dst.addr).await;
        }
        
        // Drain notifications
        while let Some(notification) = runtime.to_notify() {
            match notification {
                foca::OwnedNotification::MemberUp(m) => println!("Cluster: Member UP {:?}", m),
                foca::OwnedNotification::MemberDown(m) => println!("Cluster: Member DOWN {:?}", m),
                 foca::OwnedNotification::Active => println!("Cluster: Active"),
                 foca::OwnedNotification::Idle => println!("Cluster: Idle"),
                 foca::OwnedNotification::Defunct => println!("Cluster: Defunct"),
                _ => {}
            }
        }
    }
}
