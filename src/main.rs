mod sht20;
use sht20::SHT20;
use rppal::gpio::{Gpio, OutputPin};
use chrono::{DateTime, Duration, Utc};
use std::error::Error;
use std::time::{Duration as StdDuration};
use std::sync::Arc;
use std::future::Future;
use std::pin::Pin;
use tokio::time::{interval_at, sleep, Instant, Duration as TokioDuration};
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};
use systemd::journal;

//
// @brief  configuration parameters
//
// @note see pinout at link below:
// https://www.etechnophiles.com/wp-content/uploads/2020/12/R-PI-pinout.jpg?ezimgfmt=ng:webp/ngcb40
// 
// @note the control board still has one additional relay for future expansion (recommend gpio 23)
//
const  FAN_PIN:               u8           = 22; 
const  FAN_PERIODIC_MINS:     i64          = 3;
const  FAN_DURATION_SECS:     u64          = 30;
const  HUMIDIFIER_PIN:        u8           = 24; 
const  CLIMATE_PERIODIC_MINS: i64          = 5;
const  PUMP_PIN:              u8           = 27;
const  PUMP_PERIODIC_HRS:     i64          = 24;
const  PUMP_DURATION_SECS:    u64          = 60;

///
/// @brief The main routine, for mains
///
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    // get that journal up 
    journal::JournalLog::init().unwrap();

    // create our GPIO'y-bois
    let gpio = Gpio::new()?;

    // initialize gpios and peripherals
    let sht20             = Arc::new(Mutex::new(SHT20::new()?));
    let mut humd_gpio     = gpio.get(HUMIDIFIER_PIN)?.into_output(); 
    let mut pump_gpio     = gpio.get(PUMP_PIN)?.into_output(); 
    let mut fan_gpio      = gpio.get(FAN_PIN)?.into_output();

    // connect to database
    let (mut postgres_client, connection) = establish_connection().await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            journal::print(3, &format!("Connection error: {}", e));
            panic!("Panic! Could not connect to DB");
        }
    });

    // get updated timing for the next pump sequence
    let pump_schedule_dt: DateTime<Utc> = match get_next_pump_schedule(&mut postgres_client).await {
        Ok(t) => t,
        Err(e) => panic!("No pump scheduled: {}", e),
    };

    // Calculate the duration until the pump schedule. If the time is in the past, default to a zero duration.
    let now_utc = Utc::now();
    let duration_until_pump = if pump_schedule_dt > now_utc {
        // pump_schedule_dt is in the future with respect to now
        StdDuration::from_secs((pump_schedule_dt - now_utc).num_seconds().try_into().unwrap())
    } else {
        // pump_schedule_dt is in the past with respect to now, we should schedule this immediately
        StdDuration::from_secs(0)
    };

    // setup service tick intervals
    let now = Instant::now();
    let mut climate_interval = interval_at(now, TokioDuration::from_secs(60 * CLIMATE_PERIODIC_MINS as u64));
    let mut fan_interval = interval_at(now, TokioDuration::from_secs(60 * FAN_PERIODIC_MINS as u64));
    let mut pump_interval = interval_at(now + duration_until_pump,
                        TokioDuration::from_secs(60 * 60 * PUMP_PERIODIC_HRS as u64));

    // Convert the pump schedule to Mountain Time (UTC-7) and format for logging
    let mountain_time = pump_schedule_dt.with_timezone(&chrono::FixedOffset::west_opt(7 * 3600).unwrap());
    journal::print(6, &format!("Next pump sequence scheduled at Localtime: {}", mountain_time.format("%Y-%m-%d %H:%M:%S %Z")));

    loop {
        tokio::select! {
            _ = climate_interval.tick() => {
                match climate_service(&mut postgres_client, sht20.clone(), &mut humd_gpio).await {
                    Ok(_) => {},
                    Err(e) => {
                        journal::print(3, &format!("Climate service error: {}", e));
                        if let Some(db_error) = e.downcast_ref::<tokio_postgres::error::Error>() {
                            if db_error.is_closed() {
                                try_reconnect(&mut postgres_client).await?;
                            } else {
                                journal::print(3, &format!("Unhandled error: {}", db_error));
                            }
                        }
                        ()
                    },
                }
            }
            _ = fan_interval.tick() => {
                match fan_service(&mut fan_gpio).await {
                    Ok(_) => {},
                    Err(e) => {
                        journal::print(3, &format!("Fan service error: {}", e));
                        ()
                    }
                }
            },
            _ = pump_interval.tick() => {
                match pump_service(&mut postgres_client, &mut pump_gpio).await {
                    Ok(_) => {},
                    Err(e) => {
                        journal::print(3, &format!("Pump service error: {}", e));
                        ()
                    }
                }
            }
        }
    }
}

///
/// @brief Establishes client connection to the postgres DB
///
async fn establish_connection() -> Result<(Client, Pin<Box<dyn Future<Output = Result<(), tokio_postgres::Error>> + Send>>), Box<dyn std::error::Error>> {
    let database_url = std::env::var("BONSAIBOT_DATABASE_URL")?;
    let (client, connection) = tokio_postgres::connect(&database_url, NoTls).await?;
    Ok((client, Box::pin(connection)))
}

/// 
/// @brief Tries to reconnect to the postgres client
///
async fn try_reconnect(postgres_client: &mut tokio_postgres::Client) -> Result<(), Box<dyn std::error::Error>> {
    if postgres_client.is_closed() {
        let (new_client, new_connection) = establish_connection().await?;
        *postgres_client = new_client;

        tokio::spawn(async move {
            if let Err(e) = new_connection.await {
                journal::print(3, &format!("Reconnection error: {}", e));
            }
        });
    }
    Ok(())
}


///
/// @brief turns on humidifier if RH < RH_LO_THRESH and off if RH > RH_HI_THRESH
///        and logs temperature and humidity to the database
///    
async fn climate_service(
    client: &mut Client, 
    sht20: Arc<Mutex<SHT20>>, 
    humd: &mut OutputPin
) -> Result<(), Box<dyn Error>> {

    const RH_LO_THRESH: f64 = 70.0;  // percent
    const RH_HI_THRESH: f64 = 80.0;  // percent


    let temp = match SHT20::get_temperature_celsius(sht20.clone()).await {
        Ok(t) => t as f64,
        Err(e) => {
            journal::print(3, &format!("No temp measurement avail"));
            return Err(Box::new(e));
        },
    };

    let rh = match SHT20::get_humidity_percent(sht20).await {
        Ok(t) => t as f64,
        Err(e) => {
            journal::print(3, &format!("No humidity measurement avail"));
            return Err(Box::new(e));
        },
    };

    rh = if rh > 100.0 { 100.0 } else { rh };

    // Insert data into the database
    let stmt = match client.prepare("INSERT INTO climate_data (timestamp, temperature, humidity, is_pump_start) VALUES ($1, $2, $3, FALSE)").await {
        Ok(t) => t,
        Err(e) => {
            journal::print(3, &format!("Database prepare error {:?}", e));
            return Err(Box::new(e));
        }
    };

    let utctime = Utc::now();

    let _ = match client.execute(&stmt, &[&utctime, &(temp.clone()), &(rh.clone())]).await {
        Ok(t) => t,
        Err(e) => {
            journal::print(3, &format!("Database execute error {:?}", e));
            return Err(Box::new(e));
        }
    };

    journal::print(6, &format!("Inserted {:3.2}, {:3.2} into database", temp, rh));

    // humidifier is on and humidity is less than threshold
    if rh < RH_LO_THRESH {
        // turn on humidifier
        humd.set_high();
    }
    if rh > RH_HI_THRESH {
        // turn off humidifier
        humd.set_low();
    }
    
    Ok(())
}

///
/// @brief runs the pump for a brief period of time and writes timestamp to log file 
///
async fn pump_service(client: &mut Client, pump: &mut OutputPin) -> Result<(), Box<dyn std::error::Error>> {

    let start_time = Utc::now();
    let stmt = match client.prepare("INSERT INTO climate_data (timestamp, temperature, humidity, is_pump_start) VALUES ($1, NULL, NULL, TRUE);").await {
        Ok(t) => t,
        Err(e) => {
            journal::print(3, &format!("Database prepare error {:?}", e));
            return Err(Box::new(e));
        }
    };

    
    journal::print(6, &format!("Starting pump sequence at {}", Utc::now().with_timezone(&chrono::FixedOffset::west_opt(7*3600).expect("FixedOffset::west_opt fail")).format("%Y-%m-%d %H:%M:%S %Z")));
    
    run_pump_interval(pump, PUMP_DURATION_SECS).await?;

    journal::print(6, &format!("Ending pump sequence at {}", Utc::now().with_timezone(&chrono::FixedOffset::west_opt(7*3600).expect("FixedOffset::west_opt fail")).format("%Y-%m-%d %H:%M:%S %Z")));

    let _ = match client.execute(&stmt, &[&start_time]).await {
        Ok(t) => t,
        Err(e) => {
            journal::print(3, &format!("Database execute error {:?}", e));
            return Err(Box::new(e));
        }
    };

    Ok(())
}

///
/// @brief Runs the pump for a specified duration in seconds by asserting the GPIO
///
async fn run_pump_interval(pump: &mut OutputPin, seconds: u64) -> Result<(), Box<dyn std::error::Error>> {
    pump.set_high();
    sleep(TokioDuration::from_secs(seconds)).await;
    pump.set_low();

    Ok(())
}

///
/// @brief gets the next pump service time based on pump log file timestamps 
///
async fn get_next_pump_schedule(client: &mut Client) -> Result<DateTime<Utc>, Box<dyn std::error::Error>> {
    let stmt = "SELECT MAX(timestamp) FROM climate_data WHERE is_pump_start = TRUE;";
    let rows = client.query(stmt, &[]).await?;

    if let Some(row) = rows.get(0) {
        let last_pump_time: DateTime<Utc> = row.get(0);
        Ok(last_pump_time + Duration::hours(PUMP_PERIODIC_HRS))
    } else {
        // If no entry found, default to current time + pump interval
        Ok(Utc::now() + Duration::hours(PUMP_PERIODIC_HRS))
    }
}

///
/// @brief runs the fans for a brief period of time
///
async fn fan_service(fan: &mut OutputPin) -> Result<(), Box<dyn std::error::Error>> {
    fan.set_high();
    sleep(TokioDuration::from_secs(FAN_DURATION_SECS)).await;
    fan.set_low();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    pub async fn test_pump() {
        let gpio = Gpio::new().expect("Cannot get access to GPIO");
        let mut pump_gpio = gpio.get(PUMP_PIN).expect("GPIO cannot be taken").into_output(); 
        run_pump_interval(&mut pump_gpio, 10).await.expect("Pump did not run"); 
    }
}
