mod sht20;
use sht20::SHT20;
use rppal::gpio::Gpio;
use chrono::Duration;
use std::error::Error;
use timer::Timer;

const HUMIDIFIER_PIN: u8 = 21; 
const FAN_PIN:        u8 = 18; 

fn main() -> Result<(), Box<dyn Error>> {

    let timer = Timer::new();
    let gpio = Gpio::new()?;
    let pwm = Pwm::new()?;

    let mut temp_humidity: SHT20 = SHT20::new()?;
    let humidifier_gpio = gpio.get(HUMIDIFIER_PIN)?.into_output(); 
    let fan_pwm = gpio.get(FAN_PIN)?.into_output();

    let _g1 = timer.schedule_repeating(Duration::seconds(5), move || {
        temp_rh_control_callback(&mut temp_humidity);
    });

    loop {

    }
}

fn temp_rh_control_callback(temp_humidity: &mut SHT20) {
    const TEMP_MAX_C: f32 = 39.0; // degC
    const RH_MIN:     f32 = 60.0; // percent
    let temp = temp_humidity.get_temperature_celsius().unwrap_or(0.0);
    let rh   = temp_humidity.get_humidity_percent().unwrap_or(0.0);
    // print temp/humidity
    println!("{:.2} °C, {:.2} %RH", temp, rh);

    if temp > TEMP_MAX_C  {
        // turn on fan
    }

    if rh < RH_MIN {
        // turn on humidifier
    }

}