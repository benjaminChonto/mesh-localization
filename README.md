# Mesh localization

This project localizes ESP32-C3 devices using RSSI with the ESP-NOW protocol.

## Commands
```bash
# Run code
docker compose up -d
cargo run-firmware
cargo run-ui
# Set board ID
ID=1 cargo run-firmware
# Attach terminal to running board
espflash monitor
# Make sure to format your code
cargo fmt --all
```

## Project structure
* Docker compose starts a Mosquitto MQTT on port 1883
* `esp32-firmware` contains MCU related code
* `mesh-ui` is a Ratatui desktop application, communicating with the nodes by subscribing to the right MQTT topics

## Architecture
// TODO
