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

    keys = ["timestamp", 
    "temperature", 
    "humidity", 
    "co2eq_ppm", 
    "tvoc_ppb", 
    "air_quality_index", 
    "thermistor_temp", 
    "image_path", 
    "image_motion",
    "mmwave_presence",
    "mmwave_movement",
    "mmwave_heart_rate_bpm",
    "mmwave_resp_rate_bpm"]

    try:
        with h5py.File(HDF5_PATH, "r") as f:
            group = f[group_name]
            for key in keys:
                if key in group:
                    dataset = group[key]
                    if dataset.dtype.kind in {'u', 'i'}:
                        data[key] = dataset[:].astype(float).tolist()
                    elif dataset.dtype.kind == 'f':
                        data[key] = clean_nan_array(dataset[:])
                    elif dataset.dtype.kind == 'b':
                        data[key] = dataset[:].astype(bool).tolist()
                    else:
                        data[key] = dataset[:].astype(str).tolist()
            
            if "audio" in group:
                audio_ds = group["audio"]
                audio_data = []
                for i in range(len(audio_ds)):
                    entry = audio_ds[i]
                    fields = {
                        "start_time_s": int(entry["start_time_s"]),
                        "duration_s": int(entry["duration_s"]),
                        "path": str(entry["path"].decode() if isinstance(entry["path"], bytes) else entry["path"]),
                    }
                    if "audio_rms_db" in audio_ds.dtype.names:
                        fields["audio_rms_db"] = entry["audio_rms_db"].tolist()
                        fields["audio_rms_t_s"] = entry["audio_rms_t_s"].tolist()
                        fields["audio_rms_rel_t_s"] = (entry["audio_rms_t_s"] - entry["start_time_s"]).tolist()
                    audio_data.append(fields)
                data["audio"] = audio_data
            else:
                data["audio"] = []
    except Exception as e:
        traceback.print_exc()
        return jsonify({"error": str(e)}), 500
    
    # Convert to Fahrenheit if the data is present
    if "temperature" in data:
        data["temperature"] = [None if x is None else (x * 9 / 5 + 32) for x in data["temperature"]]
    if "thermistor_temp" in data:
        data["thermistor_temp"] = [None if x is None else (x * 9 / 5 + 32) for x in data["thermistor_temp"]]

    return jsonify(data)       


@app.route("/preview")
def preview_image():
    image_path = request.args.get("path")
    if not image_path or not os.path.exists(image_path):
        return "", 404

    return send_file(image_path, mimetype="image/jpeg")

if __name__ == "__main__":
    os.makedirs("static", exist_ok=True)
    app.run(host="0.0.0.0", port=8001)
