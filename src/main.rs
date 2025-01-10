#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_net::{
    tcp::TcpSocket,
    IpListenEndpoint,
    Ipv4Address,
    Ipv4Cidr,
    Stack,
    StackResources,
    StaticConfigV4,
};
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{prelude::*, rng::Rng, timer::timg::TimerGroup};
use esp_println::{print, println};
use esp_wifi::{
    init,
    wifi::{
        AccessPointConfiguration,
        Configuration,
        WifiApDevice,
        WifiController,
        WifiDevice,
        WifiEvent,
        WifiState,
    },
    EspWifiController,
};// use valence_protocol::packets::handshaking::HandshakeC2s;
// use valence_protocol::packets::login::{LoginHelloC2s, LoginSuccessS2c, LoginCompressionS2c};
// use valence_protocol::packets::play::{GameJoinS2c, PlayerPositionLookS2c, KeepAliveS2c, DisconnectS2c};
use valence_protocol::{Decode, Encode, PacketDecoder, PacketEncoder, VarInt};
// use panic_probe as _;
use esp_wifi::wifi::WifiStaDevice;

// esp_alloc::esp_heap!(96 * 1024);


use {defmt_rtt as _, esp_backtrace as _};

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}


const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // Initialize peripherals and system timer for Embassy
    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });



    println!("System initialized!");

    esp_alloc::heap_allocator!(72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);

    let init = &*mk_static!(
        EspWifiController<'static>,
        init(
            timg0.timer0,
            Rng::new(peripherals.RNG),
            peripherals.RADIO_CLK,
        )
        .unwrap()
    );

    let wifi = peripherals.WIFI;
    let (wifi_interface, controller) =
        esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice).unwrap();

    cfg_if::cfg_if! {
        if #[cfg(feature = "esp32")] {
            let timg1 = TimerGroup::new(peripherals.TIMG1);
            esp_hal_embassy::init(timg1.timer0);
        } else {
            use esp_hal::timer::systimer::{SystemTimer, Target};
            let systimer = SystemTimer::new(peripherals.SYSTIMER).split::<Target>();
            esp_hal_embassy::init(systimer.alarm0);
        }
    }

    let config = embassy_net::Config::dhcpv4(Default::default());

    let seed = 1234; // very random, very secure seed

    // Init network stack
    // let stack = &*mk_static!(
    //     Stack<WifiDevice<'_, WifiApDevice>>,
    //     Stack::new(
    //         wifi_interface,
    //         config,
    //         mk_static!(StackResources<3>, StackResources::<3>::new()),
    //         seed
    //     )
    // );

    // spawner.spawn(connection(controller)).ok();
    // spawner.spawn(net_task(&stack)).ok();

    // let mut rx_buffer = [0; 4096];
    // let mut tx_buffer = [0; 4096];
    //
    // loop {
    //     if stack.is_link_up() {
    //         break;
    //     }
    //     Timer::after(Duration::from_millis(500)).await;
    // }
    //
    // println!("Waiting to get IP address...");
    // loop {
    //     if let Some(config) = stack.config_v4() {
    //         println!("Got IP: {}", config.address);
    //         break;
    //     }
    //     Timer::after(Duration::from_millis(500)).await;
    // }
    println!("Wi-Fi initialized!");
    // let wifi = esp_wifi::init(
    //     timer_group.timer0,
    //     esp_hal::rng::Rng::new(peripherals.RNG),
    //     peripherals.RADIO_CLK,
    // )
    //     .expect("Failed to initialize Wi-Fi");
    //
    // // Configure and initialize Embassy networking
    // let stack = configure_network_stack(wifi).await;
    //
    // // Spawn the main client task
    // if let Err(e) = spawner.spawn(run_client()) {
    //     // error!("Failed to spawn client task: {:?}", e);
    //     println!("error");
    // }
    let mut dec = PacketDecoder::new();
}

//
// #[embassy_executor::task]
// async fn connection(mut controller: WifiController<'static>) {
//     println!("start connection task");
//     // println!("Device capabilities: {:?}", controller.capabilities());
//     loop {
//         match esp_wifi::wifi::wifi_state() {
//             WifiState::StaConnected => {
//                 // wait until we're no longer connected
//                 controller.wait_for_event(WifiEvent::StaDisconnected).await;
//                 Timer::after(Duration::from_millis(5000)).await
//             }
//             _ => {}
//         }
//         if !matches!(controller.is_started(), Ok(true)) {
//             let client_config = Configuration::Client(ClientConfiguration {
//                 ssid: SSID.try_into().unwrap(),
//                 password: PASSWORD.try_into().unwrap(),
//                 ..Default::default()
//             });
//             controller.set_configuration(&client_config).unwrap();
//             println!("Starting wifi");
//             controller.start_async().await.unwrap();
//             println!("Wifi started!");
//         }
//         println!("About to connect...");
//
//         match controller.connect_async().await {
//             Ok(_) => println!("Wifi connected!"),
//             Err(e) => {
//                 println!("Failed to connect to wifi:");
//                 Timer::after(Duration::from_millis(5000)).await
//             }
//         }
//     }
// }
//
//
//
// #[embassy_executor::task]
// async fn net_task(stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>) {
//     stack.run().await
// }

// Configures and initializes the network stack
// async fn configure_network_stack(wifi: Wifi<'static>) -> Stack<StackResources<1>> {
//     let stack_resources = StackResources::<1>::new();
//     let network_driver = wifi.into_driver();
//     let stack = Stack::new(network_driver, stack_resources);
//
//     // Optionally configure DHCP or static IP here
//
//     info!("Network stack configured!");
//     stack
// }


async fn run_client() {
//     let server_address = "127.0.0.1:25565";
//
//     let mut socket = TcpSocket::new(&stack);
//     socket.set_nodelay(true);
//
//     if let Err(e) = socket.connect(server_address).await {
//         error!("Failed to connect to server: {:?}", e);
//         return;
//     }
//     info!("Connected to server at {}", server_address);
//
    let mut dec = PacketDecoder::new();
//     let mut enc = PacketEncoder::new();
//
//     // Step 1: Perform handshake
//     if let Err(e) = send_handshake(&mut socket, &mut enc).await {
//         error!("Handshake failed: {:?}", e);
//         return;
//     }
//
//     // Step 2: Log in and handle updates
//     if let Err(e) = login_and_handle_updates(&mut socket, &mut dec, &mut enc).await {
//         error!("Login failed: {:?}", e);
//     }
}

// Sends the handshake packet
// async fn send_handshake(socket: &mut TcpSocket<'_>, enc: &mut PacketEncoder) -> Result<(), ()> {
//     let handshake_packet = HandshakeC2s {
//         protocol_version: VarInt(763), // Minecraft 1.20 protocol version
//         server_address: String::from("127.0.0.1"),
//         server_port: 25565,
//         next_state: valence::protocol::packets::handshaking::handshake_c2s::HandshakeNextState::Login,
//     };
//
//     enc.append_packet(&handshake_packet)
//         .expect("Failed to encode handshake packet");
//     socket.write_all(&enc.take()).await.map_err(|_| ())?;
//     info!("Handshake sent!");
//     Ok(())
// }
//


// async fn login_and_handle_updates(
//     socket: &mut TcpSocket<'_>,
//     dec: &mut PacketDecoder,
//     enc: &mut PacketEncoder,
// ) -> Result<(), ()> {
//     // Send login start packet
//     let login_start_packet = LoginHelloC2s {
//         username: String::from("Player"),
//         profile_id: None,
//     };
//
//     enc.append_packet(&login_start_packet)
//         .expect("Failed to encode LoginHelloC2s packet");
//     socket.write_all(&enc.take()).await.map_err(|_| ())?;
//     info!("Login request sent.");
//
//     let mut buf = [0u8; 4096];
//     loop {
//         let bytes_read = socket.read(&mut buf).await.map_err(|_| ())?;
//         if bytes_read == 0 {
//             info!("Connection closed by server.");
//             return Ok(());
//         }
//
//         dec.queue_bytes(buf[..bytes_read].into());
//
//         while let Ok(Some(frame)) = dec.try_next_packet() {
//             match frame.id {
//                 LoginCompressionS2c::ID => {
//                     let packet: LoginCompressionS2c = frame.decode().expect("Failed to decode");
//                     info!(
//                         "Received LoginCompressionS2c packet with threshold: {}",
//                         packet.threshold.0
//                     );
//                     dec.set_compression(valence::protocol::CompressionThreshold(packet.threshold.0));
//                 }
//                 LoginSuccessS2c::ID => {
//                     let packet: LoginSuccessS2c = frame.decode().expect("Failed to decode");
//                     info!(
//                         "Login successful! Username: {}, UUID: {}",
//                         packet.username,
//                         packet.uuid
//                     );
//                 }
//                 GameJoinS2c::ID => {
//                     let packet: GameJoinS2c = frame.decode().expect("Failed to decode");
//                     info!("Joined the game world with Entity ID: {}", packet.entity_id);
//                 }
//                 PlayerPositionLookS2c::ID => {
//                     let packet: PlayerPositionLookS2c = frame.decode().expect("Failed to decode");
//                     info!(
//                         "Player position updated: x={}, y={}, z={}, yaw={}, pitch={}",
//                         packet.position.x,
//                         packet.position.y,
//                         packet.position.z,
//                         packet.yaw,
//                         packet.pitch
//                     );
//                 }
//                 KeepAliveS2c::ID => {
//                     let packet: KeepAliveS2c = frame.decode().expect("Failed to decode");
//                     info!("KeepAlive received with ID: {}", packet.id);
//                 }
//                 DisconnectS2c::ID => {
//                     let packet: DisconnectS2c = frame.decode().expect("Failed to decode");
//                     info!("Disconnected by server: {}", packet.reason);
//                     return Ok(());
//                 }
//                 _ => warn!("Unhandled packet ID: {}", frame.id),
//             }
//         }
//     }
// }
