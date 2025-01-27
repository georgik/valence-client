#![no_std]
#![no_main]
extern crate alloc;
use esp_hal::gpio::Level;
use esp_hal::gpio::Output;
use esp_hal::dma::DmaPriority;
use esp_hal::dma::Owner::Dma;
use esp_hal::spi::master::Spi;
use esp_hal::timer::systimer::SystemTimer;
use defmt_rtt as _;
use heapless::String;
use core::net::Ipv4Addr;
use defmt::info;
use embedded_hal::delay::DelayNs;
use alloc::vec::Vec;
use crate::alloc::string::ToString;
#[cfg(feature = "gui")]
use esp_bsp::prelude::*;
#[cfg(feature = "gui")]
use esp_display_interface_spi_dma::display_interface_spi_dma;

#[cfg(feature = "gui")]
use embedded_graphics::{
    mono_font::{ascii::FONT_8X13, MonoTextStyle},
    prelude::{Point, RgbColor},
    text::Text,
    Drawable,
};

#[cfg(feature = "gui")]
use esp_hal::prelude::*;

use embassy_executor::Spawner;
use embassy_net::{tcp::TcpSocket, Runner, StackResources};
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::Write;
use esp_alloc as _;
use esp_alloc::HeapStats;
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup, delay::Delay,};
use esp_println::{print, println};
use esp_wifi::{
    init,
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice, WifiState},
    EspWifiController,
};
use valence_protocol::{Bounded, Decode, Encode, Packet, PacketDecoder, PacketEncoder, VarInt};
use valence_protocol::packets::login::{LoginHelloC2s, LoginSuccessS2c, LoginCompressionS2c};
use valence_protocol::packets::play::{GameJoinS2c, KeepAliveS2c, KeepAliveC2s, PlayerPositionLookS2c, PlayerAbilitiesS2c, ChunkDataS2c, ChatMessageS2c, DisconnectS2c, EntityStatusS2c, PlayerListS2c, PlayerRespawnS2c, PlayerSpawnPositionS2c, CommandTreeS2c, UpdateSelectedSlotS2c, AdvancementUpdateS2c, HealthUpdateS2c, EntityAttributesS2c, SynchronizeTagsS2c, ScreenHandlerSlotUpdateS2c, ChatMessageC2s, GameMessageS2c, EntitySetHeadYawS2c, RotateS2c};
use valence_protocol::packets::status::{QueryRequestC2s, QueryResponseS2c};

use esp_hal::{rmt::Rmt, time::RateExtU32};
use esp_hal_smartled::{smartLedBuffer, SmartLedsAdapter};

use smart_leds::{brightness, gamma, hsv::{hsv2rgb, Hsv}, SmartLedsWrite, RGB8};

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use esp_hal::rmt::TxChannel;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;


// Define a static channel with a capacity of 1 for `HardwareEvent`s.
static CHANNEL: Channel<CriticalSectionRawMutex, HardwareEvent, 1> = Channel::new();


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
use embassy_futures::yield_now;
#[cfg(feature = "gui")]
use embedded_graphics::{pixelcolor::Rgb565, prelude::Size, primitives::Rectangle};

const LOG_CAPACITY: usize = 1024; // Total characters for logging
const SCREEN_WIDTH: u32 = 320; // Adjust based on your display
const SCREEN_HEIGHT: u32 = 240; // Adjust based on your display
const LINE_HEIGHT: u32 = 14; // Line height for the chosen font

#[cfg(feature = "gui")]
pub struct Logger<'a, D>
where
    D: embedded_graphics::draw_target::DrawTarget<Color = Rgb565>,
{
    buffer: String<LOG_CAPACITY>, // Logging buffer
    display: &'a mut D,           // Reference to the display
    text_style: MonoTextStyle<'static, Rgb565>, // Text style
    scroll_offset: usize,         // Offset for scrolling
}
#[cfg(feature = "gui")]
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

fn heap_stats() {
    let stats: HeapStats = esp_alloc::HEAP.stats();
    // HeapStats implements the Display and defmt::Format traits, so you can pretty-print the heap stats.
    println!("{}", stats);

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


    const memory_size: usize = 300 * 1024;
    print!("Initializing allocator with {} bytes...", memory_size);
    esp_alloc::heap_allocator!(memory_size);
    println!(" ok");


    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut rng = Rng::new(peripherals.RNG);

    let init = &*mk_static!(
        EspWifiController<'static>,
        init(timg0.timer0, rng.clone(), peripherals.RADIO_CLK).unwrap()
    );

    let led_pin = peripherals.GPIO8;
    let freq = 80.MHz();
    let rmt = Rmt::new(peripherals.RMT, freq).unwrap();
    let rmt_buffer = smartLedBuffer!(1);
    let mut led = SmartLedsAdapter::new(rmt.channel0, led_pin, rmt_buffer);
    // Set the RGB color (e.g., Red)
    let color = RGB8 { r: 0, g: 0, b: 255 };

    // Write color data to NeoPixel with gamma correction and brightness adjustment
    led.write(brightness(gamma(core::iter::once(color)), 10))
        .unwrap();

    #[cfg(feature = "gui")]
    let spi = lcd_spi!(peripherals);

    info!("SPI ready");

    // Use the `lcd_display_interface` macro to create the display interface
    #[cfg(feature = "gui")]
    let di = lcd_display_interface!(peripherals, spi);

    // let mut delay = Delay::new();
    // delay.delay_ns(500_000u32);

    #[cfg(feature = "gui")]
    let mut display = lcd_display!(peripherals, di).init(&mut delay).unwrap();

    // Use the `lcd_backlight_init` macro to turn on the backlight
    #[cfg(feature = "gui")]
    lcd_backlight_init!(peripherals);

    #[cfg(feature = "gui")]
    let mut logger = Logger::new(&mut display);
    // Text::new(
    //     "Initializing...",
    //     Point::new(80, 110),
    //     MonoTextStyle::new(&FONT_8X13, RgbColor::WHITE),
    // )
    //     .draw(&mut display)
    //     .unwrap();
    #[cfg(feature = "gui")]
    logger.log("Initializing...");


    let wifi = peripherals.WIFI;
    let (wifi_interface, controller) =
        esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice).unwrap();

    #[cfg(feature = "psram")]
    {
        print!("init_psram... ");
        esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);
        println!("ok");
    }

    heap_stats();

    let systimer = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(systimer.alarm0);

    let server_ip: Ipv4Addr = SERVER_IP.parse().expect("Invalid SERVER_IP address");
    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    spawner
        .spawn(hardware_task_runner(led, CHANNEL.receiver()))
        .unwrap();

    spawner.spawn(connection(controller)).ok();
    spawner.spawn(net_task(runner)).ok();
    spawner.spawn(tick_task()).ok();

    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    #[cfg(feature = "gui")]
    logger.log("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            println!("Got IP: {}", config.address);
            #[cfg(feature = "gui")]
            logger.log("Got IP address:");
            #[cfg(feature = "gui")]
            logger.log(&config.address.to_string());
            // Create buffers for the TCP socket
            let mut rx_buffer = [0; 4096];
            let mut tx_buffer = [0; 4096];

            // Create the socket
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

            // Connect to the server
            let remote_endpoint = (SERVER_IP.parse::<Ipv4Addr>().expect("Invalid SERVER_IP address"), 25566);
            #[cfg(feature = "gui")]
            logger.log("Connecting to server:");
            #[cfg(feature = "gui")]
            logger.log(&*remote_endpoint.0.to_string());

            if let Err(e) = socket.connect(remote_endpoint).await {
                println!("Failed to connect to server: {:?}", e);
                #[cfg(feature = "gui")]
                logger.log("Failed to connect to server");
                return;
            }
            println!("Connected to server at {}:{}", remote_endpoint.0, remote_endpoint.1);
            #[cfg(feature = "gui")]
            logger.log("Connected.");

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
        server_port: 25566,
        next_state,
    };

    enc.append_packet(&handshake_packet).expect("Failed to encode handshake packet");
    socket.write(&enc.take()).await.map_err(|_| ())?;
    println!("Handshake sent with next state: {:?}", next_state);
    Ok(())
}

#[embassy_executor::task]
async fn tick_task() {
    loop {
        println!("Tick...");
        yield_now().await;
        Timer::after(Duration::from_secs(1)).await;
    }
}

#[derive(Debug)]
enum HardwareEvent {
    ToggleLed,
    // Future events can be added here (e.g., ButtonPressed, DisplayUpdate, etc.)
}


#[embassy_executor::task]
async fn hardware_task_runner(
    mut led: SmartLedsAdapter<esp_hal::rmt::Channel<esp_hal::Blocking, 0>, 25>,
    receiver: embassy_sync::channel::Receiver<'static, CriticalSectionRawMutex, HardwareEvent, 1>,
) {
    let mut toggle_state: u8 = 0;

    loop {
        let event = receiver.receive().await;

        match event {
            HardwareEvent::ToggleLed => {
                println!("Toggle led");
                toggle_state = (toggle_state + 1) % 3;
                let color = match toggle_state {
                    0 => RGB8 { r: 255, g: 0, b: 0 }, // Red
                    1 => RGB8 { r: 0, g: 255, b: 0 }, // Green
                    _ => RGB8 { r: 0, g: 0, b: 0 },   // Off
                };

                led.write(brightness(gamma(core::iter::once(color)), 10))
                    .unwrap();
            }
        }
        yield_now().await;
    }
}


async fn login_and_handle_updates(
    socket: &mut TcpSocket<'_>,
    dec: &mut PacketDecoder,
    enc: &mut PacketEncoder,
) -> Result<(), ()> {
    let sender = CHANNEL.sender();
    let login_start_packet = valence_protocol::packets::login::login_hello_c2s::LoginHelloC2s {
        username: valence_protocol::Bounded("ESP32-S3"), // Replace with your username
        profile_id: None, // Optional in offline mode
    };

    enc.append_packet(&login_start_packet).expect("Failed to encode LoginHelloC2s packet");
    let data = enc.take();
    println!("Login start packet: {:?}", data);
    socket.write_all(&data).await.map_err(|_| ())?;
    println!("Login request sent.");

    let mut buf = Vec::with_capacity(4096);
    buf.resize(4096, 0);
    loop {
        let bytes_read = socket.read(&mut buf).await.map_err(|_| ())?;
        if bytes_read == 0 {
            println!("Connection closed by server.");
            return Ok(());
        }
        println!("Received {} bytes", bytes_read);
        heap_stats();
        // println!("Received data: {:?}", &buf[..bytes_read]);

        dec.queue_bytes((&buf[..bytes_read]).into());
        while let Ok(Some(frame)) = dec.try_next_packet() {
            println!("Received packet ID: 0x{:X}", frame.id);
            match frame.id {
                LoginCompressionS2c::ID => {
                    println!("LoginCompressionS2c");
                    // let packet: LoginCompressionS2c = frame.decode().expect("Failed to decode LoginCompressionS2c");
                    let threshold = 256;
                    // let threshold = packet.threshold.0;
                    println!("Compression threshold received: {}", threshold);

                    // Set compression threshold for decoder and encoder
                    dec.set_compression(valence_protocol::CompressionThreshold(threshold));
                    enc.set_compression(valence_protocol::CompressionThreshold(threshold));
                }

                LoginSuccessS2c::ID => {
                    heap_stats();
                    sender.try_send(HardwareEvent::ToggleLed).unwrap();
                    let packet: LoginSuccessS2c =
                        frame.decode().expect("Failed to decode LoginSuccessS2c");
                    println!(
                        "Login successful! Username: {}, UUID: {}",
                        packet.username, packet.uuid
                    );
                }
                GameJoinS2c::ID => {
                    // Assuming the player successfully joined the game world.
                    println!("GameJoin - skipping deserialization - requires binary compound support");

                }
                PlayerPositionLookS2c::ID => {
                    let packet: PlayerPositionLookS2c =
                        frame.decode().expect("Failed to decode PlayerPositionLookS2c");
                    println!(
                        "Player position look: x={}, y={}, z={}, yaw={}, pitch={}",
                        packet.position.x, packet.position.y, packet.position.z, packet.yaw, packet.pitch
                    );
                }
                KeepAliveS2c::ID => {
                    let packet: KeepAliveS2c = frame.decode().expect("Failed to decode KeepAliveS2c");
                    println!("KeepAlive received with ID: {}", packet.id);

                    // Encode the KeepAliveC2s response
                    enc.clear();
                    enc.append_packet(&KeepAliveC2s { id: packet.id })
                        .expect("Failed to encode KeepAliveC2s");

                    let data = enc.take();

                    println!("Encoded KeepAliveC2s packet: {:?}", data);

                    // Send the packet to the server
                    match socket.write_all(&data).await {
                        Ok(_) => {
                            println!("Successfully sent KeepAliveC2s with ID: {}", packet.id);
                        }
                        Err(e) => {
                            println!(
                                "Failed to send KeepAliveC2s with ID: {}. Error: {:?}",
                                packet.id, e
                            );
                            return Err(()); // Handle error
                        }
                    }
                    socket.flush().await.unwrap();
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
                PlayerSpawnPositionS2c::ID => {
                    // let packet: PlayerSpawnPositionS2c =
                    //     frame.decode().expect("Failed to decode PlayerSpawnPositionS2c");
                    // println!(
                    //     "Player spawn position: x={}, y={}, z={}",
                    //     packet.position.x, packet.position.y, packet.position.z
                    // );
                    println!("PlayerSpawnPositionS2c");
                }
                PlayerAbilitiesS2c::ID => {
                    heap_stats();
                    let packet: PlayerAbilitiesS2c =
                        frame.decode().expect("Failed to decode PlayerAbilitiesS2c");
                    println!("Player abilities: {:?}", packet.flags);
                }
                EntityStatusS2c::ID => {
                    let packet: EntityStatusS2c =
                        frame.decode().expect("Failed to decode EntityStatusS2c");
                    println!("Entity status: entity_id={}, status={}", packet.entity_id, packet.entity_status);
                }
                EntityAttributesS2c::ID => {
                    let packet: EntityAttributesS2c =
                        frame.decode().expect("Failed to decode EntityAttributesS2c");
                    println!("Entity attributes: entity_id={:?}, attributes={:?}", packet.entity_id, packet.properties);
                }
                UpdateSelectedSlotS2c::ID => {
                    let packet: UpdateSelectedSlotS2c =
                        frame.decode().expect("Failed to decode UpdateSelectedSlotS2c");
                    println!("Selected slot updated: slot={}", packet.slot);
                }
                PlayerListS2c::ID => {
                    let packet: PlayerListS2c =
                        frame.decode().expect("Failed to decode PlayerListS2c");
                    println!("Player list: {:?}", packet.entries);
                }
                ScreenHandlerSlotUpdateS2c::ID => {
                    println!("Received ScreenHandlerSlotUpdateS2c.");
                }
                AdvancementUpdateS2c::ID => {
                    let packet: AdvancementUpdateS2c =
                        frame.decode().expect("Failed to decode AdvancementUpdateS2c");
                    println!("Advancement update: {:?}", packet.identifiers);
                }
                CommandTreeS2c::ID => {
                    println!("Received CommandTreeS2c.");
                }
                SynchronizeTagsS2c::ID => {
                    println!("Received SynchronizeTagsS2c.");
                }
                GameMessageS2c::ID => {
                    let packet: GameMessageS2c =
                        frame.decode().expect("Failed to decode GameMessageS2c");
                    let received_message = packet.chat.to_string();
                    println!("Received message: {:?}", received_message);

                    if received_message.contains("How are you?") {
                        // Send a chat message "ahoj"
                        let message = ChatMessageC2s {
                            message: valence_protocol::Bounded("I feel good. I'm running at 240 MHz.".into()), // The message content
                            timestamp: 0,
                            salt: 0,
                            signature: None,
                            message_count: Default::default(),
                            acknowledgement: Default::default(),
                        };

                        enc.clear();
                        enc.append_packet(&message)
                            .expect("Failed to encode ChatMessageC2s");
                        let data = enc.take();

                        println!("Sending ChatMessageC2s packet: {:?}", data);

                        match socket.write_all(&data).await {
                            Ok(_) => {
                                println!("Chat message sent: 'ahoj'");
                            }
                            Err(e) => {
                                println!("Failed to send chat message. Error: {:?}", e);
                            }
                        }
                        socket.flush().await.unwrap();
                    }
                }
                EntitySetHeadYawS2c::ID => {
                    println!("EntitySetHeadYawS2c");
                }
                RotateS2c::ID => {
                    println!("RotateS2c");
                }
                _ => println!("Unhandled packet ID: 0x{:X}", frame.id),
            }
            // heap_stats();
            yield_now().await;
        }
        yield_now().await;
    }
}
