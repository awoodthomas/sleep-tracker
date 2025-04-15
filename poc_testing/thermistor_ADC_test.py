#!/usr/bin/env python

import logging
import smbus2
import time
import math
from hardware_libraries.MCP342x import MCP342x

A = 0.0002264321654
B = 0.0003753456578
C = -0.0000004022657641
Vs = 5.3
Ri = 3200

logging.basicConfig(level='DEBUG')


def voltage_to_resistance(voltage):
    return Ri * (Vs / voltage - 1)

def resistance_to_temp(R):
    return 1/(A + B*math.log(R) + C*math.log(R)**3) - 273.15

def voltage_to_temp(voltage):
    return resistance_to_temp(voltage_to_resistance(voltage))

if __name__ == "__main__":
    logger = logging.getLogger(__name__)

    bus = smbus2.SMBus(1)

    # Create objects for each signal to be sampled
    adc_ch = MCP342x(bus, 0x68, channel=3, resolution=16)

    try_count = 0
    for i in range(20):
        try:
            adc_ch.configure()
            break
        except:
            try_count += 1
            time.sleep(0.1)
    print(f"Configured sensor after {try_count} retries")


    print("Setup\nADC address: {}\nConfig: {:08b}".format(hex(adc_ch.address), adc_ch.config))
    time.sleep(1)

    while True:
        # i2c_output = adc_ch.bus.read_i2c_block_data(adc_ch.address, 
        #                                          adc_ch.config, 
        #                                          3)
        # print(f"I2C out: {i2c_output}")
        # print("I2C out: {:08b}, {:08b}, {:08b}".format(i2c_output[0], i2c_output[1], i2c_output[2]))
        try_count = 0
        for i in range(20):
            try:
                reading = adc_ch.convert_and_read()
                break
            except:
                try_count += 1
                time.sleep(0.1)
        temp_C = voltage_to_temp(reading)
        print(f"Sensor reading: {reading:.3f} V, {temp_C:.2f}°C [{temp_C*9/5+32:.2f}°F] after {try_count} retries")
        time.sleep(3)