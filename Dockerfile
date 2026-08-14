# Minimal appliance image: native supervisor + stdio MCP.
# Build: docker build -t ruflo-appliance .
# Run:   docker run --rm ruflo-appliance

FROM rust:1.87-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p ruflo --no-default-features

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 ruflo
WORKDIR /home/ruflo/workspace
COPY --from=build /src/target/release/ruflo /usr/local/bin/ruflo
USER ruflo
EXPOSE 0
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
  CMD ruflo daemon status || exit 1
ENTRYPOINT ["ruflo", "daemon", "start", "--foreground", "--ttl", "0"]
