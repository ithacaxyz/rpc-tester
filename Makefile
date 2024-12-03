fmt:
	cargo +nightly fmt

fix:
	cargo +nightly clippy \
	--workspace \
	--bin "rpc-tester-cli" \
	--lib \
	--tests \
	--benches \
	--fix \
	--allow-staged \
	--allow-dirty \
	-- -D warnings && make fmt

.PHONY: maxperf
maxperf: 
	RUSTFLAGS="-C target-cpu=native" cargo build --profile maxperf 
