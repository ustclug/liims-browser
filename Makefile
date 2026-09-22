PREFIX ?= /usr
DESTDIR ?=

.PHONY: build test check install
build:
	cargo build --release --locked

test:
	cargo test --locked

check:
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings

install:
	install -Dm755 target/release/liims-browser $(DESTDIR)$(PREFIX)/bin/liims-browser
	install -Dm644 data/browser.toml $(DESTDIR)/etc/liims/browser.toml
	install -Dm644 packaging/liims-browser.service $(DESTDIR)$(PREFIX)/lib/systemd/user/liims-browser.service
	install -Dm644 packaging/cn.edu.ustc.liims.Browser.desktop $(DESTDIR)$(PREFIX)/share/applications/cn.edu.ustc.liims.Browser.desktop
