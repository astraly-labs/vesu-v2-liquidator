<div align="center">
  <h1>Vesu Liquidator (v2)</h1>
  <img src="docs/images/logo.jpeg" height="400" width="400">
  <br />
</div>

## About

Vesu Liquidator 🤖 is an automated bot that monitors positions on the Vesu V2 Protocol and liquidates them.

## Getting Started

### Prerequisites

#### Protobuf

In order to run the liquidator, you need the protoc Protocol Buffers compiler, along with Protocol Buffers resource files.

##### Ubuntu

```sh
sudo apt install -y protobuf-compiler libprotobuf-dev
```

##### macOS

Assuming Homebrew is already installed.

```sh
brew install protobuf
```

#### Environment Variables

Create an `.env` file following the example file and fill the keys.

## Usage

```shell
RUST_LOG="info" cargo run --release
```

## Contributing

First off, thanks for taking the time to contribute! Contributions are what make the open-source community such an amazing place to learn, inspire, and create. Any contributions you make will benefit everybody else and are **greatly appreciated**.

Please read [our contribution guidelines](docs/CONTRIBUTING.md), and thank you for being involved!

## Security

We follows good practices of security, but 100% security cannot be assured.
The bot is provided **"as is"** without any **warranty**. Use at your own risk.

_For more information and to report security issues, please refer to our [security documentation](docs/SECURITY.md)._

## License

This project is licensed under the **MIT license**.

See [LICENSE](LICENSE) for more information.

