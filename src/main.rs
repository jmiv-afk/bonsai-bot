mod sht20;
pub use crate::sht20::SHT20;

fn main() {
    println!("Hello, world!");
    let temp_humidity: SHT20 = SHT20::new().unwrap(); 
    // println!("{:?}", sht20.capabilities());
}
