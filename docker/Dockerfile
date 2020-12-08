FROM ubuntu

# Set non-interactive apt install & install deps
ENV DEBIAN_FRONTEND=noninteractive
RUN apt update && apt install -y \
        libssl-dev \
        libglib2.0-dev \
        libpango1.0-dev \
        libgtk-3-dev \
        libsoup2.4-dev \
        libwebkit2gtk-4.0-dev \
        build-essential \
        gcc-mingw-w64-x86-64 \
        curl

# Install rust and Windows CPU Arch
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
RUN /root/.cargo/bin/rustup target add x86_64-pc-windows-gnu

# Set Entrypoint build directory and arguments
ENTRYPOINT cd rpatchur && /root/.cargo/bin/cargo build --target x86_64-pc-windows-gnu --release
