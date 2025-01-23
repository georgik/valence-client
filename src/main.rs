#![no_std]
#![no_main]
extern crate alloc;
use defmt_rtt as _;
use defmt::info;
use esp_hal::psram;

use esp_hal::prelude::*;

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, rng::Rng};
use esp_println::{print, println};
use esp_wifi::{
    init,
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice, WifiState},
    EspWifiController,
};


macro_rules! mk_static {
    ($t:ty, $val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}

// const SSID: &str = env!("SSID");
// const PASSWORD: &str = env!("PASSWORD");


fn init_psram_heap(start: *mut u8, size: usize) {
    println!("Starting PSRAM heap at 0x{:08X} with size {}", start as usize, size);
    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            start,
            size,
            esp_alloc::MemoryCapability::External.into(),
        ));
    }
}

#[cfg(is_not_release)]
compile_error!("PSRAM example must be built in release mode!");

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    print!("System starting up...");
    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });
    println!(" ok");
    // let wifi_stack = mk_static!(StackResources<3>, StackResources::<3>::new());

    const MEMORY_SIZE: usize = 72 * 1024;
    print!("Initializing allocator with {} bytes...", MEMORY_SIZE);
    esp_alloc::heap_allocator!(MEMORY_SIZE);
    println!(" ok");


    esp_println::logger::init_logger_from_env();
    // esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);
    println!(" ok");

    let timer0 = esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER)
        .split::<esp_hal::timer::systimer::Target>();
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");
    let mut rng = Rng::new(peripherals.RNG);
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let timer1 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1);
    // let init = esp_wifi::init(
    //     timer1.timer0,
    //     rng,
    //     peripherals.RADIO_CLK,
    // )
    //     .unwrap();

    Timer::after(Duration::from_millis(1500)).await;

    // let timg0 = TimerGroup::new(peripherals.TIMG0);

    //
    let init = &*mk_static!(
        EspWifiController<'static>,
        init(timer1.timer0, rng.clone(), peripherals.RADIO_CLK).unwrap()
    );

    let wifi = peripherals.WIFI;

    let (wifi_interface, controller) = match esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice) {
        Ok(result) => result,
        Err(e) => {
            println!("Failed to initialize WiFi with mode: {:?}", e);
            return;
        }
    };


    println!("PSRAMConfig");
    let psram_config  = psram::PsramConfig::default();

    println!("init_psram");
    let (start, size) = psram::init_psram(peripherals.PSRAM, psram::PsramConfig::default()); // It hangs here

    println!("init_psram_heap");
    init_psram_heap(start, size);
    println!("Delay for 1500 ms");


    Timer::after(Duration::from_millis(1500)).await;
    println!("Starting network stack...");

}




#[embassy_executor::task]
async fn tick_task() {
    loop {
        println!("Tick...");
        Timer::after(Duration::from_secs(1)).await;
    }
}
