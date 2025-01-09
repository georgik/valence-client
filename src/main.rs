#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use bytes::Buf;
use defmt::{info, warn, error};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_alloc::EspHeap;
use esp_hal::prelude::*;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal_embassy::{self as hal_embassy};
use esp_wifi::wifi::Wifi;
use valence_protocol::packets::handshaking::HandshakeC2s;
use valence_protocol::packets::login::{LoginHelloC2s, LoginSuccessS2c, LoginCompressionS2c};
use valence_protocol::packets::play::{GameJoinS2c, PlayerPositionLookS2c, KeepAliveS2c, DisconnectS2c};
use valence_protocol::{Decode, Encode, PacketDecoder, PacketEncoder, VarInt};
use panic_probe as _;

esp_alloc::esp_heap!(96 * 1024);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Initialize peripherals and system timer for Embassy
    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });

    let systimer = SystemTimer::new(peripherals.SYSTIMER).split();
    hal_embassy::init(systimer.alarm0);

    // Initialize heap allocator
    let _heap = EspHeap::init(esp_alloc::heap(), 72 * 1024);

    info!("System initialized!");

    // Initialize the timer and Wi-Fi
    let timer_group = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    let wifi = esp_wifi::init(
        timer_group.timer0,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
    )
        .expect("Failed to initialize Wi-Fi");

    // Configure and initialize Embassy networking
    let stack = configure_network_stack(wifi).await;

    // Spawn the main client task
    if let Err(e) = spawner.spawn(run_client(stack)) {
        error!("Failed to spawn client task: {:?}", e);
    }
}

/// Configures and initializes the network stack
async fn configure_network_stack(wifi: Wifi<'static>) -> Stack<StackResources<1>> {
    let stack_resources = StackResources::<1>::new();
    let network_driver = wifi.into_driver();
    let stack = Stack::new(network_driver, stack_resources);

    // Optionally configure DHCP or static IP here

    info!("Network stack configured!");
    stack
}

/// Main client logic
#[embassy_executor::task]
async fn run_client(stack: Stack<StackResources<1>>) {
    let server_address = "127.0.0.1:25565";

    let mut socket = TcpSocket::new(&stack);
    socket.set_nodelay(true);

    if let Err(e) = socket.connect(server_address).await {
        error!("Failed to connect to server: {:?}", e);
        return;
    }
    info!("Connected to server at {}", server_address);

    let mut dec = PacketDecoder::new();
    let mut enc = PacketEncoder::new();

    // Step 1: Perform handshake
    if let Err(e) = send_handshake(&mut socket, &mut enc).await {
        error!("Handshake failed: {:?}", e);
        return;
    }

    // Step 2: Log in and handle updates
    if let Err(e) = login_and_handle_updates(&mut socket, &mut dec, &mut enc).await {
        error!("Login failed: {:?}", e);
    }
}

/// Sends the handshake packet
async fn send_handshake(socket: &mut TcpSocket<'_>, enc: &mut PacketEncoder) -> Result<(), ()> {
    let handshake_packet = HandshakeC2s {
        protocol_version: VarInt(763), // Minecraft 1.20 protocol version
        server_address: String::from("127.0.0.1"),
        server_port: 25565,
        next_state: valence::protocol::packets::handshaking::handshake_c2s::HandshakeNextState::Login,
    };

    enc.append_packet(&handshake_packet)
        .expect("Failed to encode handshake packet");
    socket.write_all(&enc.take()).await.map_err(|_| ())?;
    info!("Handshake sent!");
    Ok(())
}

/// Handles login and subsequent server updates
async fn login_and_handle_updates(
    socket: &mut TcpSocket<'_>,
    dec: &mut PacketDecoder,
    enc: &mut PacketEncoder,
) -> Result<(), ()> {
    // Send login start packet
    let login_start_packet = LoginHelloC2s {
        username: String::from("Player"),
        profile_id: None,
    };

    enc.append_packet(&login_start_packet)
        .expect("Failed to encode LoginHelloC2s packet");
    socket.write_all(&enc.take()).await.map_err(|_| ())?;
    info!("Login request sent.");

    let mut buf = [0u8; 4096];
    loop {
        let bytes_read = socket.read(&mut buf).await.map_err(|_| ())?;
        if bytes_read == 0 {
            info!("Connection closed by server.");
            return Ok(());
        }

        dec.queue_bytes(buf[..bytes_read].into());

        while let Ok(Some(frame)) = dec.try_next_packet() {
            match frame.id {
                LoginCompressionS2c::ID => {
                    let packet: LoginCompressionS2c = frame.decode().expect("Failed to decode");
                    info!(
                        "Received LoginCompressionS2c packet with threshold: {}",
                        packet.threshold.0
                    );
                    dec.set_compression(valence::protocol::CompressionThreshold(packet.threshold.0));
                }
                LoginSuccessS2c::ID => {
                    let packet: LoginSuccessS2c = frame.decode().expect("Failed to decode");
                    info!(
                        "Login successful! Username: {}, UUID: {}",
                        packet.username,
                        packet.uuid
                    );
                }
                GameJoinS2c::ID => {
                    let packet: GameJoinS2c = frame.decode().expect("Failed to decode");
                    info!("Joined the game world with Entity ID: {}", packet.entity_id);
                }
                PlayerPositionLookS2c::ID => {
                    let packet: PlayerPositionLookS2c = frame.decode().expect("Failed to decode");
                    info!(
                        "Player position updated: x={}, y={}, z={}, yaw={}, pitch={}",
                        packet.position.x,
                        packet.position.y,
                        packet.position.z,
                        packet.yaw,
                        packet.pitch
                    );
                }
                KeepAliveS2c::ID => {
                    let packet: KeepAliveS2c = frame.decode().expect("Failed to decode");
                    info!("KeepAlive received with ID: {}", packet.id);
                }
                DisconnectS2c::ID => {
                    let packet: DisconnectS2c = frame.decode().expect("Failed to decode");
                    info!("Disconnected by server: {}", packet.reason);
                    return Ok(());
                }
                _ => warn!("Unhandled packet ID: {}", frame.id),
            }
        }
    }
}
