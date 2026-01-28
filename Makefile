.PHONY: build-linux 

build-linux:
	@echo "Building lightd for Linux (x86_64)..."
	cargo zigbuild --target x86_64-unknown-linux-musl

.PHONY: run
run:
	@echo "build n run"
	cargo build --release
	./target/release/Lightd-v2 --$(filter-out $@,$(MAKECMDGOALS))