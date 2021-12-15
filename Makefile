.PHONY: install clean

target/release/jxact: src
	cargo build --release

install:
	cargo install --path .

clean:
	cargo clean
