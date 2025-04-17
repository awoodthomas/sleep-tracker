use linux_embedded_hal::{Delay, I2cdev};
use ens160_aq::Ens160;


fn main() {
    let i2c = I2cdev::new("/dev/i2c-1").unwrap();
    let delay = Delay;
    let mut ens = Ens160::new_secondary_address(i2c, delay);
    ens.initialize().unwrap();  // or inspect the error here
    
    println!("ENS160 initialized!");
}
