use rppal::i2c::I2c;
use std::{error, fmt};

const I2C_GPIO_BUS: u8              = 1;
const SHT20_ADDR: u8                = 0b1000000;  // @note: does not include R/W bit 
const RH_MEAS_NO_HOLD_MASTER: u8    = 0b11110101; 
const TEMP_MEAS_NO_HOLD_MASTER: u8  = 0b11110011;

/// Note: prefixed underscores on unused consts
const _TEMP_MEAS_HOLD_MASTER: u8     = 0b11100011;
const _RH_MEAS_HOLD_MASTER: u8       = 0b11100101; 
const _WRITE_USER_REG: u8            = 0b11100110;
const _READ_USER_REG: u8             = 0b11100111;
const _SOFT_RESET: u8                = 0b11111110;

const LSB_STATUS_MASK: u16           = 0x03;

pub type Result<T> = std::result::Result<T, ShtError>;

#[derive(Debug)]
pub enum ShtError {
    MeasInProgress,
    BytesReadMismatch,
    I2c(rppal::i2c::Error),
}

impl fmt::Display for ShtError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ShtError::MeasInProgress => 
                write!(f, "measurement in progress"),
            ShtError::BytesReadMismatch => 
                write!(f, "unexpected number of bytes read"),
            ShtError::I2c(..) => 
                write!(f, "i2c error"),
        }
    }
}

impl error::Error for ShtError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            ShtError::MeasInProgress => None,
            ShtError::BytesReadMismatch => None,
            ShtError::I2c(ref e) => Some(e),
        }
    }
}

pub enum Measurement {
    Temperature,
    Humidity,
}

pub struct SHT20 {
    i2c: I2c,
    measurement_type: Option<Measurement>,
    in_progress: bool,
}

impl SHT20 {

    pub fn new() -> Result<SHT20> {
        match I2c::with_bus(I2C_GPIO_BUS) {
            Ok(mut i2c_device) => 
                if let Err(e) = i2c_device.set_slave_address(SHT20_ADDR as u16) {
                    return Err(ShtError::I2c(e));
                } else {
                    return Ok(
                        SHT20 {
                            i2c: i2c_device,
                            measurement_type: None,
                            in_progress: false,
                        })
                },
            Err(e) => {
                return Err(ShtError::I2c(e));
            }, 
        }
    }

    pub fn get_temperature_celsius(&mut self) -> Result<f32> {
        self.trigger_temp_measurement()?;
        std::thread::sleep(std::time::Duration::from_millis(85));
        return self.read_measurement();
    }

    pub fn get_humidity_percent(&mut self) -> Result<f32> {
        self.trigger_humidity_measurement()?;
        std::thread::sleep(std::time::Duration::from_millis(85));
        return self.read_measurement();
    }

    fn trigger_temp_measurement(&mut self) -> Result<()> {
       
        if self.in_progress { 
            return Err(ShtError::MeasInProgress);
        }

        match self.i2c.write(&[TEMP_MEAS_NO_HOLD_MASTER]) {
           Ok(_) => {
               self.in_progress = true; 
               Ok(())
           }
           Err(e) => Err(ShtError::I2c(e))
        }
    }

    fn trigger_humidity_measurement(&mut self) -> Result<()> {

        if self.in_progress { 
            return Err(ShtError::MeasInProgress);
        }

        match self.i2c.write(&[RH_MEAS_NO_HOLD_MASTER]) {
           Ok(_) => {
               self.in_progress = true; 
               Ok(())
           },
           Err(e) => Err(ShtError::I2c(e))
        }
    }

    fn read_measurement(&mut self) -> Result<f32> {

        const EXPECTED_BYTES: usize = 2;
        let mut raw_bytes: [u8; EXPECTED_BYTES] = [0, 0];

        if let Ok(EXPECTED_BYTES) = self.i2c.read(&mut raw_bytes[..]) {

            let data: u16 = (raw_bytes[0] as u16) << 8 | raw_bytes[1] as u16;
            if data & LSB_STATUS_MASK == 0 {
                // it is a temperature measurement - use 14-bit representation
                self.measurement_type = Some(Measurement::Temperature);
                self.in_progress = false;
                return Ok(Self::convert_temp(data & !LSB_STATUS_MASK));
            } else {
                // it is a relative humidity measurement - use 12-bit representation
                self.measurement_type = Some(Measurement::Humidity);
                self.in_progress = false;
                return Ok(Self::convert_humidity(data & !LSB_STATUS_MASK));
            }

        } else { 
            self.in_progress = false;
            return Err(ShtError::BytesReadMismatch);
        } 
    }

    #[allow(dead_code)]
    pub fn get_measurement_type(self) -> Option<Measurement> {
        return self.measurement_type;
    }

    fn convert_humidity(raw_humidity: u16) -> f32 {
        // SHT20 datasheet sec. 6.1:
        // RH [%] = -6 + 125 * S_RH / 2^16
        return -6.0 + 125.0 * raw_humidity as f32 / 65536.0;
    }

    fn convert_temp(raw_temp: u16) -> f32 {
        // SHT20 datasheet sec. 6.2:
        // T [Celsius] = -46.85 + 175.72 * S_T / 2^16
        return -46.85 + 175.72 * raw_temp as f32 / 65536.0;
    }
} 
