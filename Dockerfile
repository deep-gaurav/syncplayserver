FROM debian:bookworm-slim
COPY target/release/syncplayserver /usr/local/bin/app
ENTRYPOINT ["/usr/local/bin/app"]