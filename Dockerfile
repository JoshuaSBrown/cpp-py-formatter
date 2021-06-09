# Build the GitHub Action
FROM rust:1.42 as builder
WORKDIR /usr/src/myapp
COPY Cargo.toml .
COPY Cargo.lock .
COPY src ./src
RUN cargo install --path .

# GitHub Action Image
FROM ubuntu:18.04
# Install our apt packages
RUN apt-get update
RUN apt-get upgrade -y
RUN apt-get install -y git
RUN apt-get install -y python3-pip
RUN python3 -m pip install --upgrade pip
RUN python3 -m pip install black

# Install clang-formats
ADD ./clang-format /clang-format
COPY --from=builder /usr/local/cargo/bin/cpp-py-format /cpp-py-format
ENTRYPOINT [ "/cpp-py-format" ]
