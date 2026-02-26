# dev-hud recipes

build:
    cargo build --release

deploy: build
    systemctl --user restart dev-hud

status:
    systemctl --user status dev-hud

logs:
    journalctl --user -u dev-hud -f

install:
    ./setup.sh install
