import traceback
from flask import Flask, render_template, jsonify, send_file, request
import h5py
import os
import matplotlib.pyplot as plt
import numpy as np

app = Flask(__name__)
HDF5_PATH = os.environ["SLEEP_DATA_DIR"] + "/sleep_data.h5"
GROUP_NAME = "2025-04-14_test"

@app.route("/")
def index():
    return render_template("index.html")

@app.route("/groups")
def list_groups():
    groups = []
    with h5py.File(HDF5_PATH, "r") as f:
        def visitor(name, obj):
            if isinstance(obj, h5py.Group) and name.count("/") == 0:
                groups.append(name)
        f.visititems(visitor)
    return jsonify(groups)

def clean_nan_array(arr):
    return [None if np.isnan(x) else float(x) for x in arr]

@app.route("/data")
def get_data():
    data = {}
    group_name = request.args.get("group")

    if not group_name:
        return jsonify({"error": "Missing group parameter"}), 404

    try:
        with h5py.File(HDF5_PATH, "r") as f:
            group = f[group_name]
            # timestamps = g["timestamp"][:].tolist()
            # temperature = g["temperature"][:].tolist()
            # pressures = g["pressure"][:].tolist()
            # humidity = g["humidity"][:].tolist()
            # co2 = g["co2eq_ppm"][:].tolist()
            # tvoc = g["tvoc_ppb"][:].tolist()
            # aqi = g["air_quality_index"][:].tolist()
            # thermistor_temp = g["thermistor_temp"][:].tolist()
            # image_paths = g.get("image_path", [])[:].astype(str).tolist()
            for key in ["timestamp", "temperature", "humidity", "co2eq_ppm", "tvoc_ppb", "air_quality_index", "thermistor_temp", "image_path"]:
                if key in group:
                    dataset = group[key]
                    if dataset.dtype.kind in {'u', 'i'}:
                        data[key] = dataset[:].astype(float).tolist()
                    elif dataset.dtype.kind == 'f':
                        data[key] = clean_nan_array(dataset[:])
                    else:
                        data[key] = dataset[:].astype(str).tolist()
            
            if "audio" in group:
                audio_ds = group["audio"]
                audio_data = []
                for i in range(len(audio_ds)):
                    entry = audio_ds[i]
                    audio_data.append({
                        "start_time": int(entry["start_time"]),
                        "duration": int(entry["duration"]),
                        "path": str(entry["path"].decode() if isinstance(entry["path"], bytes) else entry["path"])
                    })
                data["audio"] = audio_data
            else:
                data["audio"] = []
    except Exception as e:
        traceback.print_exc()
        return jsonify({"error": str(e)}), 500
    
    if "temperature" in data:
        data["temperature"] = [None if x is None else (x * 9 / 5 + 32) for x in data["temperature"]]
    if "thermistor_temp" in data:
        data["thermistor_temp"] = [None if x is None else (x * 9 / 5 + 32) for x in data["thermistor_temp"]]

    return jsonify(data)       

    # print(g, "min: ", min(tvoc), "max: ", max(tvoc));

    # return jsonify({
    #     "timestamp": timestamps,
    #     "temperature": temperature,
    #     "pressure": pressures,
    #     "humidity": humidity,
    #     "co2eq_ppm": co2,
    #     "tvoc_ppb": tvoc,
    #     "air_quality_index": aqi,
    #     "thermistor_temp": thermistor_temp,
    #     "image_paths": image_paths
    # })

@app.route("/preview")
def preview_image():
    image_path = request.args.get("path")
    if not image_path or not os.path.exists(image_path):
        return "", 404

    return send_file(image_path, mimetype="image/jpeg")

if __name__ == "__main__":
    os.makedirs("static", exist_ok=True)
    app.run(host="0.0.0.0", port=8000)
