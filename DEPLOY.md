# Deploying LocalApp on a home server

LocalApp runs as one Docker container behind your existing HTTPS reverse
proxy, at a path such as `https://miniserver.local/localapp`.

## At a glance

| | |
|---|---|
| Image | `ghcr.io/<your-github-user>/localapp:latest` (built by CI), or build it yourself |
| Port | `8080`, plain HTTP; the proxy provides HTTPS |
| Data | the `/data` volume: `auth.sqlite` (accounts) and `app.sqlite` (app data) |
| URL path | `LOCALAPP_BASE_PATH`, e.g. `/localapp`; the proxy must **not** strip it |
| First start | the log prints a one-time setup link for creating the admin |

## Getting the image onto the server

### Option A (recommended): pull from GitHub Container Registry

The CI workflow publishes an image on every push to `main` and on version tags
(`v0.2.0` → `0.2.0` and `0.2`). The first time, make the package visible to
your server:

- either make it public (GitHub → your profile → *Packages* → `localapp` →
  *Package settings* → *Change visibility*),
- or keep it private and sign the server in once with a personal access token
  that has the `read:packages` scope:
  `echo <token> | docker login ghcr.io -u <your-github-user> --password-stdin`.

For an ARM server (Raspberry Pi), add `linux/arm64` to `PLATFORMS` in
`.github/workflows/ci.yml`.

### Option B: build on your PC, copy the image file

```sh
# On the PC, in the repository (use --platform linux/arm64 for an ARM server)
docker build -t localapp:latest .
docker save localapp:latest | gzip > localapp.tar.gz

# On the server
gunzip -c localapp.tar.gz | docker load
```

## Compose file for the server

```yaml
# ~/localapp/compose.yaml
services:
  localapp:
    image: ghcr.io/<your-github-user>/localapp:latest
    restart: unless-stopped
    environment:
      LOCALAPP_BASE_PATH: /localapp
      TZ: America/Vancouver
    volumes:
      - localapp-data:/data
    ports:
      - "127.0.0.1:8080:8080"   # only the proxy on this machine needs it
    # If the proxy runs in Docker, drop `ports:` and join its network instead:
    # networks: [proxy]

volumes:
  localapp-data:

# networks:
#   proxy:
#     external: true
```

```sh
docker compose up -d
docker compose logs localapp      # the setup link is printed here
```

## Reverse proxy requirements

- Forward the path **with** its prefix (`/localapp/...` arrives as
  `/localapp/...`).
- Send `X-Forwarded-Proto` (so the app can warn when HTTPS is missing),
  `X-Forwarded-For` (sign-in throttling per client) and `X-Forwarded-Host`
  or the original `Host` (cross-site checks).

Ready-made examples are in [`docker/`](docker/):
[`Caddyfile.example`](docker/Caddyfile.example) (use `handle`, not `handle_path`),
[`nginx.conf.example`](docker/nginx.conf.example) (no URI after the upstream),
[`traefik.example.yaml`](docker/traefik.example.yaml) (no StripPrefix), and
[`compose.with-caddy.yaml`](docker/compose.with-caddy.yaml) for a server that
has no proxy yet.

## First start

1. Start the container and read the log: `docker compose logs localapp`.
2. Open the printed `/localapp/setup?token=…` link through your proxy and
   create the admin account. The link stops working once an admin exists.
3. Add other people under *Users*. Each gets a temporary password to pass on;
   they choose their own at first sign-in.

Instead of the setup link you can set `LOCALAPP_ADMIN_USER` and
`LOCALAPP_ADMIN_PASSWORD` for the first start (remove them afterwards), or run
`docker compose exec localapp localapp-server admin create <name>`.

## All settings

See the configuration table in the [README](README.md#configuration).

## Data, backups and restore

Everything lives in the `/data` volume. For a consistent backup, stop the
container briefly or copy the SQLite files together with their `-wal`
files:

```sh
docker compose stop localapp
docker run --rm -v localapp_localapp-data:/data -v "$PWD":/backup alpine \
  tar czf /backup/localapp-data-$(date +%F).tar.gz -C /data .
docker compose start localapp
```

To restore, stop the container, unpack the archive into the volume the same
way, and start it again.

## Updating

```sh
docker compose pull
docker compose up -d
```

Database migrations run automatically at start-up. Pin a version tag (e.g.
`:0.2`) instead of `latest` if you want to update deliberately.

## Maintenance commands

```sh
docker compose exec localapp localapp-server admin list
docker compose exec localapp localapp-server admin reset-password <user>
docker compose exec localapp localapp-server admin create <user>
```

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Sign-in "works" but you land back on the sign-in page | Served over plain HTTP with secure cookies on. Use HTTPS, or set `LOCALAPP_SECURE_COOKIES=false` for testing. |
| 404 for every page | The proxy strips the prefix, or `LOCALAPP_BASE_PATH` doesn't match the proxied path. |
| "Cross-site request blocked" on every form | The proxy changes `Host` without sending `X-Forwarded-Host`. |
| Container can't write `/data` | A bind-mounted folder must be writable by uid 65532: `sudo chown 65532:65532 ./data`. |
| Locked out | `docker compose exec localapp localapp-server admin reset-password <user>`. |
