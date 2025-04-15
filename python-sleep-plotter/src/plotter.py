import h5py
import matplotlib.pyplot as plt

# Path to your HDF5 file and group name
HDF5_PATH = "../../sleep-tracker/sleep_recorder/sleep_data.h5"
GROUP_NAME = "2025-04-14_test"  # match what you passed to your Rust code

def plot_hdf5_data(path, group_name):
    with h5py.File(path, 'r') as f:
        group = f[group_name]
        timestamp = group['timestamp'][:]
        temperature = group['temperature'][:]
        co2 = group['co2'][:]
        humidity = group['humidity'][:]

    # Plot each signal
    fig, axs = plt.subplots(3, 1, figsize=(10, 8), sharex=True)
    axs[0].plot(timestamp, temperature, label="Temperature (°C)")
    axs[0].legend()
    axs[0].set_ylabel("Temp (°C)")

    axs[1].plot(timestamp, co2, label="CO2 (ppm)", color='orange')
    axs[1].legend()
    axs[1].set_ylabel("CO2")

    axs[2].plot(timestamp, humidity, label="Humidity (%)", color='green')
    axs[2].legend()
    axs[2].set_ylabel("Humidity")
    axs[2].set_xlabel("Time (s since start)")

    plt.tight_layout()
    plt.show()

if __name__ == "__main__":
    plot_hdf5_data(HDF5_PATH, GROUP_NAME)
