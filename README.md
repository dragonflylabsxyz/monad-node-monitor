## Monad Node Monitoring via Telegram

- Telegram-Based Block Height Monitoring tool for `fullnodes` and `validator` nodes
  - Fetches the latest block height from your Monad node
  - Sends it as a Telegram message to a dedicated channel
  - Checks the node's sync status automatically
  - No dashboards. No browser needed.
  - Just a clean, systemd or cron-backed heartbeat notification from your node — straight to Telegram.

---

### Prerequisites

| Tool | Purpose |
|------|---------|
| [Rust + Cargo](https://rustup.rs) | Build binary locally |
| [Docker](https://docs.docker.com/get-docker/) | Build & run containerised |
| systemd | Manage the service on Linux |

---

### Configure `.env`

All three deployment paths read credentials from a `.env` file.

```sh
cp .env.sample .env
vim .env
```

```sh
# .env
TELEGRAM_TOKEN=804xxxx:AAEymFxxxxxxxxxxxxxxxxxxxxxxxxx
TELEGRAM_CHAT_ID=-38xxxxxx
RPC_PORT=8080   # default; adjust if your node listens on a different port
```

- How to get a [Telegram Bot Token](https://core.telegram.org/bots/features#creating-a-new-bot)
- How to get your [Telegram Chat ID](https://neliosoftware.com/content/help/how-do-i-get-the-channel-id-in-telegram/)

---

### Option A — Build & run locally (bare metal)

```sh
git clone https://github.com/dragonflylabsxyz/monad-node-monitor.git
cd monad-node-monitor

# Build a release binary
cargo build --release

# Run (reads .env automatically)
./target/release/monad-monitoring
```

#### Automated systemd install

```sh
# Installs binary, creates 'monad' user, writes service file, starts service
sudo ./install.sh
```

```sh
# Uninstall
sudo ./install.sh --uninstall
```

The install script:
1. Creates a `monad` system user
2. Copies the binary to `/usr/local/bin/monad-monitoring`
3. Creates `/opt/monad-node-monitor/` as the working directory (state files live here)
4. Writes an `.env` template if one is not already present
5. Installs `monad-monitoring.service`, enables it, and starts it

```sh
# View live logs
journalctl -u monad-monitoring -f
```

---

### Option B — Docker

#### Build image

```sh
docker build -t monad-node-monitor:latest .
```

The Dockerfile uses a two-stage build:

| Stage | Image | Purpose |
|-------|-------|---------|
| `builder` | `rust:1.78-alpine` | Compiles a fully static musl binary |
| runtime | `scratch` | Minimal — only the binary + CA certs |

Final image size is typically **5–8 MB**.

#### Run container

```sh
docker run -d \
  --name monad-node-monitor \
  --restart unless-stopped \
  --env-file .env \
  --network host \
  -v monad-state:/data \
  monad-node-monitor:latest
```

> `--network host` lets the container reach your Monad node on `localhost`.  
> The named volume `monad-state` persists `.last_height` / `.last_status` across restarts.

#### Docker Compose (optional)

```yaml
services:
  monad-node-monitor:
    build: .
    restart: unless-stopped
    env_file: .env
    network_mode: host
    volumes:
      - monad-state:/data

volumes:
  monad-state:
```

```sh
docker compose up -d
docker compose logs -f
```

---

### Option C — systemd service (manual)

```sh
sudo vim /etc/systemd/system/monad-monitoring.service
# paste the contents of monad-monitoring.service

sudo systemctl daemon-reload
sudo systemctl enable monad-monitoring
sudo systemctl start monad-monitoring
```

---

### Running tests

```sh
cargo test
```

---

### Expected Outcome

The Telegram bot reports three states:

| Status | Emoji | Message |
|--------|-------|---------|
| UP | ✅ | `Monad RPC is back UP! Current height: <height>` |
| Stuck | ⚠️ | `Monad node stuck at height: <height>` |
| DOWN | 🚨 | `Monad RPC is DOWN! Unable to connect to port.` |

> THE POSSIBILITY OF SUCH DAMAGE.
