FROM debian:bookworm-slim
COPY target/release/syncplayserver /usr/local/bin
ENTRYPOINT ["/usr/local/bin/app"]