# syntax=docker/dockerfile:1.7

# Pin to bookworm so the binary links against the same glibc (2.36) as the
# runtime below; rust:latest tracks a newer Debian and would require a glibc
# the bookworm-slim runtime can't provide.
FROM rust:1-bookworm AS chef
WORKDIR /app
RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
	--mount=type=cache,target=/app/target \
	cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
	--mount=type=cache,target=/app/target \
	cargo build --release --bin axochat \
	&& cp /app/target/release/axochat /app/axochat

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
	&& apt-get install -y --no-install-recommends ca-certificates \
	&& rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/axochat /app/axochat

EXPOSE 8080
ENTRYPOINT ["/app/axochat"]
CMD ["start"]
