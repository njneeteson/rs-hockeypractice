# Deploying LocalApp on a home server

LocalApp runs as one Docker container behind your existing HTTPS reverse
proxy, at a path such as `https://miniserver.local/localapp`.

## At a glance

| | |
|---|---|
| Image | `ghcr.io/<your-github-user>/rs-hockeypractice:latest` (built by CI), or build it yourself |
| Port | `8080`, plain HTTP; the proxy provides HTTPS |
| Data | the `/data` volume: `auth.sqlite` (accounts) and `app.sqlite` (app data) |
| URL path | `LOCALAPP_BASE_PATH`, e.g. `/localapp`; the proxy must **not** strip it |
| First start | the log prints a one-time setup link for creating the admin |

## Getting the image onto the server

### Option A (recommended): pull from GitHub Container Registry

The CI workflow publishes an image on every push to `main` and on version tags
(`v0.2.0` → `0.2.0` and `0.2`). The first time, make the package visible to
your server:

- either make it public (GitHub → your profile → *Packages* → `rs-hockeypractice` →
  *Package settings* → *Change visibility*),
- or keep it private and sign the server in once with a personal access token
  that has the `read:packages` scope:
  `echo <token> | docker login ghcr.io -u <your-github-user> --password-stdin`.

For an ARM server (Raspberry Pi), add `linux/arm64` to `PLATFORMS` in
`.github/workflows/ci.yml`.

### Option B: build on your PC, copy the image file

```sh
# On the PC, in the repository (use --platform linux/arm64 for an ARM server)
docker build -t rs-hockeypractice:latest .
docker save rs-hockeypractice:latest | gzip > rs-hockeypractice.tar.gz

# On the server
gunzip -c rs-hockeypractice.tar.gz | docker load
```

## Compose file for the server

```yaml
# ~/localapp/compose.yaml
services:
  localapp:
    image: ghcr.io/<your-github-user>/rs-hockeypractice:latest
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
- Set `LOCALAPP_CLIENT_IP_HEADER=x-forwarded-for` so throttling counts real
  clients rather than the proxy. Only do this when the app can't be reached
  except through the proxy (it publishes no port to the network), since the
  header is taken at its word.

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
4. Create your team under *Teams*, then add the other coaches to it by their
   username. Everyone sees only their own drills and their teams'
   practices; admins manage accounts but don't see other teams.

Instead of the setup link you can set `LOCALAPP_ADMIN_USER` and
`LOCALAPP_ADMIN_PASSWORD` for the first start (remove them afterwards), or run
`docker compose exec localapp localapp-server admin create <name>`.

## All settings

See the configuration table in the [README](README.md#configuration).

## Data, backups and restore

Everything lives in the `/data` volume: `auth.sqlite` (accounts) and
`app.sqlite` (teams, drills, practices).

### Nightly backups (while it runs)

`backup` writes a consistent copy of both databases, safe while the app
runs, and keeps the newest 14 sets (`--keep N` to change that).
`backup-check` opens the newest set and runs SQLite's integrity check.

```sh
docker compose exec localapp localapp-server backup /data/backups
docker compose exec localapp localapp-server backup-check /data/backups
```

The copies are inside the volume, so also copy `/data/backups` off the
machine (another disk, a synced folder): a backup on the same disk doesn't
survive that disk. On Linux, a nightly cron entry for both:

```cron
15 3 * * * cd ~/localapp && docker compose exec -T localapp localapp-server backup /data/backups && docker compose cp localapp:/data/backups /mnt/offsite/localapp
```

To restore a set: stop the app, put the two files in the volume as
`app.sqlite` and `auth.sqlite` (deleting any `-wal`/`-shm` files next to
them), and start it.

### Whole-volume archive

For a full copy of the volume, stop the container briefly:

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

Take a backup before updating (see above).

**Updating to teams and libraries.** Before this version, everything was
shared by everyone. On the first start after updating:

- every existing team gets every existing account as a member, with the
  first admin as its owner and everyone else as coaches;
- each team gets its own copy of every drill its practices use (the
  practices point at those copies, so they look exactly as before);
- all existing drills also go to the first admin's own library.

The log says what was moved. Afterwards, owners can take people out of
teams they shouldn't be in, under *Teams*.

## Maintenance commands

```sh
docker compose exec localapp localapp-server admin list
docker compose exec localapp localapp-server admin reset-password <user>
docker compose exec localapp localapp-server admin create <user>
docker compose exec localapp localapp-server backup /data/backups
docker compose exec localapp localapp-server backup-check /data/backups
```

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Sign-in "works" but you land back on the sign-in page | Served over plain HTTP with secure cookies on. Use HTTPS, or set `LOCALAPP_SECURE_COOKIES=false` for testing. |
| 404 for every page | The proxy strips the prefix, or `LOCALAPP_BASE_PATH` doesn't match the proxied path. |
| "Cross-site request blocked" on every form | The proxy changes `Host` without sending `X-Forwarded-Host`. |
| Container can't write `/data` | A bind-mounted folder must be writable by uid 65532: `sudo chown 65532:65532 ./data`. |
| Locked out | `docker compose exec localapp localapp-server admin reset-password <user>`. |

## Going public with Cloudflare Tunnel

To use the app away from home (e.g. at `https://practice.example.com` on a
domain you own), keep it running on the home server and let a **Cloudflare
Tunnel** carry the traffic. `cloudflared` connects *out* to Cloudflare, so you
open no ports on your router and your home address stays hidden. Cloudflare's
free plan covers the tunnel, DNS, HTTPS and the basic protections below, and
none of it is billed by use. A traffic spike can make the app slow, but it
can't cost you money. [`docker/compose.cloudflare.yaml`](docker/compose.cloudflare.yaml)
is ready for this.

### Steps

1. **DNS on Cloudflare.** Add your domain to a free Cloudflare account and
   switch the domain's nameservers to the two Cloudflare gives you (at your
   registrar). Existing records are imported; check them before switching.
2. **Create the tunnel.** Zero Trust → Networks → Tunnels → *Create a tunnel*
   (Cloudflared). Copy the token from the install command.
3. **Public hostname.** In the tunnel, add `practice.example.com` with service
   `HTTP` and URL `app:8080`.
4. **Start it.**

   ```sh
   cp docker/cloudflare.env.example docker/.env   # paste the token into it
   docker compose -f docker/compose.cloudflare.yaml up -d
   docker compose -f docker/compose.cloudflare.yaml logs app
   ```

   The compose file serves the app at the root of the hostname
   (`LOCALAPP_BASE_PATH=/`), with HTTPS-only cookies and HSTS, takes the client
   address from `CF-Connecting-IP` (`LOCALAPP_CLIENT_IP_HEADER`), keeps the
   app on a network with no internet access of its own, and caps its CPU and
   memory. To move existing data, copy your current volume's `app.sqlite` and
   `auth.sqlite` into the new one, or point `app-data` at it.
5. **Cloudflare settings** for the hostname's zone:
   - SSL/TLS → *Full*; Edge Certificates → *Always Use HTTPS* on.
   - Security → Bots → *Bot Fight Mode* on.
   - Security → WAF → *Rate limiting rules* (one is free): requests whose
     URI path is `/login` and method `POST`, more than 10 a minute from one IP
     → *Block* for 10 minutes. The app throttles sign-in itself too; this
     stops floods before they reach your server.
   - Caching → *Cache Rules*: bypass the cache for everything except
     `/static/*` (pages are private and already say `no-store`).
6. **Optional, strongest: Cloudflare Access.** Zero Trust → Access →
   Applications → *Self-hosted* for the hostname, with a policy allowing your
   coaches' email addresses (one-time codes by email; free for up to 50
   people). Then nobody reaches even the sign-in page without passing
   Cloudflare first. It's one more step for coaches; you can add or remove it
   at any time without changing the app.

### Go-live checklist

- [ ] Running a current image (CI passes `cargo deny`; update with
      `docker compose pull`).
- [ ] Every account has a strong password; remove accounts nobody uses.
- [ ] `LOCALAPP_ADMIN_USER`/`LOCALAPP_ADMIN_PASSWORD` are not set.
- [ ] Teams contain only the right people (after the update above, everyone
      was put in every team).
- [ ] Nightly `backup` runs and the copies leave the machine; you've tried
      a restore once.
- [ ] `https://practice.example.com` shows the sign-in page with a valid
      certificate, and `http://` redirects to it.
- [ ] Ten wrong sign-ins from one browser get blocked (Cloudflare rule) and
      the app's own throttle message shows before that.
- [ ] The home server's OS and Docker get security updates.

### Risks and what covers them

| Risk | Covered by |
|---|---|
| Someone sees or changes another team's data | Every route with an id checks it belongs to one of your teams or your own library (404 otherwise); tests try each one as an outsider. |
| Guessing passwords | Argon2id hashes; the app's throttle (per account and per real client address); Cloudflare's rate-limiting rule; optionally Cloudflare Access. |
| Faking the client address to dodge the throttle | Only `CF-Connecting-IP` is believed, and only cloudflared can reach the app. |
| Losing the data (disk failure, bad update) | Nightly `backup` copied off the machine; `backup-check`; migrations tested on old data. |
| Unexpected bills | Nothing metered: free tunnel, DNS and WAF rules; no paid APIs, email service or storage. CPU and memory caps keep your server usable. |
| Floods and heavy requests | Cloudflare in front; 1 MB request limit; at most 2 PDF renders at once and 60 a minute per user; quotas per team and library. |
| Your home network exposed | No open ports; the app's network has no internet route; read-only, non-root container with no shell. |
| A vulnerable dependency | `cargo deny` in CI, Dependabot updates weekly, actions pinned to commit SHAs. |
| Practice notes mentioning players (often minors) | No analytics or third-party scripts; keep notes to what coaches need. Cloudflare decrypts traffic at its edge; Access adds a gate in front. |
| Uptime depends on your home internet and server | Fine for planning; print the PDF before the rink. The same compose file runs on a small VPS later. |

**Not covered yet:** people can't sign themselves up or reset a forgotten
password (an admin creates accounts and issues temporary passwords), and there
is no email.
