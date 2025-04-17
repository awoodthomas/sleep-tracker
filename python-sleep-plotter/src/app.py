from flask import Flask, render_template, jsonify, send_file, request
import h5py
import os
import matplotlib.pyplot as plt

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


@app.route("/data")
def get_data():
    with h5py.File(HDF5_PATH, "r") as f:
        g = f[request.args.get("group")]
        timestamps = g["timestamp"][:].tolist()
        temperature = g["temperature"][:].tolist()
        pressures = g["pressure"][:].tolist()
        humidity = g["humidity"][:].tolist()
        co2 = g["co2eq_ppm"][:].tolist()
        tvoc = g["tvoc_ppb"][:].tolist()
        aqi = g["air_quality_index"][:].tolist()
        image_paths = g.get("image_path", [])[:].astype(str).tolist()

    return jsonify({
        "timestamp": timestamps,
        "temperature": temperature,
        "pressure": pressures,
        "humidity": humidity,
        "co2eq_ppm": co2,
        "tvoc_ppb": tvoc,
        "air_quality_index": aqi,
        "image_paths": image_paths
    })

@app.route("/preview")
def preview_image():
    image_path = request.args.get("path")
    if not image_path or not os.path.exists(image_path):
        return "", 404

    return send_file(image_path, mimetype="image/jpeg")

if __name__ == "__main__":
    os.makedirs("static", exist_ok=True)
    app.run(host="0.0.0.0", port=8000)
