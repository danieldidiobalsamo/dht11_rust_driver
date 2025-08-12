use core::fmt::Display;

use embassy_time::{Duration, Instant, Timer};
use esp_hal::gpio::Flex;

const COOLDOWN_TIME_MS: u64 = 2000;

#[derive(Debug, PartialEq)]
pub enum DhtState {
    Idle,
    Init,
    BeginMeasurement,
    Read,
    Cooldown,
}

#[derive(Debug)]
pub struct Dht11<'a> {
    pin: Flex<'a>,
    data: [u8; 5],
    state: DhtState,
    max_cycles: u32,
    dht_timestamp: u64,
}

#[derive(Debug)]
pub struct Dht11Measurement {
    humidity: f32,
    temperature: f32,
}

impl Display for Dht11Measurement {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}% {}°", self.humidity, self.temperature)
    }
}

#[derive(Debug)]
pub enum Dht11Error {
    Data,
    ReplyHeaderMissing,
    Timeout,
    Checksum,
}

impl<'a> Dht11<'a> {
    pub fn new(pin: Flex<'a>) -> Dht11<'a> {
        Self {
            pin,
            data: [0u8; 5],
            state: DhtState::Idle,
            max_cycles: 10000,
            dht_timestamp: 0,
        }
    }

    fn read_temperature(&self) -> f32 {
        let integral_part = self.data[2] as f32;
        let decimal_part = self.data[3] as f32;

        integral_part + decimal_part
    }

    fn read_humidity(&self) -> f32 {
        let integral_part = self.data[0] as f32;
        let decimal_part = self.data[1] as f32;

        integral_part + decimal_part
    }

    pub async fn measure(&mut self) -> Result<Dht11Measurement, Dht11Error> {
        loop {
            if let Err(e) = self.step().await {
                self.state = DhtState::Idle;
                return Err(e);
            }

            if self.state == DhtState::Cooldown {
                let temperature = self.read_temperature();
                let humidity = self.read_humidity();

                return Ok(Dht11Measurement {
                    humidity,
                    temperature,
                });
            }
        }
    }

    async fn step(&mut self) -> Result<(), Dht11Error> {
        match self.state {
            DhtState::Idle => self.state = DhtState::Init,
            DhtState::Init => {
                self.pin.set_high();
                self.pin.set_output_enable(true);
                self.data = [0u8; 5];
                self.dht_timestamp = Instant::now().as_millis();

                Timer::after(Duration::from_millis(250)).await;

                self.state = DhtState::BeginMeasurement;
            }
            DhtState::BeginMeasurement => {
                // start signal
                self.pin.set_low();
                self.pin.set_output_enable(true);
                self.dht_timestamp = Instant::now().as_millis();

                Timer::after(Duration::from_millis(20)).await;
                self.state = DhtState::Read;
            }
            DhtState::Read => {
                self.dht_timestamp = Instant::now().as_millis();
                self.state = DhtState::Cooldown;
                self.read_data().await?;
            }
            DhtState::Cooldown => {
                if Instant::now().as_millis() - self.dht_timestamp > COOLDOWN_TIME_MS {
                    self.state = DhtState::Idle;
                }
            }
        };

        Ok(())
    }
    async fn read_data(&mut self) -> Result<(), Dht11Error> {
        let mut cycles = [0u32; 80];

        // end start signal
        self.pin.set_high();
        self.pin.set_output_enable(true);

        self.pin.set_output_enable(false);
        self.pin.set_input_enable(true);
        // wait to let the sensor pull data line low
        Timer::after(Duration::from_micros(15)).await;

        // expect sensor reply:
        // first 80µs low signal
        // then 80µs high signal

        if self.pulse_count(false)? == 0 {
            return Err(Dht11Error::ReplyHeaderMissing);
        }

        if self.pulse_count(true)? == 0 {
            return Err(Dht11Error::ReplyHeaderMissing);
        }

        // read 40 bits : humidity + temperature + check sum

        for i in (0..80).step_by(2) {
            cycles[i] = self.pulse_count(false)?; // start to transmit 1 LOW bit during 50µs
            cycles[i + 1] = self.pulse_count(true)?; // then the real data
        }

        // 26-28µs high pulse is a 0
        // 70µs high pulse is a 1

        for i in 0..40 {
            let low_cycles = cycles[2 * i];
            let high_cycles = cycles[2 * i + 1];

            if (low_cycles == 0) || (high_cycles == 0) {
                return Err(Dht11Error::Data);
            }

            self.data[i / 8] <<= 1;

            if high_cycles > low_cycles {
                // must be a 1.
                self.data[i / 8] |= 1;
            }
            // else it's a 0, data array is initialized with 0s, nothing to change
        }

        self.checksum()
    }

    fn checksum(&self) -> Result<(), Dht11Error> {
        if self.data[4] == self.data[0..=3].iter().sum() {
            Ok(())
        } else {
            Err(Dht11Error::Checksum)
        }
    }

    fn pulse_count(&self, level: bool) -> Result<u32, Dht11Error> {
        let mut count = 0;

        if level {
            while self.pin.is_high() {
                count += 1;
                if count >= self.max_cycles {
                    return Err(Dht11Error::Timeout);
                }
            }
        } else {
            while self.pin.is_low() {
                count += 1;
                if count >= self.max_cycles {
                    return Err(Dht11Error::Timeout);
                }
            }
        }

        Ok(count)
    }
}
