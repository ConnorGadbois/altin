# ALTIN C2

## Docker Setup
Modify the environment variables in `docker-compose.yml` as needed.

```bash
docker compose up -d
```

## Development Setup
### Server
```bash
cd server
cp .env.example .env 

# Modify .env as needed, recomended to use sqlite database

pip install -r requirements.txt

python3 debug_server.py
```

### Web UI
```bash
cd webui
python3 -m http.server # Starts a webserver on port 8000
```

## Implants
|Implant|Description|Language|OS|
|---|---|---|---|
|`Tombili`|Competition focused implant with general purpose functionality|Nim|Windows, Linux| 
|`Midas`|Windows trolling utilities|Rust|Windows