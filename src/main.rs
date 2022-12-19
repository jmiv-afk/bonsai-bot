mod sht20;
use sht20::SHT20;
use rppal::gpio::{
    Gpio,
    OutputPin,
};
use timer::Timer;
use chrono::Duration;
use std::error::Error;
use std::time::{
    Duration as stdDuration, 
    Instant,
};
use std::thread::sleep;

//
// see pinout at link below:
// https://www.etechnophiles.com/wp-content/uploads/2020/12/R-PI-pinout.jpg?ezimgfmt=ng:webp/ngcb40
// 
const HUMIDIFIER_PIN:     u8 = 24; 
const LIGHT_PIN:          u8 = 23;
const PUMP_PIN:           u8 = 27;
const FAN_PIN:            u8 = 22; 

fn main() -> Result<(), Box<dyn Error>> {

    let gpio = Gpio::new()?;

    // initialize gpios and peripherals
    let mut temp_humidity = SHT20::new()?;
    let mut humd_gpio     = gpio.get(HUMIDIFIER_PIN)?.into_output(); 
    let mut pump_gpio     = gpio.get(PUMP_PIN)?.into_output(); 
    let mut fan_gpio      = gpio.get(FAN_PIN)?.into_output();
    let mut light_gpio    = gpio.get(LIGHT_PIN)?.into_output();

    // setup timers tracking state changes
    let mut prev_state_change_humidity: Instant = Instant::now();
    let mut prev_state_change_fan:      Instant = Instant::now();

    /*let _guard_1 = Timer::new().schedule_repeating(Duration::seconds(5), move || {
        humidity_service(&mut prev_state_change_humidity, &mut temp_humidity, &mut humd_gpio);
    });
    */

    let pump_timer = Timer::new();
    let fan_timer = Timer::new();

    let _fan_guard = fan_timer.schedule_repeating(Duration::seconds(60), move || {
        fan_service(&mut prev_state_change_fan, &mut fan_gpio);
    });

    let _pump_guard = pump_timer.schedule_repeating(Duration::seconds(10), move || {
        pump_service(&mut pump_gpio);
    });

    loop {

    }
}

fn humidity_service(prev_time: &mut Instant, temp_humidity: &mut SHT20, humd: &mut OutputPin) {

    const RH_LO_THRESH: f32 = 60.0;  // percent
    const RH_HI_THRESH: f32 = 60.0;  // percent

    let temp = temp_humidity.get_temperature_celsius().unwrap_or(0.0);
    let rh   = temp_humidity.get_humidity_percent().unwrap_or(0.0);

    // print temp/humidity
    println!("{:.2} °C, {:.2} %RH", temp, rh);

    // humidifier is on and humidity is less than threshold
    if rh < RH_LO_THRESH {
        // turn on humidifier
        println!("Turning humidifier on");
        humd.set_high();
    }
    if rh > RH_HI_THRESH {
        // turn off humidifier
        println!("Turning humidifier off");
        humd.set_low();
    }
}

fn light_service(light: &mut OutputPin) {

}

fn pump_service(pump: &mut OutputPin) {
    println!("Turning pump on");
    pump.set_high();
    sleep(stdDuration::from_secs(1));
    println!("Turning pump off");
    pump.set_low();
}

fn fan_service(prev_time: &mut Instant, fan: &mut OutputPin) {

    println!("Turning fan on");
    fan.set_high();
    sleep(stdDuration::from_secs(1));
    println!("Turning fan off");
    fan.set_low();
}