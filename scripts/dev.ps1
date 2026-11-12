# Run the server locally at http://127.0.0.1:8080/localapp with data in ./data.
# Plain HTTP, so secure cookies are off. The first-run setup link is printed below.
#
#   ./scripts/dev.ps1
#   ./scripts/dev.ps1 admin list       # any localapp-server command

Set-Location (Join-Path $PSScriptRoot '..')
$env:LOCALAPP_DATA_DIR = './data'
$env:LOCALAPP_BASE_PATH = '/localapp'
$env:LOCALAPP_BIND = '127.0.0.1:8080'
$env:LOCALAPP_SECURE_COOKIES = 'false'
if (-not $env:RUST_LOG) { $env:RUST_LOG = 'info,tower_http=debug' }
cargo run -p localapp-server -- @args
