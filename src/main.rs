#![no_std]
#![no_main]
use esp_hal::gpio::Level;
use esp_hal::gpio::Output;
use esp_hal::dma::DmaPriority;
use esp_hal::dma::Dma;
use esp_hal::spi::master::Spi;
use defmt_rtt as _;
use heapless::String;
use core::net::Ipv4Addr;
use defmt::info;
use embedded_hal::delay::DelayNs;

use esp_bsp::prelude::*;
use esp_display_interface_spi_dma::display_interface_spi_dma;

use embedded_graphics::{
    mono_font::{ascii::FONT_8X13, MonoTextStyle},
    prelude::{Point, RgbColor},
    text::Text,
    Drawable,
};

use esp_hal::prelude::*;

use embassy_executor::Spawner;
use embassy_net::{tcp::TcpSocket, Runner, StackResources};
use embassy_time::{Duration, Timer};
use embedded_io_async::Write;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup, delay::Delay,};
use esp_println::println;
use esp_wifi::{
    init,
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice, WifiState},
    EspWifiController,
};
use valence_protocol::{Decode, Encode, Packet, PacketDecoder, PacketEncoder, VarInt};

macro_rules! mk_static {
    ($t:ty, $val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");
const SERVER_IP: &str = env!("SERVER_IP");


#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();
    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });

    esp_alloc::heap_allocator!(72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut rng = Rng::new(peripherals.RNG);

    let init = &*mk_static!(
        EspWifiController<'static>,
        init(timg0.timer0, rng.clone(), peripherals.RADIO_CLK).unwrap()
    );

    #[cfg(not(feature = "no-psram"))]
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);
    #[cfg(feature = "no-psram")]
    esp_alloc::heap_allocator!(280 * 1024);

    let spi = lcd_spi!(peripherals);

    info!("SPI ready");

    // Use the `lcd_display_interface` macro to create the display interface
    let di = lcd_display_interface!(peripherals, spi);

    let mut delay = Delay::new();
    delay.delay_ns(500_000u32);

    let mut display = lcd_display!(peripherals, di).init(&mut delay).unwrap();

    // Use the `lcd_backlight_init` macro to turn on the backlight
    lcd_backlight_init!(peripherals);


    Text::new(
        "Initializing...",
        Point::new(80, 110),
        MonoTextStyle::new(&FONT_8X13, RgbColor::WHITE),
    )
        .draw(&mut display)
        .unwrap();

    let wifi = peripherals.WIFI;
    let (wifi_interface, controller) =
        esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice).unwrap();

    cfg_if::cfg_if! {
        if #[cfg(feature = "esp32")] {
            let timg1 = TimerGroup::new(peripherals.TIMG1);
            esp_hal_embassy::init(timg1.timer0);
        } else {
            let timer0 = esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER)
                .split::<esp_hal::timer::systimer::Target>();
            esp_hal_embassy::init(timer0.alarm0);
        }
    }

    let server_ip: Ipv4Addr = SERVER_IP.parse().expect("Invalid SERVER_IP address");
    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    spawner.spawn(connection(controller)).ok();
    spawner.spawn(net_task(runner)).ok();

    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    println!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            println!("Got IP: {}", config.address);
            // Create buffers for the TCP socket
            let mut rx_buffer = [0; 4096];
            let mut tx_buffer = [0; 4096];

            // Create the socket
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

            // Connect to the server
            let remote_endpoint = (SERVER_IP.parse::<Ipv4Addr>().expect("Invalid SERVER_IP address"), 25565);
            if let Err(e) = socket.connect(remote_endpoint).await {
                println!("Failed to connect to server: {:?}", e);
                return;
            }
            println!("Connected to server at {}:{}", remote_endpoint.0, remote_endpoint.1);

            // Pass the socket to run_client
            if let Err(e) = run_client(socket).await {
                println!("Error in run_client: {:?}", e);
            }

            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

}




#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    println!("start connection task");
    println!("Device capabilities: {:?}", controller.capabilities());

    loop {
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await;
            }
            _ => {}
        }

        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.try_into().unwrap(),
                password: PASSWORD.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            println!("Starting wifi");
            controller.start_async().await.unwrap();
            println!("Wifi started!");
        }

        println!("About to connect...");
        match controller.connect_async().await {
            Ok(_) => println!("Wifi connected!"),
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await;
            }
        }
    }
}

async fn run_client(mut socket: TcpSocket<'_>) -> Result<(), ()> {
    let mut dec = PacketDecoder::new();
    let mut enc = PacketEncoder::new();

    // Step 1: Send Handshake
    if let Err(e) = send_handshake(
        &mut socket,
        &mut enc,
        valence_protocol::packets::handshaking::handshake_c2s::HandshakeNextState::Login,
    )
        .await
    {
        println!("Handshake failed: {:?}", e);
        return Err(());
    }

    // Step 2: Login and handle updates
    if let Err(e) = login_and_handle_updates(&mut socket, &mut dec, &mut enc).await {
        println!("Login failed: {:?}", e);
        return Err(());
    }

    Ok(())
}


#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static, WifiStaDevice>>) {
    runner.run().await;

}

async fn send_handshake(
    socket: &mut TcpSocket<'_>,
    enc: &mut PacketEncoder,
    next_state: valence_protocol::packets::handshaking::handshake_c2s::HandshakeNextState,
) -> Result<(), ()> {
    let handshake_packet = valence_protocol::packets::handshaking::handshake_c2s::HandshakeC2s {
        protocol_version: VarInt(763), // Protocol version for Minecraft 1.20
        server_address: valence_protocol::Bounded(SERVER_IP),
        server_port: 25565,
        next_state,
    };

    enc.append_packet(&handshake_packet).expect("Failed to encode handshake packet");
    socket.write(&enc.take()).await.map_err(|_| ())?;
    println!("Handshake sent with next state: {:?}", next_state);
    Ok(())
}

async fn login_and_handle_updates(
    socket: &mut TcpSocket<'_>,
    dec: &mut PacketDecoder,
    enc: &mut PacketEncoder,
) -> Result<(), ()> {
    let login_start_packet = valence_protocol::packets::login::login_hello_c2s::LoginHelloC2s {
        username: valence_protocol::Bounded("ESP32-S3"), // Replace with your username
        profile_id: None, // Optional in offline mode
    };

    enc.append_packet(&login_start_packet).expect("Failed to encode LoginHelloC2s packet");
    let data = enc.take();
    println!("Login start packet: {:?}", data);
    socket.write_all(&data).await.map_err(|_| ())?;
    println!("Login request sent.");

    let mut buf = [0u8; 4096];
    loop {
        let bytes_read = socket.read(&mut buf).await.map_err(|_| ())?;
        if bytes_read == 0 {
            println!("Connection closed by server.");
            return Ok(());
        }

        dec.queue_bytes((&buf[..bytes_read]).into());
        while let Ok(Some(frame)) = dec.try_next_packet() {
            match frame.id {
                valence_protocol::packets::login::LoginCompressionS2c::ID => {
                    let packet: valence_protocol::packets::login::LoginCompressionS2c =
                        frame.decode().expect("Failed to decode LoginCompressionS2c");
                    println!("Compression threshold received: {}", packet.threshold.0);
                    // dec.set_compression(valence_protocol::CompressionThreshold(packet.threshold.0));
                }
                valence_protocol::packets::login::LoginSuccessS2c::ID => {
                    let packet: valence_protocol::packets::login::LoginSuccessS2c =
                        frame.decode().expect("Failed to decode LoginSuccessS2c");
                    println!(
                        "Login successful! Username: {}, UUID: {}",
                        packet.username, packet.uuid
                    );
                }
                valence_protocol::packets::play::GameJoinS2c::ID => {
                    let packet: valence_protocol::packets::play::GameJoinS2c =
                        frame.decode().expect("Failed to decode GameJoinS2c");
                    println!("Joined the game world with Entity ID: {}", packet.entity_id);
                }
                valence_protocol::packets::play::PlayerPositionLookS2c::ID => {
                    let packet: valence_protocol::packets::play::PlayerPositionLookS2c =
                        frame.decode().expect("Failed to decode PlayerPositionLookS2c");
                    println!(
                        "Player position updated: x={}, y={}, z={}, yaw={}, pitch={}",
                        packet.position.x, packet.position.y, packet.position.z, packet.yaw, packet.pitch
                    );
                }
                valence_protocol::packets::play::KeepAliveS2c::ID => {
                    let packet: valence_protocol::packets::play::KeepAliveS2c =
                        frame.decode().expect("Failed to decode KeepAliveS2c");
                    println!("KeepAlive received with ID: {}", packet.id);
                    enc.append_packet(&valence_protocol::packets::play::KeepAliveC2s { id: packet.id })
                        .expect("Failed to encode KeepAliveC2s");
                    socket.write_all(&enc.take()).await.map_err(|_| ())?;
                }
                valence_protocol::packets::play::ChatMessageS2c::ID => {
                    let packet: valence_protocol::packets::play::ChatMessageS2c =
                        frame.decode().expect("Failed to decode ChatMessageS2c");
                    println!("Chat message: {}", packet.message);
                }
                valence_protocol::packets::play::DisconnectS2c::ID => {
                    let packet: valence_protocol::packets::play::DisconnectS2c =
                        frame.decode().expect("Failed to decode DisconnectS2c");
                    println!("Disconnected by server: {}", packet.reason);
                    return Err(()); // Exit loop after disconnect
                }
                valence_protocol::packets::play::HealthUpdateS2c::ID => {
                    let packet: valence_protocol::packets::play::HealthUpdateS2c =
                        frame.decode().expect("Failed to decode HealthUpdateS2c");
                    println!(
                        "Health Update: health={}, saturation={}",
                        packet.health, packet.food_saturation
                    );
                }
                valence_protocol::packets::play::ChunkDataS2c::ID => {
                    println!("Received chunk data.");
                }
                _ => println!("Unhandled packet ID during login/update: {}", frame.id),
            }
        }
    }
}
