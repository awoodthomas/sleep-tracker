//! MCP342x ADC driver for Linux using linux_embedded_hal and embedded-hal.

use embedded_hal::i2c::I2c;
use std::time::Duration;
use thiserror::Error;

/// Errors for the MCP342x driver.
#[derive(Error, Debug)]
pub enum Error<E: std::error::Error + 'static> {
    #[error("I2C bus error: {0}")]
    I2c(#[from] E),
    #[error("Configuration read back from device does not match driver config: used {used}, stored {stored}")]
    ConfigMismatch { used: u8, stored: u8 },

}

/// PGA gain settings.
#[derive(Clone, Copy, Debug)]
pub enum Gain {
    G1 = 0b00,
    G2 = 0b01,
    G4 = 0b10,
    G8 = 0b11,
}

/// Conversion resolution and SPS timing.
#[derive(Clone, Copy, Debug)]
pub enum Resolution {
    Bits12 = 0b0000, // 240 SPS
    Bits14 = 0b0100, // 60 SPS
    Bits16 = 0b1000, // 15 SPS
    Bits18 = 0b1100, // 3.75 SPS (MCP3422/3/4 only)
}

/// Input channel selection.
#[derive(Clone, Copy, Debug)]
pub enum Channel {
    Ch1 = 0b0000000,
    Ch2 = 0b0100000,
    Ch3 = 0b1000000,
    Ch4 = 0b1100000,
}

/// MCP342x driver struct.
pub struct MCP342x<I2C> {
    i2c: I2C,
    address: u8,
    config: u8,
    scale_factor: f32,
    offset: f32,
}

impl<I2C, E> MCP342x<I2C>
where
    I2C: I2c<Error = E>,
    I2C::Error: std::error::Error + 'static,
{
    const GAIN_MASK: u8 = 0b00000011;
    const RES_MASK: u8 = 0b00001100;
    const CONT_MASK: u8 = 0b00010000;
    const CH_MASK: u8 = 0b01100000;
    const NOT_READY: u8 = 0b10000000;

    /// Create a new ADC instance. Default config = 0.
    pub fn new(i2c: I2C, address: u8) -> Self {
        MCP342x { i2c, address, config: 0, scale_factor: 1.0, offset: 0.0 }
    }

    /// Select input channel.
    pub fn set_channel(&mut self, channel: Channel) {
        self.config = (self.config & !Self::CH_MASK) | (channel as u8);
    }

    /// Set PGA gain.
    pub fn set_gain(&mut self, gain: Gain) {
        self.config = (self.config & !Self::GAIN_MASK) | (gain as u8);
    }

    /// Set conversion resolution.
    pub fn set_resolution(&mut self, res: Resolution) {
        self.config = (self.config & !Self::RES_MASK) | (res as u8);
    }

    /// Enable or disable continuous conversion.
    pub fn set_continuous_mode(&mut self, continuous: bool) {
        if continuous {
            self.config |= Self::CONT_MASK;
        } else {
            self.config &= !Self::CONT_MASK;
        }
    }

    /// Apply a scale factor for the voltage conversion.
    pub fn set_scale_factor(&mut self, factor: f32) {
        self.scale_factor = factor;
    }

    /// Apply an offset for the voltage conversion.
    pub fn set_offset(&mut self, offset: f32) {
        self.offset = offset;
    }

    /// Write current config to device.
    pub fn configure(&mut self) -> Result<(), Error<E>> {
        self.i2c.write(self.address, &[self.config]).map_err(Error::I2c)
    }

    /// Initiate one-shot conversion (ignores continuous mode bit).
    pub fn convert(&mut self) -> Result<(), Error<E>> {
        let mut c = self.config & !Self::CONT_MASK;
        c |= Self::NOT_READY;
        self.i2c.write(self.address, &[c]).map_err(Error::I2c)
    }

    /// Low-level raw read: returns (count, config_used).
    pub fn raw_read(&mut self) -> Result<(i32, u8), Error<E>> {
        // Decode resolution bits to sample width
        let res_bits = match self.config & Self::RES_MASK {
            0b0000 => 12,
            0b0100 => 14,
            0b1000 => 16,
            0b110000 => 18,
            _ => unreachable!(),
        }; // note: mask covers only two bits, last arm covers unused bits
        let bytes = if res_bits == 18 { 4 } else { 3 };
        let mut buf = [0u8; 4];

        loop {
            // Write config then read bytes
            self.i2c
                .write_read(self.address, &[self.config], &mut buf[..bytes])
                .map_err(Error::I2c)?;

            let config_used = buf[bytes - 1];
            if config_used & Self::NOT_READY == 0 {
                // Assemble raw count
                let mut count = 0i32;
                for &b in &buf[..bytes - 1] {
                    count = (count << 8) | (b as i32);
                }
                // Sign extend
                let sign_mask = 1 << (res_bits - 1);
                let mag_mask = (sign_mask - 1) as i32;
                if (count & sign_mask) != 0 {
                    count = -((!count & mag_mask) + 1);
                }
                return Ok((count, config_used));
            }
        }
    }

    /// Read voltage (or raw count if `raw`=true).
    pub fn read(&mut self, raw: bool) -> Result<f32, Error<E>> {
        let (count, config_used) = self.raw_read()?;
        if config_used != self.config {
            return Err(Error::ConfigMismatch { used: config_used, stored: self.config });
        }
        if raw {
            return Ok(count as f32);
        }
        // Determine LSB
        let lsb = match config_used & Self::RES_MASK {
            0b0000 => 1e-3,
            0b0100 => 250e-6,
            0b1000 => 62.5e-6,
            0b1100 => 15.625e-6,
            _ => unreachable!(),
        };
        // Determine gain
        let gain = match config_used & Self::GAIN_MASK {
            0b00 => 1.0,
            0b01 => 2.0,
            0b10 => 4.0,
            0b11 => 8.0,
            _ => unreachable!(),
        };
        let voltage = (count as f32) * lsb * self.scale_factor / gain + self.offset;
        Ok(voltage)
    }

    /// Expected conversion time in seconds for current resolution.
    pub fn conversion_time(&self) -> f32 {
        match self.config & Self::RES_MASK {
            0b0000 => 1.0 / 240.0,
            0b0100 => 1.0 / 60.0,
            0b1000 => 1.0 / 15.0,
            0b1100 => 1.0 / 3.75,
            _ => unreachable!(),
        }
    }

    /// Do a convert + read cycle, sleeping until conversion completes if `sleep`=true.
    pub fn convert_and_read(&mut self, sleep: bool, raw: bool) -> Result<f32, Error<E>> {
        self.convert()?;
        if sleep {
            let delay = self.conversion_time() * 1.2;
            std::thread::sleep(Duration::from_secs_f32(delay));
        }
        self.read(raw)
    }
}

/// General call reset (0x06).
pub fn general_call_reset<I2C, E>(i2c: &mut I2C) -> Result<(), E>
where
    I2C: I2c<Error = E>,
{
    i2c.write(0, &[0x06])
}

/// General call latch (0x04).
pub fn general_call_latch<I2C, E>(i2c: &mut I2C) -> Result<(), E>
where
    I2C: I2c<Error = E>,
{
    i2c.write(0, &[0x04])
}

/// General call convert (0x08).
pub fn general_call_convert<I2C, E>(i2c: &mut I2C) -> Result<(), E>
where
    I2C: I2c<Error = E>,
{
    i2c.write(0, &[0x08])
}
