# Mesh localization

This project localizes ESP32-C3 devices using RSSI with the ESP-NOW protocol.

## Commands
```bash
# Run code
cargo run --release
# Set board ID
ID=1 cargo run --release
# Attach terminal to running board
espflash monitor
# Make sure to format your code
cargo clippy
```