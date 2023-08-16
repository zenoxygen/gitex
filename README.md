# gitex

[![pipeline](https://github.com/zenoxygen/gitex/actions/workflows/ci.yaml/badge.svg)](https://github.com/zenoxygen/gitex/actions/workflows/ci.yaml)

`gitex` is a command-line tool designed to extract data from a Git repository.
It filters commits based on criteria like file extensions, message length and changes length.
The result is saved to a CSV file, enabling further analysis using other processing tools.

## Installation

Build the binary with optimizations:

```sh
cargo build --release
```

Install the binary on your system, for example on Linux:

```sh
sudo install -m 0755 -o root -g root -t /usr/local/bin ./target/release/gitex
```

## Basic usage

```
gitex --repository /path/to/git/repo --output output.csv --size 100 --extensions rs,py
```

This command will analyze the Git repository located at `/path/to/git/repo`, looking for changes in files with the extensions `.rs` and `.py`.
It will extract data for up to 100 commits and save the results in a file named `output.csv`.

## Filter commits on message length

```
gitex --repository /path/to/git/repo --output output.csv --size 100 --extensions py --message-len-min 10 --message-len-max 50
```

This command will analyze commits with messages between 10 and 50 characters in length, focusing on changes to `.py` files.

## Filter commits on changes length

```
gitex --repository /path/to/git/repo --output output.csv --size 100 --extensions rs --changes-len-min 5 --changes-len-max 500
```

This command will analyze commits with changes between 5 and 500 characters in length, focusing on changes to `.rs` files.

## Debug

Run with the environment variable set:

```sh
RUST_LOG=trace gitex --repository /path/to/git/repo --output output.csv --size 100 --extensions rs
```

## License

This project is released under the [MIT License](LICENSE).
