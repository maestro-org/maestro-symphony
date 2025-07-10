use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use bitcoin::{
    Block, BlockHash, VarInt,
    block::Header,
    consensus::{self, Decodable, Encodable, ReadExt, encode},
    hashes::Hash,
    io,
    key::rand::Rng,
    p2p::{
        Address, Magic, ServiceFlags,
        message::{CommandString, NetworkMessage, RawNetworkMessage},
        message_blockdata::{GetHeadersMessage, Inventory},
        message_network::VersionMessage,
    },
    secp256k1,
};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        TcpStream, ToSocketAddrs,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    select,
    sync::{Notify, mpsc},
    task::JoinHandle,
};
use tracing::{error, info, trace};

const VERSION: u32 = 70016;
const USER_AGENT: &str = "/symphony:0.0.1/";
const SERVICES: ServiceFlags = ServiceFlags::NONE;

pub type RawBlock = Vec<u8>;

#[derive(Debug)]
pub enum DecodedMessage {
    Version(VersionMessage),
    Verack,
    Ping(u64),
    Inv(Vec<Inventory>),
    Headers(Vec<Header>),
    Block(RawBlock),
    Unused,
}

pub fn version_message() -> NetworkMessage {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

    let services = SERVICES;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time error")
        .as_secs() as i64;

    NetworkMessage::Version(VersionMessage {
        version: VERSION,
        services,
        timestamp,
        receiver: Address::new(&addr, services),
        sender: Address::new(&addr, services),
        nonce: secp256k1::rand::thread_rng().r#gen(),
        user_agent: USER_AGENT.into(),
        start_height: 0,
        relay: false,
    })
}

impl Decodable for DecodedMessage {
    fn consensus_decode<D: io::Read + ?Sized>(d: &mut D) -> Result<Self, encode::Error> {
        let _magic: Magic = Decodable::consensus_decode(d)?;

        let command = CommandString::consensus_decode(d)?;

        let len = u32::consensus_decode(d)?;
        let _checksum = <[u8; 4]>::consensus_decode(d)?;

        let mut payload = vec![0u8; len as usize];
        d.read_slice(&mut payload)?;
        let payload = &mut &payload[..];

        let message = match command.as_ref() {
            "version" => DecodedMessage::Version(Decodable::consensus_decode(payload)?),
            "verack" => DecodedMessage::Verack,
            "ping" => DecodedMessage::Ping(Decodable::consensus_decode(payload)?),
            "inv" => DecodedMessage::Inv(Decodable::consensus_decode(payload)?),
            "headers" => DecodedMessage::Headers({
                let len = VarInt::consensus_decode(payload)?.0 as usize;
                let mut headers = Vec::with_capacity(len);

                for _ in 0..len {
                    headers.push(Decodable::consensus_decode(payload)?);
                    let _: VarInt = Decodable::consensus_decode(payload)?;
                }

                headers
            }),
            "block" => DecodedMessage::Block(payload.to_vec()),
            _ => DecodedMessage::Unused,
        };

        Ok(message)
    }
}

#[derive(Debug, Error)]
pub enum P2PError {
    #[error("error connecting bearer")]
    ConnectFailure(#[source] tokio::io::Error),

    #[error("error ingress IO")]
    IngressIO(#[source] std::io::Error),

    #[error("error ingress relay")]
    IngressChannelClosed,

    #[error("error ingress message decoding")]
    IngressDecoding(#[source] consensus::encode::Error),

    #[error("error egress IO")]
    EgressIO(#[source] std::io::Error),

    #[error("error egress relay")]
    EgressChannelClosed,

    #[error("error peer channel: {0}")]
    PeerChannelClosed(String),

    #[error("error getting blocks, requested block {0} but received {1}")]
    GetBlocksMismatch(BlockHash, BlockHash),
}

pub struct Peer {
    request_send: mpsc::Sender<Request>,
    headers_recv: mpsc::Receiver<Vec<Header>>,
    blocks_recv: mpsc::Receiver<RawBlock>,
    pub new_block_notification: Arc<Notify>,
    pub handler: JoinHandle<()>,
}

#[allow(dead_code)]
#[derive(Debug)]
enum Request {
    NewHeaders(GetHeadersMessage),
    Blocks(Vec<Inventory>),
    MempoolIds,
}

impl Request {
    // https://en.bitcoin.it/wiki/Protocol_documentation#getheaders
    fn get_new_headers(intersects: Vec<BlockHash>) -> Request {
        Request::NewHeaders(GetHeadersMessage::new(intersects, BlockHash::all_zeros()))
    }

    // https://en.bitcoin.it/wiki/Protocol_documentation#getdata
    fn get_blocks(blockhashes: &[BlockHash]) -> Request {
        Request::Blocks(
            blockhashes
                .iter()
                .map(|blockhash| Inventory::WitnessBlock(*blockhash))
                .collect(),
        )
    }

    #[allow(dead_code)]
    fn get_mempool_ids() -> Request {
        Request::MempoolIds
    }
}

impl Peer {
    pub async fn connect<A: ToSocketAddrs>(addr: A, magic: Magic) -> Result<Self, P2PError> {
        let stream = TcpStream::connect(addr)
            .await
            .map_err(P2PError::ConnectFailure)?;

        // configure tcp
        let sock_ref = socket2::SockRef::from(&stream);
        let mut tcp_keepalive = socket2::TcpKeepalive::new();
        tcp_keepalive = tcp_keepalive.with_time(tokio::time::Duration::from_secs(20));
        tcp_keepalive = tcp_keepalive.with_interval(tokio::time::Duration::from_secs(20));

        sock_ref
            .set_tcp_keepalive(&tcp_keepalive)
            .map_err(P2PError::ConnectFailure)?;

        sock_ref
            .set_tcp_nodelay(true)
            .map_err(P2PError::ConnectFailure)?;

        // for sending requests to the handler
        let (request_send, request_recv) = tokio::sync::mpsc::channel(1);

        // for the handler to signal that the version handshake was performed
        let init_notifier = Arc::new(tokio::sync::Notify::new());

        // for the handler to relay blocks received from the peer
        let (blocks_send, blocks_recv) = tokio::sync::mpsc::channel(20);

        // for the handler to relay new headers received from the peer
        let (headers_send, headers_recv) = tokio::sync::mpsc::channel(20);

        // for the handler to notify that the peer broadcasted a new block
        let new_block_notifier = Arc::new(tokio::sync::Notify::new());

        let mut handler = PeerHandler::new(
            magic,
            stream,
            request_recv,
            init_notifier.clone(),
            headers_send,
            blocks_send,
            new_block_notifier.clone(),
        );

        let running_handler = tokio::spawn(async move {
            if let Err(e) = handler.run().await {
                error!("running handler errored: {e:?}");
            }
        });

        init_notifier.notified().await;

        Ok(Self {
            request_send,
            headers_recv,
            blocks_recv,
            new_block_notification: new_block_notifier.clone(),
            handler: running_handler,
        })
    }

    pub async fn get_new_headers(
        &mut self,
        intersects: Vec<BlockHash>,
    ) -> Result<Vec<Header>, P2PError> {
        trace!("requesting headers {intersects:?}");

        self.request_send
            .send(Request::get_new_headers(intersects))
            .await
            .map_err(|_| P2PError::EgressChannelClosed)?;

        let headers = self
            .headers_recv
            .recv()
            .await
            .ok_or(P2PError::PeerChannelClosed("headers".into()))?;

        Ok(headers)
    }

    pub async fn get_blocks(&mut self, hashes: Vec<BlockHash>) -> Result<Vec<Block>, P2PError> {
        trace!("requesting blocks {hashes:?}");

        self.request_send
            .send(Request::get_blocks(hashes.as_slice()))
            .await
            .unwrap();

        let mut blocks = Vec::with_capacity(hashes.len());

        for _ in hashes.iter() {
            let raw = self
                .blocks_recv
                .recv()
                .await
                .ok_or(P2PError::PeerChannelClosed("blocks".into()))?;

            blocks.push(raw)
        }

        let mut decoded_blocks = Vec::with_capacity(hashes.len());

        // receive each block
        for (hash, raw) in hashes.into_iter().zip(blocks.into_iter()) {
            let block = Block::consensus_decode_from_finite_reader(&mut &raw[..])
                .map_err(P2PError::IngressDecoding)?;

            // check the received block matches the list of hashes we requested
            if block.block_hash() != hash {
                return Err(P2PError::GetBlocksMismatch(block.block_hash(), hash));
            }

            decoded_blocks.push(block);
        }

        Ok(decoded_blocks)
    }

    pub async fn get_mempool(&mut self) -> Result<(), P2PError> {
        unimplemented!()
    }
}

struct Ingress {
    stream_read: OwnedReadHalf,
    // relay decoded message to appropriate output
    rx_send: mpsc::Sender<DecodedMessage>,
}

impl Ingress {
    pub fn new(stream_read: OwnedReadHalf, rx_send: mpsc::Sender<DecodedMessage>) -> Self {
        Self {
            stream_read,
            rx_send,
        }
    }

    pub async fn read_message(&mut self) -> Result<DecodedMessage, P2PError> {
        let mut header = [0u8; 24];
        self.stream_read
            .read_exact(&mut header)
            .await
            .map_err(P2PError::IngressIO)?;

        let payload_len = u32::consensus_decode_from_finite_reader(&mut &header[16..20])
            .map_err(P2PError::IngressDecoding)?;

        let mut payload = vec![0u8; payload_len as usize];
        self.stream_read
            .read_exact(&mut payload)
            .await
            .map_err(P2PError::IngressIO)?;

        let header_and_payload = [header.to_vec(), payload].concat();

        let message = Decodable::consensus_decode(&mut &header_and_payload[..])
            .map_err(P2PError::IngressDecoding)?;

        Ok(message)
    }

    async fn tick(&mut self) -> Result<(), P2PError> {
        let msg = self.read_message().await?;

        self.rx_send
            .send(msg)
            .await
            .map_err(|_| P2PError::IngressChannelClosed)
    }

    pub async fn run(&mut self) -> Result<(), P2PError> {
        loop {
            if let Err(err) = self.tick().await {
                error!("ingress handler errored: {err:?}");
                break Err(err);
            }
        }
    }
}

struct Egress {
    stream_write: OwnedWriteHalf,
    // receive decoded message from local (requests)
    tx_recv: mpsc::Receiver<RawNetworkMessage>,
}

impl Egress {
    pub fn new(stream_write: OwnedWriteHalf, tx_recv: mpsc::Receiver<RawNetworkMessage>) -> Self {
        Self {
            stream_write,
            tx_recv,
        }
    }

    pub async fn write_message(&mut self, msg: RawNetworkMessage) -> Result<(), std::io::Error> {
        let mut buf = vec![];
        msg.consensus_encode(&mut buf)?;
        self.stream_write.write_all(&buf).await?;
        self.stream_write.flush().await?;

        Ok(())
    }

    async fn tick(&mut self) -> Result<(), P2PError> {
        let msg = self.tx_recv.recv().await;

        if let Some(x) = msg {
            self.write_message(x).await.map_err(P2PError::EgressIO)?
        }

        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), P2PError> {
        loop {
            if let Err(err) = self.tick().await {
                error!("egress handler errored: {err:?}");
                break Err(err);
            }
        }
    }
}

struct PeerHandler {
    magic: Magic,
    request_recv: mpsc::Receiver<Request>,
    init_notifier: Arc<Notify>,
    headers_send: mpsc::Sender<Vec<Header>>,
    blocks_send: mpsc::Sender<RawBlock>,
    new_block_notifier: Arc<Notify>,
    ingress_handle: JoinHandle<Result<(), P2PError>>,
    ingress_recv: mpsc::Receiver<DecodedMessage>,
    egress_handle: JoinHandle<Result<(), P2PError>>,
    egress_send: mpsc::Sender<RawNetworkMessage>,
}

impl PeerHandler {
    pub fn new(
        magic: Magic,
        stream: TcpStream,
        request_recv: mpsc::Receiver<Request>,
        init_notifier: Arc<Notify>,
        headers_send: mpsc::Sender<Vec<Header>>,
        blocks_send: mpsc::Sender<RawBlock>,
        new_block_notifier: Arc<Notify>,
    ) -> Self {
        let (stream_read, stream_write) = stream.into_split();

        let (ingress_send, ingress_recv) = mpsc::channel(1);
        let (egress_send, egress_recv) = mpsc::channel(1);

        let ingress =
            tokio::spawn(async move { Ingress::new(stream_read, ingress_send).run().await });
        let egress =
            tokio::spawn(async move { Egress::new(stream_write, egress_recv).run().await });

        Self {
            magic,
            request_recv,
            init_notifier,
            headers_send,
            blocks_send,
            new_block_notifier,
            ingress_handle: ingress,
            ingress_recv,
            egress_handle: egress,
            egress_send,
        }
    }

    async fn tick(&mut self) -> Result<(), P2PError> {
        select! {
            maybe_req = self.request_recv.recv() => {
                let req = maybe_req.ok_or(P2PError::PeerChannelClosed("request".into()))?;
                let to_send = match req {
                    Request::NewHeaders(msg) => NetworkMessage::GetHeaders(msg),
                    Request::Blocks(inv) => NetworkMessage::GetData(inv),
                    Request::MempoolIds => NetworkMessage::MemPool,
                };

                let to_send = RawNetworkMessage::new(self.magic, to_send);

                self.egress_send.send(to_send).await.map_err(|_| P2PError::EgressChannelClosed)?;
            }
            maybe_msg = self.ingress_recv.recv() => {
                let msg = maybe_msg.ok_or(P2PError::IngressChannelClosed)?;

                match msg {
                    DecodedMessage::Version(ver) => {
                        info!("peer reported version {ver:?}, responding with ack...");
                        self.egress_send.send(RawNetworkMessage::new(self.magic, NetworkMessage::Verack)).await.map_err(|_| P2PError::EgressChannelClosed)?;
                    },
                    DecodedMessage::Verack => {
                        trace!("peer sent verack, sending init notification");
                        self.init_notifier.notify_one();
                    },
                    DecodedMessage::Inv(inv) => {
                        if inv.iter().any(|i| matches!(i, Inventory::Block(_))) {
                            trace!("peer sent inventory containing block: {inv:?}, sending new block notification");
                            self.new_block_notifier.notify_one();
                        } else {
                            trace!("peer sent inventory: {inv:?}");
                        }
                    },
                    DecodedMessage::Ping(nonce) => {
                        trace!("peer pinged with nonce {nonce}, responding with pong");
                        self.egress_send.send(RawNetworkMessage::new(self.magic, NetworkMessage::Pong(nonce))).await.map_err(|_| P2PError::EgressChannelClosed)?;
                    },
                    DecodedMessage::Headers(headers) => {
                        trace!("peer sent headers {headers:?}, relaying");
                        self.headers_send.send(headers).await.map_err(|_| P2PError::PeerChannelClosed("headers".into()))?;
                    },
                    DecodedMessage::Block(block) => {
                        trace!("peer sent block {block:?}, relaying");
                        self.blocks_send.send(block).await.map_err(|_| P2PError::PeerChannelClosed("blocks".into()))?;
                    },
                    DecodedMessage::Unused => (),
                }
            }
        }

        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), P2PError> {
        info!("sending initial version message to peer...");

        let version_message = RawNetworkMessage::new(self.magic, version_message());

        self.egress_send
            .send(version_message)
            .await
            .map_err(|_| P2PError::EgressChannelClosed)?;

        loop {
            if let Err(err) = self.tick().await {
                error!("peer handler errored, stopping... (reason: {err:?})");

                self.ingress_handle.abort();
                self.egress_handle.abort();

                break Err(err);
            }
        }
    }
}
