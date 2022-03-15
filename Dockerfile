FROM rust as builder

WORKDIR /app

COPY . .

RUN cargo install --path . && cargo clean

FROM debian:11
WORKDIR /app
COPY --from=builder /usr/local/cargo/bin/audition-dates .
COPY static static
COPY templates templates
COPY templates-mail templates-mail
CMD ["./audition-dates"]
