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
    FixedOffset,
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

///
/// @brief  configuration parameters
///
/// @note see pinout at link below:
/// https://www.etechnophiles.com/wp-content/uploads/2020/12/R-PI-pinout.jpg?ezimgfmt=ng:webp/ngcb40
/// 
/// @note the control board still has one additional relay for future expansion (recommend gpio 23)
///
static CLIMATE_LOG_FILE:      &'static str = "/home/jake/bonsai-bot/climate_log.csv";
static PUMP_LOG_FILE:         &'static str = "/home/jake/bonsai-bot/pump_log.txt";
const  HUMIDIFIER_PIN:        u8           = 24; 
const  PUMP_PIN:              u8           = 27;
const  FAN_PIN:               u8           = 22; 
const  FAN_PERIODIC_MINS:     i64          = 5;
const  CLIMATE_PERIODIC_MINS: i64          = 1;
const  PUMP_PERIODIC_HRS:     i64          = 24;
const  DATALOG_INTERVAL_MINS: u64          = 30;
const  FAN_DURATION_SECS:     u64          = 30;
const  PUMP_DURATION_SECS:    u64          = 60;

///
/// @brief The main routine
///
fn main() -> Result<(), Box<dyn Error>> {
    // create our GPIO'y-bois
    let gpio = Gpio::new()?;

    // initialize gpios and peripherals
    let mut temp_humidity = SHT20::new()?;
    let mut humd_gpio     = gpio.get(HUMIDIFIER_PIN)?.into_output(); 
    let mut pump_gpio     = gpio.get(PUMP_PIN)?.into_output(); 
    let mut fan_gpio      = gpio.get(FAN_PIN)?.into_output();

    // create or append to climate temp/humidity log file
    let climate_log_file = PathBuf::from(CLIMATE_LOG_FILE);
    match create_climate_log(climate_log_file) {
        Ok(_) => (),
        Err(e) => panic!("climate_log.csv cannot be created: {}", e),
    };

    // get updated timing for the next pump sequence
    let pump_log_file = PathBuf::from(PUMP_LOG_FILE);
    let pump_schedule_dt: DateTime<FixedOffset> = match get_next_pump_schedule(pump_log_file) {
        Ok(t) => t,
        Err(e) => panic!("no pump scheduled: {}", e),
    };

    // setup Instants tracking state changes
    let mut prev_state_change_climate: Instant = Instant::now();

    // setup timers
    let pump_timer = Timer::new();
    let fan_timer = Timer::new();
    let climate_timer = Timer::new();

    let _climate_guard = climate_timer.schedule_repeating(Duration::minutes(CLIMATE_PERIODIC_MINS), move || {
        climate_service(&mut prev_state_change_climate, &mut temp_humidity, &mut humd_gpio);
    });

    let _fan_guard = fan_timer.schedule_repeating(Duration::minutes(FAN_PERIODIC_MINS), move || {
        fan_service(&mut fan_gpio);
    });

    let _pump_guard = pump_timer.schedule(pump_schedule_dt, Some(Duration::hours(PUMP_PERIODIC_HRS)), move || { 
        pump_service(&mut pump_gpio); 
    });

    loop {
        // do nothing!
    }
}

///
/// @brief turns on humidifier if RH < RH_LO_THRESH and off if RH > RH_HI_THRESH 
///        and logs data at DATALOG_INTERVAL_MINS 
///    
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

    if prev_time.elapsed() > OldDuration::from_secs(DATALOG_INTERVAL_MINS * 60) {
        *prev_time = Instant::now();
        let mut f = fs::OpenOptions::new()
            .write(true)
            .append(true) 
            .open(CLIMATE_LOG_FILE)
            .expect("climate_log.csv cannot be opened");
        // log temp/humidity
        if let Err(e) = writeln!(f, "{}, {:.2}, {:.2}", Local::now(), temp, rh) {
            println!("cannot write to climate_log: {}", e.to_string());
        }
    }

    // humidifier is on and humidity is less than threshold
    if rh < RH_LO_THRESH {
        // turn on humidifier
        humd.set_high();
    }
    if rh > RH_HI_THRESH {
        // turn off humidifier
        humd.set_low();
    }
}

///
/// @brief runs the pump for a brief period of time and writes timestamp to log file 
///
fn pump_service(pump: &mut OutputPin) {
    pump.set_high();
    sleep(OldDuration::from_secs(PUMP_DURATION_SECS));
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

///
/// @brief runs the fans for a brief period of time
///
fn fan_service(fan: &mut OutputPin) {
    fan.set_high();
    sleep(OldDuration::from_secs(FAN_DURATION_SECS));
    fan.set_low();
}

///
/// @brief gets the next pump service time based on pump log file timestamps 
///
fn get_next_pump_schedule(path: PathBuf) -> Result<DateTime<FixedOffset>, Box<dyn Error>> {
    if path.is_file() && path.exists() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open(&path)?;
        let timestr_vec: Vec<String> = BufReader::new(f).lines().collect::<Result<Vec<String>, _>>()?;
        if let Some(timestr) = timestr_vec.last() {
            if let Ok(t) = DateTime::parse_from_str(&timestr, "%Y-%m-%d %H:%M:%S%.f %z") {
                // we have found the local time of the last line of the file so the next
                // watering time is 24 hours later 
                let next_sequence = t+Duration::hours(PUMP_PERIODIC_HRS);
                println!("scheduling next pump sequence at: {}", next_sequence);
                return Ok(next_sequence);
            }
        }
    }

    // return default Dec 1, 2022 12:00 MT(-7:00)
    return Ok(FixedOffset::west_opt(7*3600).unwrap().with_ymd_and_hms(2022, 12, 01, 12, 0, 0).unwrap());
}

///
/// @brief creates the climate log file with header if it doesn't exist or is empty
///
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