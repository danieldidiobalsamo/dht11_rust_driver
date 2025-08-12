#![no_std]

use dht11::Dht11;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Flex;
use esp_println::println;

pub mod dht11;

#[embassy_executor::task]
pub async fn print_measurements(pin: Flex<'static>) {
    let mut dht11 = Dht11::new(pin);

    loop {
        match dht11.measure().await {
            Ok(m) => println!("{m}"),
            Err(e) => println!("Error: {e:?}"),
        }

        Timer::after(Duration::from_millis(500)).await;
    }
}
