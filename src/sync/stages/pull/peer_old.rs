use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use bitcoin::{
    Block, BlockHash, VarInt,
    block::Header,
    consensus::{self, Decodable, ReadExt, encode},
    hashes::Hash,
    secp256k1,
};
use bitcoin::{
    consensus::Encodable,
    p2p::{
        Address, Magic, ServiceFlags,
        message::{self, CommandString, NetworkMessage},
        message_blockdata::{GetHeadersMessage, Inventory},
        message_network,
    },
    secp256k1::rand::Rng,
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
use tracing::{debug, error, info, trace};

// TODO: use peer_wip

fn build_version_message() -> NetworkMessage {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time error")
        .as_secs() as i64;

    let services = ServiceFlags::NONE;

    let version = NetworkMessage::Version(message_network::VersionMessage {
        version: 70003,
        services,
        timestamp,
        receiver: Address::new(&addr, services),
        sender: Address::new(&addr, services),
        nonce: secp256k1::rand::thread_rng().r#gen(),
        user_agent: format!("/symphony:0.1.0/"),
        start_height: 0,
        relay: false,
    });
    return version;
}

#[derive(Debug)]
struct RawNetworkMessage {
    cmd: CommandString,
    raw: Vec<u8>,
}

impl RawNetworkMessage {
    fn parse(self) -> Result<ParsedNetworkMessage, consensus::encode::Error> {
        let mut raw: &[u8] = &self.raw;
        let payload = match self.cmd.as_ref() {
            "version" => ParsedNetworkMessage::Version(Decodable::consensus_decode(&mut raw)?),
            "verack" => ParsedNetworkMessage::Verack,
            "inv" => ParsedNetworkMessage::Inv(Decodable::consensus_decode(&mut raw)?),
            "block" => ParsedNetworkMessage::Block(self.raw),
            "headers" => {
                let len = VarInt::consensus_decode(&mut raw)?.0;
                let mut headers = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    headers.push(Block::consensus_decode(&mut raw)?.header);
                }
                ParsedNetworkMessage::Headers(headers)
            }
            "ping" => ParsedNetworkMessage::Ping(Decodable::consensus_decode(&mut raw)?),
            "pong" => ParsedNetworkMessage::Ignored, // unused
            "addr" => ParsedNetworkMessage::Ignored, // unused
            "alert" => ParsedNetworkMessage::Ignored, // https://bitcoin.org/en/alert/2016-11-01-alert-retirement
            _ => panic!(
                "unsupported message: command={}, payload={:?}",
                self.cmd, self.raw
            ),
        };
        Ok(payload)
    }
}

pub(crate) type SerBlock = Vec<u8>;

#[derive(Debug)]
enum ParsedNetworkMessage {
    Version(message_network::VersionMessage),
    Verack,
    Inv(Vec<Inventory>),
    Ping(u64),
    Headers(Vec<Header>),
    Block(SerBlock),
    Ignored,
}

impl Decodable for RawNetworkMessage {
    fn consensus_decode<D: bitcoin::io::Read + ?Sized>(d: &mut D) -> Result<Self, encode::Error> {
        let _magic: Magic = Decodable::consensus_decode(d)?;
        let cmd = Decodable::consensus_decode(d)?;

        let len = u32::consensus_decode(d)?;
        let _checksum = <[u8; 4]>::consensus_decode(d)?; // assume data is correct
        let mut raw = vec![0u8; len as usize];
        d.read_slice(&mut raw)?;

        Ok(RawNetworkMessage { cmd, raw })
    }
}

// -- end from metashrew --

pub struct Peer {
    request_send: mpsc::Sender<Request>,
    headers_recv: mpsc::Receiver<Vec<Header>>,
    blocks_recv: mpsc::Receiver<RawBlock>,
    pub new_block_notification: Arc<Notify>,
    pub handler: JoinHandle<()>,
}

type RawBlock = Vec<u8>;

#[derive(Debug)]
enum Request {
    GetNewHeaders(GetHeadersMessage),
    GetBlocks(Vec<Inventory>),
}

impl Request {
    // https://en.bitcoin.it/wiki/Protocol_documentation#getheaders
    fn get_new_headers(intersects: Vec<BlockHash>) -> Request {
        Request::GetNewHeaders(GetHeadersMessage::new(intersects, BlockHash::all_zeros()))
    }

    // https://en.bitcoin.it/wiki/Protocol_documentation#getdata
    fn get_blocks(blockhashes: &[BlockHash]) -> Request {
        Request::GetBlocks(
            blockhashes
                .iter()
                .map(|blockhash| Inventory::WitnessBlock(*blockhash))
                .collect(),
        )
    }
}

#[derive(Debug, Error)]
pub enum Error {
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

impl Peer {
    pub async fn connect<A: ToSocketAddrs>(addr: A, magic: Magic) -> Result<Self, Error> {
        let stream = TcpStream::connect(addr)
            .await
            .map_err(Error::ConnectFailure)?;

        // configure tcp
        let sock_ref = socket2::SockRef::from(&stream);
        let mut tcp_keepalive = socket2::TcpKeepalive::new();
        tcp_keepalive = tcp_keepalive.with_time(tokio::time::Duration::from_secs(20));
        tcp_keepalive = tcp_keepalive.with_interval(tokio::time::Duration::from_secs(20));
        sock_ref
            .set_tcp_keepalive(&tcp_keepalive)
            .map_err(Error::ConnectFailure)?;
        sock_ref.set_nodelay(true).map_err(Error::ConnectFailure)?;
        // sock_ref
        //     .set_linger(Some(std::time::Duration::from_secs(0)))
        //     .map_err(Error::ConnectFailure)?;

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
    ) -> Result<Vec<Header>, Error> {
        debug!("get_new_headers: {:?}", intersects);

        self.request_send
            .send(Request::get_new_headers(intersects))
            .await
            .map_err(|_| Error::EgressChannelClosed)?;

        let headers = self
            .headers_recv
            .recv()
            .await
            .ok_or(Error::PeerChannelClosed("headers".into()))?;

        debug!("get_new_headers finished");

        Ok(headers)
    }

    pub async fn get_blocks(&mut self, hashes: Vec<BlockHash>) -> Result<Vec<Block>, Error> {
        debug!("get_blocks: {:?} - {:?}", hashes.first(), hashes.last());

        self.request_send
            .send(Request::get_blocks(hashes.as_slice()))
            .await
            .unwrap();

        let mut blocks = Vec::with_capacity(hashes.len());

        debug!("get_blocks downloading");

        for _ in hashes.iter() {
            let raw = self
                .blocks_recv
                .recv()
                .await
                .ok_or(Error::PeerChannelClosed("blocks".into()))?;

            blocks.push(raw)
        }

        debug!("get_blocks finished downloading, decoding...");

        let mut decoded_blocks = Vec::with_capacity(hashes.len());

        // receive each block
        for (hash, raw) in hashes.into_iter().zip(blocks.into_iter()) {
            let block = Block::consensus_decode_from_finite_reader(&mut &raw[..])
                .map_err(Error::IngressDecoding)?;

            // check the received block matches the list of hashes we requested
            if block.block_hash() != hash {
                return Err(Error::GetBlocksMismatch(block.block_hash(), hash));
            }

            decoded_blocks.push(block);
        }

        debug!("get_blocks finished decoding");

        Ok(decoded_blocks)
    }
}

struct Ingress {
    stream_read: OwnedReadHalf,
    // relay decoded message to appropriate output
    rx_send: mpsc::Sender<ParsedNetworkMessage>,
}

impl Ingress {
    pub fn new(stream_read: OwnedReadHalf, rx_send: mpsc::Sender<ParsedNetworkMessage>) -> Self {
        Self {
            stream_read,
            rx_send,
        }
    }

    pub async fn read_message(&mut self) -> Result<ParsedNetworkMessage, Error> {
        let mut header = [0u8; 24];
        self.stream_read
            .read_exact(&mut header)
            .await
            .map_err(Error::IngressIO)?;

        let _magic: Magic = Decodable::consensus_decode_from_finite_reader(&mut &header[0..4])
            .map_err(Error::IngressDecoding)?;

        let cmd = Decodable::consensus_decode_from_finite_reader(&mut &header[4..16])
            .map_err(Error::IngressDecoding)?;

        let length = u32::consensus_decode_from_finite_reader(&mut &header[16..20])
            .map_err(Error::IngressDecoding)?;

        let _checksum = <[u8; 4]>::consensus_decode_from_finite_reader(&mut &header[20..24])
            .map_err(Error::IngressDecoding)?;

        let mut payload = vec![0u8; length as usize];
        self.stream_read
            .read_exact(&mut payload)
            .await
            .map_err(Error::IngressIO)?;

        let parsed = RawNetworkMessage { cmd, raw: payload }
            .parse()
            .map_err(Error::IngressDecoding)?;

        Ok(parsed)
    }

    async fn tick(&mut self) -> Result<(), Error> {
        let msg = self.read_message().await?;

        trace!("ingress received msg: {msg:?}");

        self.rx_send
            .send(msg)
            .await
            .map_err(|_| Error::IngressChannelClosed)
    }

    pub async fn run(&mut self) -> Result<(), Error> {
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
    tx_recv: mpsc::Receiver<message::RawNetworkMessage>,
}

impl Egress {
    pub fn new(
        stream_write: OwnedWriteHalf,
        tx_recv: mpsc::Receiver<message::RawNetworkMessage>,
    ) -> Self {
        Self {
            stream_write,
            tx_recv,
        }
    }

    pub async fn write_message(
        &mut self,
        msg: message::RawNetworkMessage,
    ) -> Result<(), std::io::Error> {
        let mut buf = vec![];
        msg.consensus_encode(&mut buf)?;
        self.stream_write.write_all(&buf).await?;
        self.stream_write.flush().await?;

        Ok(())
    }

    // TODO: error on channel close?
    async fn tick(&mut self) -> Result<(), Error> {
        let msg = self.tx_recv.recv().await;

        if let Some(x) = msg {
            trace!("egress received msg: {x:?}");
            self.write_message(x).await.map_err(Error::EgressIO)?
        }

        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), Error> {
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
    ingress_handle: JoinHandle<Result<(), Error>>,
    ingress_recv: mpsc::Receiver<ParsedNetworkMessage>,
    egress_handle: JoinHandle<Result<(), Error>>,
    egress_send: mpsc::Sender<message::RawNetworkMessage>,
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

    async fn tick(&mut self) -> Result<(), Error> {
        select! {
            maybe_req = self.request_recv.recv() => {
                let req = maybe_req.ok_or(Error::PeerChannelClosed("request".into()))?;
                trace!("peer handler received request: {req:?}");

                let to_send = match req {
                    Request::GetNewHeaders(msg) => NetworkMessage::GetHeaders(msg),
                    Request::GetBlocks(inv) => NetworkMessage::GetData(inv),
                };

                let to_send = message::RawNetworkMessage::new(self.magic, to_send);

                self.egress_send.send(to_send).await.map_err(|_| Error::EgressChannelClosed)?;
            }
            maybe_msg = self.ingress_recv.recv() => {
                let msg = maybe_msg.ok_or(Error::IngressChannelClosed)?;

                trace!("peer handler received message: {msg:?}");

                match msg {
                    ParsedNetworkMessage::Version(ver) => {
                        debug!("peer reported version {ver:?}, responding with ack");
                        self.egress_send.send(message::RawNetworkMessage::new(self.magic, NetworkMessage::Verack)).await.map_err(|_| Error::EgressChannelClosed)?;
                    },
                    ParsedNetworkMessage::Verack => {
                        debug!("peer sent verack, sending init notification");
                        self.init_notifier.notify_one();
                    },
                    ParsedNetworkMessage::Inv(inv) => {
                        if inv.iter().any(|i| matches!(i, Inventory::Block(_))) {
                            trace!("peer sent inventory containing block: {inv:?}, sending new block notification");
                            self.new_block_notifier.notify_one();
                        } else {
                            trace!("peer sent inventory: {inv:?}");
                        }
                    },
                    ParsedNetworkMessage::Ping(nonce) => {
                        debug!("peer pinged with nonce {nonce}, responding with pong");
                        self.egress_send.send(message::RawNetworkMessage::new(self.magic, NetworkMessage::Pong(nonce))).await.map_err(|_| Error::EgressChannelClosed)?;
                    },
                    ParsedNetworkMessage::Headers(headers) => {
                        trace!("peer sent headers {headers:?}, relaying");
                        self.headers_send.send(headers).await.map_err(|_| Error::PeerChannelClosed("headers".into()))?;
                    },
                    ParsedNetworkMessage::Block(block) => {
                        trace!("peer sent block {block:?}, relaying");
                        self.blocks_send.send(block).await.map_err(|_| Error::PeerChannelClosed("blocks".into()))?;
                    },
                    ParsedNetworkMessage::Ignored => (),
                }
            }
        }

        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        info!("sending version message to node...");

        let version_message = message::RawNetworkMessage::new(self.magic, build_version_message());

        self.egress_send
            .send(version_message)
            .await
            .map_err(|_| Error::EgressChannelClosed)?;

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
