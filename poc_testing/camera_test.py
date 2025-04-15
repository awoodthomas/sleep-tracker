import cv2
import json
import time

# Load camera configuration from JSON file
CONFIG_FILE = "/home/awtrpi/src/sleep-tracker/poc_testing/camera_config.json"

def load_camera_config(config_file):
    """Load camera settings from JSON file."""
    with open(config_file, "r") as file:
        return json.load(file)

def set_camera_properties(cap, config):
    """Apply camera settings to the OpenCV VideoCapture object."""
    for prop, value in config.items():
        if hasattr(cv2, prop):  # Ensure the property exists in OpenCV
            cap.set(getattr(cv2, prop), value)
            print(f"Set {prop} to {value}")

def capture_image():
    """Capture an image using OpenCV and save it."""
    config = load_camera_config(CONFIG_FILE)

    # Open the camera
    cap = cv2.VideoCapture(0, cv2.CAP_V4L2)  # Use V4L2 backend on Raspberry Pi

    if not cap.isOpened():
        print("Error: Could not open camera.")
        return

    # Set camera properties
    set_camera_properties(cap, config)

    # Capture frame
    counter = 0
    try:
        while True:
            ret, frame = cap.read()
            if ret:
                cv2.imwrite("/home/awtrpi/images/screenshot.jpg", frame)
                print(f"Screenshot saved as screenshot.jpg, #{counter}")
                counter += 1
            else:
                print("Error: Failed to capture image.")

            time.sleep(5)
    except KeyboardInterrupt:
        cap.release()
        cv2.destroyAllWindows()

if __name__ == "__main__":
    capture_image()
