## Build step
FROM rust:1.60.0-slim-buster as builder
WORKDIR /usr/src/rails
COPY . .
RUN cargo install --path .

## Execution
FROM debian:buster-slim
COPY --from=builder /usr/local/cargo/bin/rails /usr/local/bin/rails
CMD ["rails", "/etc/rails/input.csv"]
