mod sht20;
use sht20::SHT20;
use rppal::gpio::{
    Gpio,
    OutputPin,
};
use timer::Timer;
use chrono::{
    Duration,
    DateTime,
    Local,
    TimeZone,
    NaiveDate,
    NaiveDateTime,
};
use std::fs;
use std::path::PathBuf;
use std::io::{
    BufReader,
    BufRead,
    Write,
}; 
use std::error::Error;
use std::thread::sleep;
use std::time::{
    Instant,
    Duration as OldDuration,
};

// see pinout at link below:
// https://www.etechnophiles.com/wp-content/uploads/2020/12/R-PI-pinout.jpg?ezimgfmt=ng:webp/ngcb40
//const LIGHT_PIN:          u8 = 23; // maybe some other time, not messing w/ mains rn
const HUMIDIFIER_PIN:     u8 = 24; 
const PUMP_PIN:           u8 = 27;
const FAN_PIN:            u8 = 22; 

fn main() -> Result<(), Box<dyn Error>> {
    // create our GPIO'y-bois
    let gpio = Gpio::new()?;

    // initialize gpios and peripherals
    let mut temp_humidity = SHT20::new()?;
    let mut humd_gpio     = gpio.get(HUMIDIFIER_PIN)?.into_output(); 
    let mut pump_gpio     = gpio.get(PUMP_PIN)?.into_output(); 
    let mut fan_gpio      = gpio.get(FAN_PIN)?.into_output();

    // create or append to climate temp/humidity log file
    let climate_log_file = PathBuf::from("/home/jake/bonsai-bot/climate_log.csv");
    match create_climate_log(climate_log_file) {
        Ok(_) => (),
        Err(_) => panic!("climate_log.csv cannot be created"),
    };


    let pump_log_file = PathBuf::from("/home/jake/bonsai-bot/pump_log.txt");
    let pump_schedule_dt: DateTime<Local> = get_next_pump_schedule(pump_log_file)?;

    // setup Instants tracking state changes
    let mut prev_state_change_climate: Instant = Instant::now();

    // setup timers
    let pump_timer = Timer::new();
    let fan_timer = Timer::new();
    let climate_timer = Timer::new();

    let _climate_guard = climate_timer.schedule_repeating(Duration::minutes(10), move || {
        climate_service(&mut prev_state_change_climate, &mut temp_humidity, &mut humd_gpio);
    });

    let _fan_guard = fan_timer.schedule_repeating(Duration::minutes(5), move || {
        fan_service(&mut fan_gpio);
    });

    let _pump_guard = pump_timer.schedule(pump_schedule_dt, Some(Duration::hours(24)), move || { 
        pump_service(&mut pump_gpio); 
    });

    loop {
        // do nothing!
    }
}

fn climate_service(prev_time: &mut Instant, temp_humidity: &mut SHT20, humd: &mut OutputPin) {

    const RH_LO_THRESH: f32 = 70.0;  // percent
    const RH_HI_THRESH: f32 = 80.0;  // percent

    let temp = match temp_humidity.get_temperature_celsius() {
        Ok(t) => t,
        Err(_) => return,
    };

    let rh = match temp_humidity.get_humidity_percent() {
        Ok(t) => t,
        Err(_) => return,
    };

    let mut f = fs::OpenOptions::new()
        .write(true)
        .append(true) 
        .open("/home/jake/bonsai-bot/climate_log.csv")
        .expect("climate_log.csv cannot be opened");

    // print temp/humidity
    if let Err(e) = writeln!(f, "{}, {:.2}, {:.2}", Local::now(), temp, rh) {
        println!("cannot write to climate_log: {}", e.to_string());
    }

    // humidifier is on and humidity is less than threshold
    if rh < RH_LO_THRESH {
        // turn on humidifier
        humd.set_high();
        *prev_time = Instant::now();
    }
    if rh > RH_HI_THRESH {
        // turn off humidifier
        humd.set_low();
        *prev_time = Instant::now();
    }
}

fn pump_service(pump: &mut OutputPin) {
    pump.set_high();
    sleep(OldDuration::from_secs(60));
    pump.set_low();
    // TODO: append timestamp to file 
    let mut f = fs::OpenOptions::new()
        .write(true)
        .append(true) 
        .open("/home/jake/bonsai-bot/pump_log.txt")
        .expect("pump_log.txt cannot be opened");
    if let Err(e) = writeln!(f, "{}", Local::now()) {
        println!("cannot write to pump log: {}", e.to_string());
    }
}

fn fan_service(fan: &mut OutputPin) {
    fan.set_high();
    sleep(OldDuration::from_secs(30));
    fan.set_low();
}

fn get_next_pump_schedule(path: PathBuf) -> Result<DateTime<Local>, Box<dyn Error>> {
    if path.is_file() && path.exists() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open(&path)?;
        let timestr = BufReader::new(f).lines().last().ok_or("");
        let dt: NaiveDateTime = timestr??.parse()?;
        println!("here");
        if let Some(t) = Local.from_local_datetime(&dt).earliest() {
            // we have found the local time of the last line of the file so the next
            // watering time is 24 hours later 
            println!("Scheduling next pump sequence at: {}", t+Duration::hours(24));
            return Ok(t + Duration::hours(24));
        }
    }

    // return default
    return Ok(Local.from_local_datetime(&NaiveDate::from_ymd_opt(2022, 12, 21).expect("ymd")
        .and_hms_opt(23, 13, 0).expect("hms")).unwrap());
}

fn create_climate_log(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let mut line = String::new();
    if path.is_file() {
        // read file, ensure header is written
        let f = fs::OpenOptions::new()
            .read(true)
            .open(&path)?;
        let mut rdr = BufReader::new(f);
        rdr.read_line(&mut line)?;
    } 
    if line.is_empty() {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&path)?;
        writeln!(f, "Timestamp(Local),Temperature(degC),Humidity(%)")?;
    }
    Ok(())
}