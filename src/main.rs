#![no_std]
#![no_main]
extern crate alloc;
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
use alloc::vec::Vec;
use crate::alloc::string::ToString;
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
use esp_println::{print, println};
use esp_wifi::{
    init,
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice, WifiState},
    EspWifiController,
};
use valence_protocol::{Decode, Encode, Packet, PacketDecoder, PacketEncoder, VarInt};
use valence_protocol::packets::login::{LoginHelloC2s, LoginSuccessS2c, LoginCompressionS2c};
use valence_protocol::packets::play::{GameJoinS2c, KeepAliveS2c, KeepAliveC2s, PlayerPositionLookS2c, PlayerAbilitiesS2c, ChunkDataS2c, ChatMessageS2c, DisconnectS2c, EntityStatusS2c, PlayerListS2c, PlayerRespawnS2c, PlayerSpawnPositionS2c, CommandTreeS2c, UpdateSelectedSlotS2c, AdvancementUpdateS2c, HealthUpdateS2c, EntityAttributesS2c, SynchronizeTagsS2c};
use valence_protocol::packets::status::{QueryRequestC2s, QueryResponseS2c};


macro_rules! mk_static {
    ($t:ty, $val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");
const SERVER_IP: &str = env!("SERVER_IP");


// Graphical logging
use core::fmt::Write as FmtWrite;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::Size;
use embedded_graphics::primitives::Rectangle;

const LOG_CAPACITY: usize = 1024; // Total characters for logging
const SCREEN_WIDTH: u32 = 320; // Adjust based on your display
const SCREEN_HEIGHT: u32 = 240; // Adjust based on your display
const LINE_HEIGHT: u32 = 14; // Line height for the chosen font

pub struct Logger<'a, D>
where
    D: embedded_graphics::draw_target::DrawTarget<Color = Rgb565>,
{
    buffer: String<LOG_CAPACITY>, // Logging buffer
    display: &'a mut D,           // Reference to the display
    text_style: MonoTextStyle<'static, Rgb565>, // Text style
    scroll_offset: usize,         // Offset for scrolling
}

impl<'a, D> Logger<'a, D>
where
    D: embedded_graphics::draw_target::DrawTarget<Color = Rgb565>,
{
    pub fn new(display: &'a mut D) -> Self {
        Self {
            buffer: String::new(),
            display,
            text_style: MonoTextStyle::new(&FONT_8X13, Rgb565::WHITE),
            scroll_offset: 0,
        }
    }

    /// Log a message to the buffer and display it.
    pub fn log(&mut self, message: &str) {
        println!("{}", message);
        // Add message to the buffer
        writeln!(self.buffer, "{}", message).ok();

        if self.buffer.len() > LOG_CAPACITY {
            let excess = self.buffer.len() - LOG_CAPACITY;

            // Create a temporary copy of the truncated string
            let truncated = &self.buffer[excess..].to_string();

            self.buffer.clear();
            self.buffer.push_str(truncated).unwrap(); // Push the truncated portion back into the buffer
        }

        self.render();
    }

    /// Render the log to the display.
    fn render(&mut self) {
        // Clear the screen
        self.display
            .fill_solid(
                &Rectangle::new(
                    Point::new(0, 0),
                    Size::new(SCREEN_WIDTH, SCREEN_HEIGHT),
                ),
                Rgb565::BLACK,
            )
            .ok();


        // Split the buffer into lines
        let lines: Vec<&str> = self
            .buffer
            .lines()
            .skip(self.scroll_offset)
            .collect();

        // Draw each visible line
        for (i, line) in lines.iter().take((SCREEN_HEIGHT / LINE_HEIGHT) as usize).enumerate() {
            Text::new(line, Point::new(0, (i as u32 * LINE_HEIGHT) as i32), self.text_style)
                .draw(self.display)
                .ok();
        }
    }

    /// Scroll the log up or down.
    pub fn scroll(&mut self, direction: i32) {
        self.scroll_offset = (self.scroll_offset as i32 + direction)
            .max(0)
            .min(self.buffer.lines().count() as i32 - 1) as usize;
        self.render();
    }
}


#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    print!("System starting up...");
    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });
    println!(" ok");

    esp_println::logger::init_logger_from_env();

    // print!("PSRAM...");
    // esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);
    // println!(" ok");

    const memory_size: usize = 160 * 1024;
    print!("Initializing allocator with {} bytes...", memory_size);
    esp_alloc::heap_allocator!(memory_size);
    println!(" ok");


    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut rng = Rng::new(peripherals.RNG);

    let init = &*mk_static!(
        EspWifiController<'static>,
        init(timg0.timer0, rng.clone(), peripherals.RADIO_CLK).unwrap()
    );


    let spi = lcd_spi!(peripherals);

    info!("SPI ready");

    // Use the `lcd_display_interface` macro to create the display interface
    let di = lcd_display_interface!(peripherals, spi);

    let mut delay = Delay::new();
    delay.delay_ns(500_000u32);

    let mut display = lcd_display!(peripherals, di).init(&mut delay).unwrap();

    // Use the `lcd_backlight_init` macro to turn on the backlight
    lcd_backlight_init!(peripherals);

    let mut logger = Logger::new(&mut display);
    // Text::new(
    //     "Initializing...",
    //     Point::new(80, 110),
    //     MonoTextStyle::new(&FONT_8X13, RgbColor::WHITE),
    // )
    //     .draw(&mut display)
    //     .unwrap();
    logger.log("Initializing...");


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

    logger.log("Waiting to get IP address...");
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
            logger.log("Connected to server!");
            logger.log(&*remote_endpoint.0.to_string());

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
            println!("Received packet ID: {}", frame.id);
            match frame.id {
                LoginCompressionS2c::ID => {
                    let packet: valence_protocol::packets::login::LoginCompressionS2c =
                        frame.decode().expect("Failed to decode LoginCompressionS2c");
                    println!("Compression threshold received: {}", packet.threshold.0);
                    dec.set_compression(valence_protocol::CompressionThreshold(packet.threshold.0));
                }
                LoginSuccessS2c::ID => {
                    let packet: LoginSuccessS2c =
                        frame.decode().expect("Failed to decode LoginSuccessS2c");
                    println!(
                        "Login successful! Username: {}, UUID: {}",
                        packet.username, packet.uuid
                    );
                }
                GameJoinS2c::ID => {
                    let packet: GameJoinS2c =
                        frame.decode().expect("Failed to decode GameJoinS2c");
                    println!("Joined the game world with Entity ID: {}", packet.entity_id);
                }
                PlayerPositionLookS2c::ID => {
                    let packet: PlayerPositionLookS2c =
                        frame.decode().expect("Failed to decode PlayerPositionLookS2c");
                    println!(
                        "Player position updated: x={}, y={}, z={}, yaw={}, pitch={}",
                        packet.position.x, packet.position.y, packet.position.z, packet.yaw, packet.pitch
                    );
                }
                KeepAliveS2c::ID => {
                    let packet: KeepAliveS2c =
                        frame.decode().expect("Failed to decode KeepAliveS2c");
                    println!("KeepAlive received with ID: {}", packet.id);
                    enc.append_packet(&KeepAliveC2s { id: packet.id })
                        .expect("Failed to encode KeepAliveC2s");
                    socket.write_all(&enc.take()).await.map_err(|_| ())?;
                }
                ChatMessageS2c::ID => {
                    let packet: ChatMessageS2c =
                        frame.decode().expect("Failed to decode ChatMessageS2c");
                    println!("Chat message: {}", packet.message);
                }
                DisconnectS2c::ID => {
                    let packet: DisconnectS2c =
                        frame.decode().expect("Failed to decode DisconnectS2c");
                    println!("Disconnected by server: {}", packet.reason);
                    return Err(()); // Exit loop after disconnect
                }
                HealthUpdateS2c::ID => {
                    let packet: HealthUpdateS2c =
                        frame.decode().expect("Failed to decode HealthUpdateS2c");
                    println!(
                        "Health Update: health={}, saturation={}",
                        packet.health, packet.food_saturation
                    );
                }
                ChunkDataS2c::ID => {
                    println!("Received chunk data.");
                }
                _ => println!("Unhandled packet ID during login/update: {}", frame.id),
            }
        }
    }
}
