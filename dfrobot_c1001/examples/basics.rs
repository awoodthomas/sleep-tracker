use std::thread::sleep;
use std::time::Duration;

use dfrobot_c1001::{HumanPresence, Mode, C1001};

/// Simple demonstration that prints presence/movement every second.
/// 
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Opening serial device");
    let mut radar = C1001::open("/dev/serial0", 115_200, Duration::from_millis(1000))?;
    println!("Requesting sensor begin");
    radar.begin()?;
    println!("Setting work mode");
    radar.config_work_mode(Mode::Sleep)?;
    println!("Start sensor loop");
    loop {
        let presence = radar.sleep_human_data(HumanPresence::Presence)?;
        let movement = radar.sleep_human_data(HumanPresence::Movement)?;
        println!("presence={}, movement={}", presence, movement);
        println!("HR={}, resp={} bpm (state={})", radar.heart_rate()?, radar.breathe_value()?, radar.breathe_state()?);
        sleep(Duration::from_secs(1));
    }
}