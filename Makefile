IP_ADDR := $(shell ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null)

up:
	IP_ADDR=$(IP_ADDR) docker compose up --build

build:
	IP_ADDR=$(IP_ADDR) docker compose build

down:
	docker compose down
