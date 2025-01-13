#![no_std]
#![no_main]

use core::net::Ipv4Addr;

use embassy_executor::Spawner;
use embassy_net::{tcp::TcpSocket, Runner, StackResources};
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup};
use esp_println::println;
use esp_wifi::{
    init,
    wifi::{
        ClientConfiguration,
        Configuration,
        WifiController,
        WifiDevice,
        WifiEvent,
        WifiStaDevice,
        WifiState,
    },
    EspWifiController,
};
use valence_protocol::{Decode, Encode, PacketDecoder, PacketEncoder, VarInt};
// use panic_probe as _;

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
            // use esp_hal::timer::systimer::SystemTimer;
            // let systimer = SystemTimer::new(peripherals.SYSTIMER);
            esp_hal_embassy::init(timer0.alarm0);
        }
    }

    let config = embassy_net::Config::dhcpv4(Default::default());

    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    spawner.spawn(connection(controller)).ok();
    spawner.spawn(net_task(runner)).ok();

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

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
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
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
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static, WifiStaDevice>>) {
    runner.run().await
}
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
