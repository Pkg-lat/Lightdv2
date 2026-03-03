.PHONY: build-linux 

build-linux:
	@echo "Building lightd for Linux (x86_64)..."
	cargo zigbuild --target x86_64-unknown-linux-musl

.PHONY: lightd-arm-release
lightd-arm-release:
	@echo "Building lightd for ARM (aarch64)..."
	cargo zigbuild --release --target aarch64-unknown-linux-musl
	@echo "ARM build complete: target/aarch64-unknown-linux-musl/release/Lightd-v2"

.PHONY: run
run:
	@echo "build n run"
	cargo build --release
	./target/release/Lightd-v2 --$(filter-out $@,$(MAKECMDGOALS))