
mod sht20;
pub use crate::sht20::SHT20;
use std::thread::sleep;
use std::time::Duration;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {

    let mut temp_humidity: SHT20 = SHT20::new()?;

    loop {
        let temp = temp_humidity.get_temperature_celsius()?;
        let rh = temp_humidity.get_humidity_percent()?;
        println!("{:.2} °C, {:.2} %RH", temp, rh);
        sleep(Duration::from_secs(1));
    }
}
