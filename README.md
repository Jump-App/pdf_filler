# pdf_filler

A Rust CLI binary that fills PDF form fields from JSON data.

## Build

```sh
cargo build --release
```

The binary is produced at `target/release/pdf_filler`.

## Downloading Released Binaries

Prebuilt binaries are published on GitHub Releases for:

- `x86_64-unknown-linux-gnu`
- `aarch64-apple-darwin`
- `x86_64-apple-darwin`

Release assets follow this naming contract:

- `pdf_filler-x86_64-unknown-linux-gnu`
- `pdf_filler-aarch64-apple-darwin`
- `pdf_filler-x86_64-apple-darwin`

Download URLs follow this shape:

```sh
https://github.com/<owner>/<repo>/releases/download/<tag>/pdf_filler-<target>
```

Example:

```sh
curl -L -o pdf_filler \
  https://github.com/<owner>/<repo>/releases/download/v0.1.0/pdf_filler-x86_64-unknown-linux-gnu
chmod +x pdf_filler
```

## Usage

```sh
pdf_filler <template.pdf> <data.json> <output.pdf>
```

- `template.pdf` — the PDF template containing AcroForm fields
- `data.json` — a flat JSON object mapping field names to values
- `output.pdf` — path for the filled PDF output

### JSON format

A flat `string → value` map where keys are fully-qualified PDF field names (dot-separated for hierarchical fields). Values can be strings, numbers, or booleans — all are converted to strings. Null values are skipped.

```json
{
  "ClientName": "Jane Doe",
  "Account.Number": "12345",
  "IsRetired": true,
  "Age": 65
}
```

### Listing field names

Set the `PDF_DUMP_FIELDS` environment variable to print all discovered field names from a template and exit:

```sh
PDF_DUMP_FIELDS=1 pdf_filler template.pdf _ _
```

Field names are printed to stdout (one per line). The total count is printed to stderr.

## Elixir integration

Call the binary from Elixir using `System.cmd/3`:

```elixir
json_path = Path.join(tmp_dir, "data.json")
output_path = Path.join(tmp_dir, "output.pdf")

File.write!(json_path, Jason.encode!(field_data))

{output, exit_code} =
  System.cmd("pdf_filler", [template_path, json_path, output_path],
    stderr_to_stdout: true
  )

case exit_code do
  0 -> {:ok, output_path}
  _ -> {:error, output}
end
```

Status information (field count, errors) is written to stderr. The binary exits with code 0 on success, 1 on failure.

## Testing

Run all tests (unit + integration):

```sh
cargo test
```

### Test structure

- **Unit tests** — `#[cfg(test)]` modules in `src/fill.rs` and `src/strip.rs`
- **Integration tests** — `tests/integration.rs` exercises the binary end-to-end
- Test PDFs are built programmatically with `lopdf` — no binary fixtures needed

### Manual e2e test

A pre-built test PDF and sample JSON are provided in `tests/fixtures/`:

```sh
cargo build --release

# Fill the test form
./target/release/pdf_filler tests/fixtures/test_form.pdf tests/fixtures/test_data.json /tmp/output.pdf

# List all fields in the test form
PDF_DUMP_FIELDS=1 ./target/release/pdf_filler tests/fixtures/test_form.pdf _ _

# Open the filled PDF to verify
open /tmp/output.pdf   # macOS
```

### Regenerating the test fixture

If you need to modify the test form fields:

```sh
cargo run --example generate_fixture
# This overwrites tests/fixtures/test_form.pdf
```

## Releasing updates

1. Update the version in `Cargo.toml`.
2. Run the local checks:

```sh
cargo test
```

3. Push a version tag such as `v0.1.0`.
4. GitHub Actions builds the supported targets and uploads them to the matching GitHub Release.

Only version tags publish release assets. Branch pushes and pull requests only run CI.
