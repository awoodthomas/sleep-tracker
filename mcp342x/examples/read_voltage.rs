use linux_embedded_hal::I2cdev;
use mcp342x::{MCP342x, Channel, Gain, Resolution};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let i2c = I2cdev::new("/dev/i2c-1")?;
    let mut adc = MCP342x::new(i2c, 0x68);
    adc.set_channel(Channel::Ch3);
    adc.set_gain(Gain::G1);
    adc.set_resolution(Resolution::Bits16);
    adc.convert().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let volts = adc.read(false).unwrap();
    println!("Voltage: {:.3} V", volts);
    Ok(())
}
