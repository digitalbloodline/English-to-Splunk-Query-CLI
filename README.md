# English to Splunk Query CLI

Terminal UI tool that translates plain English into Splunk Processing Language (SPL) using the OpenAI Chat Completions API.

## Requirements

- Rust toolchain (stable)
- `OPENAI_API_KEY` set in environment or `.env`

## Setup

1. Copy `.env.example` to `.env`.
2. Set your API key.
3. Run:

```bash
cargo run
```

## Usage

- Type an English query in the top panel.
- Press `Enter` to translate it into SPL.
- Press `Esc` to quit.

## Notes

- Input is capped to 500 characters.
- HTTP request timeout is 20 seconds.
